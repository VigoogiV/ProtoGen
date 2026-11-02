//! `.options` 文件生成与校验。
//!
//! 复刻旧工具 `btnCreateOptionsFile_Click` / `checkBytesParaSizeDefine` 的行为：
//! - 只处理 `bytes` / `string` 字段
//! - 从注释里提取 `[max_size:N]`，本行注释优先，其次上方注释
//! - 输出格式：`Msg.field` + 空格补齐 + `\t\t\t` + `max_size:N`，ASCII + CRLF

use crate::model::{FieldDef, ProtoFile};

/// 从一段注释里提取 `[max_size:N]`
pub fn extract_max_size(text: &str) -> Option<u32> {
    let idx = text.find("[max_size:")?;
    let rest = &text[idx + "[max_size:".len()..];
    let end = rest.find(']')?;
    rest[..end].trim().parse::<u32>().ok()
}

/// 一个 bytes/string 字段及其 max_size
#[derive(Debug, Clone)]
pub struct MaxSizeEntry {
    pub key: String,
    pub max_size: Option<u32>,
}

fn is_bytes_or_string(f: &FieldDef) -> bool {
    f.pbtype == "BYTES" || f.pbtype == "STRING" || f.pbtype == "FIXED_LENGTH_BYTES"
}

/// 收集所有 bytes/string 字段及其在注释里声明的 max_size
pub fn collect_max_sizes(file: &ProtoFile) -> Vec<MaxSizeEntry> {
    let mut out = Vec::new();
    for m in &file.messages {
        for f in &m.fields {
            if !is_bytes_or_string(f) {
                continue;
            }
            let mut ms = None;
            if let Some(t) = &f.comment.trailing {
                ms = extract_max_size(t);
            }
            if ms.is_none() {
                // 上方注释：从紧邻的往下找，最后一个带 max_size 的优先
                for l in f.comment.leading.iter().rev() {
                    if let Some(v) = extract_max_size(l) {
                        ms = Some(v);
                        break;
                    }
                }
            }
            out.push(MaxSizeEntry {
                // .options 的键用点分 proto 全名，嵌套层级用 `.` 分隔
                key: format!("{}.{}", m.dotted_name, f.name),
                max_size: ms,
            });
        }
    }
    out
}

/// 生成 `.options` 模板内容（LF 换行，调用方按需转 CRLF）
pub fn generate_options(file: &ProtoFile) -> String {
    let entries = collect_max_sizes(file);
    if entries.is_empty() {
        return String::new();
    }

    let max_len = entries.iter().map(|e| e.key.len()).max().unwrap_or(0);

    let mut out = String::new();
    for e in &entries {
        let pad = max_len.saturating_sub(e.key.len());
        let value = match e.max_size {
            Some(v) => format!("max_size:{}", v),
            None => "max_size:".to_string(),
        };
        out.push_str(&e.key);
        out.push_str(&" ".repeat(pad));
        out.push_str("\t\t\t");
        out.push_str(&value);
        out.push('\n');
    }
    out
}

/// 校验结果
#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    /// proto 里是 bytes/string，但 .options 里没有定义 max_size
    pub missing: Vec<String>,
    /// .options 里定义了，但 proto 里找不到对应字段
    pub extra: Vec<String>,
    /// .options 里写了 `max_size:` 但没填值
    pub blank_value: Vec<String>,
}

impl CheckReport {
    pub fn is_ok(&self) -> bool {
        self.missing.is_empty() && self.blank_value.is_empty()
    }
}

/// 校验 `.options` 是否覆盖了所有 bytes/string 字段
pub fn check(file: &ProtoFile, options_map: &crate::parse::OptionsMap) -> CheckReport {
    let mut report = CheckReport::default();
    let entries = collect_max_sizes(file);

    for e in &entries {
        match options_map.get(&e.key) {
            None => report.missing.push(e.key.clone()),
            Some(opt) => {
                if opt.max_size.is_none() {
                    report.blank_value.push(e.key.clone());
                }
            }
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
}
