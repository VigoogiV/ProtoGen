//! `.options` 文件生成与校验。
//!
//! 从 proto 注释里提取字段上限，生成 nanopb 的 options 模板：
//!
//! - `bytes` / `string` 字段 → `max_size:N`（单个元素的最大字节数）
//! - `repeated` 字段 → `max_count:N`（最多几个元素）
//! - 既是 repeated 又是 bytes/string → 两个都要
//!
//! 注释写法与旧工具一致，行尾或上方都可以：
//!
//! ```text
//! bytes  data  = 1;   /** [max_size:64] 数据 **/
//! /** [max_count:8] 条目列表 **/
//! repeated Item items = 2;
//! ```
//!
//! 输出格式：`Msg.field` + 空格补齐 + `\t\t\t` + 选项，ASCII + CRLF。

use crate::model::{FieldDef, ProtoFile, Rules};

/// 从一段注释里提取 `[max_size:N]`
pub fn extract_max_size(text: &str) -> Option<u32> {
    let idx = text.find("[max_size:")?;
    let rest = &text[idx + "[max_size:".len()..];
    let end = rest.find(']')?;
    rest[..end].trim().parse::<u32>().ok()
}

/// 从一段注释里提取 `[max_count:N]`
pub fn extract_max_count(text: &str) -> Option<u32> {
    let idx = text.find("[max_count:")?;
    let rest = &text[idx + "[max_count:".len()..];
    let end = rest.find(']')?;
    rest[..end].trim().parse::<u32>().ok()
}

/// 一个字段在 `.options` 里声明的上限
#[derive(Debug, Clone)]
pub struct FieldLimits {
    /// 点分 proto 全名，如 `Msg.field`
    pub key: String,
    /// 注释里声明的 max_size
    pub max_size: Option<u32>,
    /// 注释里声明的 max_count
    pub max_count: Option<u32>,
    /// 该字段是否需要 max_size（bytes / string）
    pub needs_max_size: bool,
    /// 该字段是否需要 max_count（repeated）
    pub needs_max_count: bool,
}

fn is_bytes_or_string(f: &FieldDef) -> bool {
    f.pbtype == "BYTES" || f.pbtype == "STRING" || f.pbtype == "FIXED_LENGTH_BYTES"
}

/// 从字段注释里提取 max_size 与 max_count。
/// 行尾注释优先，其次从紧邻的上方注释里由下往上找。
fn extract_limits(f: &FieldDef) -> (Option<u32>, Option<u32>) {
    let mut max_size = None;
    let mut max_count = None;

    if let Some(t) = &f.comment.trailing {
        max_size = extract_max_size(t);
        max_count = extract_max_count(t);
    }

    for line in f.comment.leading.iter().rev() {
        if max_size.is_none() {
            max_size = extract_max_size(line);
        }
        if max_count.is_none() {
            max_count = extract_max_count(line);
        }
        if max_size.is_some() && max_count.is_some() {
            break;
        }
    }

    (max_size, max_count)
}

/// 收集所有需要上限声明的字段（bytes/string 或 repeated）
pub fn collect_limits(file: &ProtoFile) -> Vec<FieldLimits> {
    let mut out = Vec::new();

    for m in &file.messages {
        for f in &m.fields {
            let needs_max_size = is_bytes_or_string(f);
            let needs_max_count = f.rules == Rules::Repeated;
            if !needs_max_size && !needs_max_count {
                continue;
            }

            let (mut max_size, mut max_count) = extract_limits(f);
            // 类型不支持的上限直接丢掉，避免生成 nanopb 不认的键
            if !needs_max_size {
                max_size = None;
            }
            if !needs_max_count {
                max_count = None;
            }

            out.push(FieldLimits {
                // .options 的键用点分 proto 全名，嵌套层级用 `.` 分隔
                key: format!("{}.{}", m.dotted_name, f.name),
                max_size,
                max_count,
                needs_max_size,
                needs_max_count,
            });
        }
    }

    out
}

/// 生成 `.options` 模板内容（LF 换行，调用方按需转 CRLF）
///
/// - bytes/string：总是输出一行，没写值就留 `max_size:` 占位，提醒补全
/// - repeated：只有注释里写了 `[max_count:N]` 才输出。
///   不配 max_count 也是合法的（退化成 callback），没必要用占位行刷屏。
pub fn generate_options(file: &ProtoFile) -> String {
    let entries: Vec<FieldLimits> = collect_limits(file)
        .into_iter()
        .filter(|e| e.needs_max_size || e.max_count.is_some())
        .collect();

    if entries.is_empty() {
        return String::new();
    }

    let max_len = entries.iter().map(|e| e.key.len()).max().unwrap_or(0);

    let mut out = String::new();
    for e in &entries {
        let mut parts: Vec<String> = Vec::new();

        if e.needs_max_size {
            parts.push(match e.max_size {
                Some(v) => format!("max_size:{}", v),
                None => "max_size:".to_string(),
            });
        }
        if e.needs_max_count {
            if let Some(v) = e.max_count {
                parts.push(format!("max_count:{}", v));
            }
        }
        if parts.is_empty() {
            continue;
        }

        let pad = max_len.saturating_sub(e.key.len());
        out.push_str(&e.key);
        out.push_str(&" ".repeat(pad));
        out.push_str("\t\t\t");
        out.push_str(&parts.join(" "));
        out.push('\n');
    }
    out
}

/// 校验结果
#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    /// bytes/string 字段缺少 max_size（错误，会退化成 callback）
    pub missing: Vec<String>,
    /// .options 里写了 `max_size:` 但没填值
    pub blank_value: Vec<String>,
    /// repeated 字段没配 max_count（提示，仍可用但会退化成 callback）
    pub no_max_count: Vec<String>,
    /// .options 里定义了，但 proto 里找不到对应字段
    pub extra: Vec<String>,
}

impl CheckReport {
    pub fn is_ok(&self) -> bool {
        self.missing.is_empty() && self.blank_value.is_empty()
    }
}

/// 校验 `.options` 是否覆盖了所有需要上限的字段
pub fn check(file: &ProtoFile, options_map: &crate::parse::OptionsMap) -> CheckReport {
    let mut report = CheckReport::default();
    let entries = collect_limits(file);

    for e in &entries {
        let opt = options_map.get(&e.key);

        // bytes/string 缺 max_size 是错误
        if e.needs_max_size {
            match opt {
                None => report.missing.push(e.key.clone()),
                Some(o) => {
                    if o.max_size.is_none() {
                        report.blank_value.push(e.key.clone());
                    }
                }
            }
        }

        // repeated 缺 max_count 只是提示：不配也能用，只是退化成 callback
        if e.needs_max_count && opt.map_or(true, |o| o.max_count.is_none()) {
            report.no_max_count.push(e.key.clone());
        }
    }

    let proto_keys: std::collections::HashSet<&String> =
        entries.iter().map(|e| &e.key).collect();
    for key in options_map.keys() {
        if !proto_keys.contains(key) {
            report.extra.push(key.clone());
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_max_size_with_space() {
        assert_eq!(extract_max_size("/** [max_size:548] 说明 **/"), Some(548));
    }

    #[test]
    fn extracts_max_size_without_space() {
        assert_eq!(extract_max_size("/**设备名称[max_size:34]**/"), Some(34));
    }

    #[test]
    fn returns_none_without_marker() {
        assert_eq!(extract_max_size("/** 普通注释 **/"), None);
    }

    #[test]
    fn extracts_max_count() {
        assert_eq!(extract_max_count("/** [max_count:8] 条目列表 **/"), Some(8));
        assert_eq!(extract_max_count("// [max_count:16]"), Some(16));
    }

    #[test]
    fn extracts_both_from_one_line() {
        let line = "/** [max_size:64] [max_count:4] 名字列表 **/";
        assert_eq!(extract_max_size(line), Some(64));
        assert_eq!(extract_max_count(line), Some(4));
    }

    #[test]
    fn max_count_is_not_max_size() {
        // 别把 max_count 误认成 max_size
        assert_eq!(extract_max_size("/** [max_count:8] **/"), None);
    }
}
