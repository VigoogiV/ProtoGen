//! proto 源码注释扫描。
//!
//! 为什么不用 `SourceCodeInfo` 的注释归属：
//! protoc 会把行尾的 `/** ... **/` 块注释算成**下一个元素**的 leading comment，
//! 导致整体串行（`bf_ibcp.proto` 里 695 行行尾注释全都会错位）。
//!
//! 所以这里自己按行扫描，复刻旧工具（C# ProtoBufScript）的判断规则，
//! 同时用 AST 的行号来定位元素，避免旧工具"靠字符串猜字段名"的缺陷。

/// 压缩空白后的源码行 + 该行注释信息
#[derive(Debug, Clone, Default)]
pub struct SourceLine {
    /// 原始行内容
    pub raw: String,
    /// 压缩空白后的行（tab/多空格 → 单空格，首尾 trim），用于解析
    pub code: String,
    /// 该行代码之后的注释原文（含 `//` 或 `/* */` 定界符）
    pub trailing: Option<String>,
    /// 是否为纯注释行（压缩后以 `//` 或 `/*` 开头）
    pub is_pure_comment: bool,
}

/// 扫描结果：行数组 + 原始行号到压缩行号的映射
#[derive(Debug, Default)]
pub struct ScanResult {
    pub lines: Vec<SourceLine>,
    /// `orig_to_compact[i]` = 原始第 i 行在 `lines` 里的下标（空行被移除，映射为 None）
    pub orig_to_compact: Vec<Option<usize>>,
}

impl ScanResult {
    /// 用原始行号（0 基）取压缩后的行
    pub fn line_at(&self, orig_line: u32) -> Option<&SourceLine> {
        self.orig_to_compact
            .get(orig_line as usize)
            .and_then(|v| v.as_ref())
            .map(|&i| &self.lines[i])
    }

    /// 用原始行号取压缩后的下标
    pub fn index_of(&self, orig_line: u32) -> Option<usize> {
        self.orig_to_compact
            .get(orig_line as usize)
            .and_then(|v| *v)
    }

    /// 向上收集连续的纯注释行（复刻旧工具 `getNotes` 的行为）。
    ///
    /// 返回按源码顺序排列的注释文本（每项含定界符）。
    /// `compact_idx` 是元素所在行的压缩下标，从它的上一行开始向上找。
    pub fn leading_comments(&self, compact_idx: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut i = compact_idx;
        while i > 0 {
            i -= 1;
            let line = &self.lines[i];
            if line.is_pure_comment {
                out.push(squeeze(&line.raw));
            } else {
                break;
            }
        }
        out.reverse();
        out
    }

    /// 取某行的行尾注释（代码之后的部分）
    pub fn trailing_comment(&self, compact_idx: usize) -> Option<String> {
        self.lines
            .get(compact_idx)
            .and_then(|l| l.trailing.clone())
    }
}

/// 把任意空白序列压成单个空格并 trim，等价于 C# 的
/// `Regex.Replace(s, @"\s", " ")` + `Split(' ', RemoveEmptyEntries)` + `Join(" ")`
pub fn squeeze(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 扫描 proto 源码，产出按「移除空行后」组织的行数组。
pub fn scan(src: &str) -> ScanResult {
    let raw_lines: Vec<&str> = src.split('\n').collect();
    let mut result = ScanResult {
        lines: Vec::new(),
        orig_to_compact: vec![None; raw_lines.len()],
    };

    let mut in_block = false;

    for (orig_idx, raw) in raw_lines.iter().enumerate() {
        let raw_trim_nl = raw.strip_suffix('\r').unwrap_or(raw);

        // 复刻旧工具：只把"仅含空格"的行当空行移除（含 tab 的行保留）
        if !in_block && raw_trim_nl.replace(' ', "").is_empty() {
            continue;
        }

        let (code, trailing, is_pure) = classify_line(raw_trim_nl, &mut in_block);

        // 纯空白行（例如块注释内部）不参与后续解析，但保留占位以免打乱行号
        if code.is_empty() && !is_pure && trailing.is_none() && !in_block {
            continue;
        }

        let compact_idx = result.lines.len();
        result.orig_to_compact[orig_idx] = Some(compact_idx);
        result.lines.push(SourceLine {
            raw: raw_trim_nl.to_string(),
            code,
            trailing,
            is_pure_comment: is_pure,
        });
    }

    result
}

/// 分析一行：返回 (压缩后的代码部分, 行尾注释, 是否纯注释行)
fn classify_line(line: &str, in_block: &mut bool) -> (String, Option<String>, bool) {
    let bytes: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut code_end = 0usize; // 代码部分的结束位置（字符下标）
    let mut comment_start: Option<usize> = None;
    let mut in_str: Option<char> = None;

    while i < bytes.len() {
        let c = bytes[i];

        if *in_block {
            if c == '*' && i + 1 < bytes.len() && bytes[i + 1] == '/' {
                *in_block = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if let Some(q) = in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                in_str = None;
            }
            code_end = i + 1;
            i += 1;
            continue;
        }

        if c == '"' || c == '\'' {
            in_str = Some(c);
            code_end = i + 1;
            i += 1;
            continue;
        }

        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == '/' {
            comment_start = Some(i);
            break;
        }

        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == '*' {
            comment_start = Some(i);
            // 检查是否在本行闭合
            let mut j = i + 2;
            let mut closed = false;
            while j + 1 < bytes.len() {
                if bytes[j] == '*' && bytes[j + 1] == '/' {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if !closed {
                *in_block = true;
            }
            break;
        }

        if !c.is_whitespace() {
            code_end = i + 1;
        }

        i += 1;
    }

    let code_part: String = bytes[..code_end].iter().collect();
    let code = squeeze(&code_part);

    let (trailing, is_pure) = match comment_start {
        Some(cs) => {
            // 复刻旧工具：整行先做空白压缩（`Regex.Replace(s, @"\s", " ")`
            // + Split/Join），再从注释标记开始截取
            let comment_raw: String = bytes[cs..].iter().collect();
            let comment = squeeze(&comment_raw);
            let is_pure = code.is_empty();
            let trailing = if is_pure {
                None
            } else {
                // C# 用 `Substring(分号位置+1)` 取行尾注释，保留了分隔空白，
                // 这里等价地判断代码与注释之间是否有空白
                let gap: String = bytes[code_end..cs].iter().collect();
                if gap.chars().all(|c| c.is_whitespace()) && !gap.is_empty() {
                    Some(format!(" {}", comment))
                } else {
                    Some(comment)
                }
            };
            (trailing, is_pure)
        }
        None => (None, false),
    };

    (code, trailing, is_pure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squeeze_collapses_whitespace() {
        assert_eq!(squeeze("  a   b\t c  "), "a b c");
    }

    #[test]
    fn detects_pure_comment_line() {
        let r = scan("/** 注释 **/\nmessage FOO {\n}\n");
        assert!(r.lines[0].is_pure_comment);
        assert_eq!(r.lines[0].raw.trim(), "/** 注释 **/");
        assert_eq!(r.lines[1].code, "message FOO {");
    }

    #[test]
    fn detects_trailing_comment() {
        let r = scan("uint32 a = 1;  /** 行尾 **/\n");
        assert!(!r.lines[0].is_pure_comment);
        assert_eq!(r.lines[0].code, "uint32 a = 1;");
        // 保留一个前导空格，复刻 C# 从分号之后截取的行为
        assert_eq!(r.lines[0].trailing.as_deref(), Some(" /** 行尾 **/"));
    }

    #[test]
    fn trailing_comment_without_gap() {
        let r = scan("uint32 a = 1;/** 行尾 **/\n");
        assert_eq!(r.lines[0].trailing.as_deref(), Some("/** 行尾 **/"));
    }

    #[test]
    fn leading_comments_walks_up() {
        let src = "/** 一 */\n/** 二 */\nmessage FOO {\n}\n";
        let r = scan(src);
        let idx = r.index_of(2).unwrap();
        let lead = r.leading_comments(idx);
        assert_eq!(lead.len(), 2);
        assert_eq!(lead[0], "/** 一 */");
        assert_eq!(lead[1], "/** 二 */");
    }

    #[test]
    fn ignores_slashes_inside_string() {
        let r = scan("option x = \"http://a\";\n");
        assert_eq!(r.lines[0].code, "option x = \"http://a\";");
        assert!(r.lines[0].trailing.is_none());
    }
}
