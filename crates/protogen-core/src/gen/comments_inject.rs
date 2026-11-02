//! 注释注入与枚举对齐。
//!
//! 复刻旧工具（C# ProtoBufScript）的 `addNotesToCode` + `formatEmum` 两步后处理，
//! 保证输出与旧工具一致：
//!
//! - 上方注释：message/enum 不缩进；字段缩进 4 空格
//! - 行尾注释：结构体成员补空格对齐到该结构体内最长行；枚举交给对齐步骤
//! - 仅上方注释且无行尾注释时，字段后面追加一个空行

use std::collections::HashMap;

use crate::model::*;

/// 注释索引的键
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NoteKey {
    kind: &'static str, // "struct" / "enum"
    scope: String,      // message / enum 名
    item: String,       // 字段名 / 枚举值名；空串表示结构体本身
}

#[derive(Debug, Clone, Default)]
struct Note {
    leading: Vec<String>,
    trailing: Option<String>,
}

fn build_index(file: &ProtoFile) -> HashMap<NoteKey, Note> {
    let mut map = HashMap::new();

    for e in &file.enums {
        map.insert(
            NoteKey {
                kind: "enum",
                scope: e.names.as_str(),
                item: String::new(),
            },
            Note {
                leading: e.comment.leading.clone(),
                trailing: e.comment.trailing.clone(),
            },
        );
        for v in &e.values {
            map.insert(
                NoteKey {
                    kind: "enum",
                    scope: e.names.as_str(),
                    item: v.name.clone(),
                },
                Note {
                    leading: v.comment.leading.clone(),
                    trailing: v.comment.trailing.clone(),
                },
            );
        }
    }

    for m in &file.messages {
        map.insert(
            NoteKey {
                kind: "struct",
                scope: m.name(),
                item: String::new(),
            },
            Note {
                leading: m.comment.leading.clone(),
                trailing: m.comment.trailing.clone(),
            },
        );
        for f in &m.fields {
            map.insert(
                NoteKey {
                    kind: "struct",
                    scope: m.name(),
                    item: f.name.clone(),
                },
                Note {
                    leading: f.comment.leading.clone(),
                    trailing: f.comment.trailing.clone(),
                },
            );
        }
    }

    map
}

/// 按空白和 `;` 分割，对应 C# 的
/// `Split(new char[]{' ',';'}, StringSplitOptions.RemoveEmptyEntries)`
fn split_tokens(line: &str) -> Vec<&str> {
    line.split(|c: char| matches!(c, ' ' | ';' | '\t'))
        .filter(|s| !s.is_empty())
        .collect()
}

/// 注入注释，返回新文本（行分隔仍为 `\n`）
pub fn inject(file: &ProtoFile, text: &str) -> String {
    let index = build_index(file);
    let lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();

    // 第一遍：定位每个 struct / enum 块，算出对齐所需的长度
    let mut struct_max_len: HashMap<usize, usize> = HashMap::new(); // 起始行 → 最长行长度
    let mut i = 0;
    while i < lines.len() {
        let t = &lines[i];
        if t.contains("typedef enum") || t.contains("typedef struct") {
            let mut maxlen = 0usize;
            let mut x = i + 1;
            while x < lines.len() {
                let cur = &lines[x];
                if cur.trim_start().starts_with('}') || cur.trim_start().starts_with("/*") {
                    break;
                }
                maxlen = maxlen.max(cur.chars().count());
                x += 1;
            }
            struct_max_len.insert(i, maxlen);
        }
        i += 1;
    }

    // 第二遍：逐行注入
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 64);
    let mut cur_scope: Option<(&'static str, String, usize)> = None; // (kind, name, block_start)
    let mut i = 0;

    while i < lines.len() {
        let line = &lines[i];

        // 进入 enum
        if line.contains("typedef enum") {
            let toks = split_tokens(line);
            let name = toks
                .get(2)
                .map(|s| s.trim_start_matches('_').to_string())
                .unwrap_or_default();
            cur_scope = Some(("enum", name.clone(), i));

            if let Some(note) = index.get(&NoteKey {
                kind: "enum",
                scope: name,
                item: String::new(),
            }) {
                push_leading(&mut out, note, false);
            }
            out.push(line.clone());
            i += 1;
            continue;
        }

        // 进入 struct
        if line.contains("typedef struct") {
            let toks = split_tokens(line);
            let name = toks
                .get(2)
                .map(|s| s.trim_start_matches('_').to_string())
                .unwrap_or_default();
            cur_scope = Some(("struct", name.clone(), i));

            if let Some(note) = index.get(&NoteKey {
                kind: "struct",
                scope: name,
                item: String::new(),
            }) {
                push_leading(&mut out, note, false);
            }
            out.push(line.clone());
            i += 1;
            continue;
        }

        // 块结束
        if line.trim_start().starts_with('}') {
            cur_scope = None;
            out.push(line.clone());
            i += 1;
            continue;
        }

        if let Some((kind, scope, block_start)) = &cur_scope {
            let toks = split_tokens(line);
            // enum 取第 0 个 token，struct 取第 1 个
            let item = match *kind {
                "enum" => toks.first().map(|s| s.to_string()),
                _ => toks.get(1).map(|s| s.to_string()),
            };

            if let Some(item) = item {
                if let Some(note) = index.get(&NoteKey {
                    kind,
                    scope: scope.clone(),
                    item: item.clone(),
                }) {
                    if !note.leading.is_empty() || note.trailing.is_some() {
                        push_leading(&mut out, note, true);
                        let mut rendered = line.clone();

                        if let Some(trailing) = &note.trailing {
                            if *kind == "struct" {
                                // 结构体成员的行尾注释：补空格对齐
                                let target = struct_max_len.get(block_start).copied().unwrap_or(0);
                                let cur_len = line.chars().count();
                                if target > cur_len {
                                    rendered.push_str(&" ".repeat(target - cur_len));
                                }
                            }
                            rendered.push_str(trailing);
                        }

                        // 仅上方注释、无行尾注释 → 追加空行（仅字段）
                        if !note.leading.is_empty() && note.trailing.is_none() {
                            rendered.push('\n');
                        }

                        out.push(rendered);
                        i += 1;
                        continue;
                    }
                }
            }
        }

        out.push(line.clone());
        i += 1;
    }

    out.join("\n")
}

fn push_leading(out: &mut Vec<String>, note: &Note, indent: bool) {
    let prefix = if indent { "    " } else { "" };
    for l in &note.leading {
        out.push(format!("{}{}", prefix, l.trim()));
    }
}

/// 对应 `formatEmum`：把枚举成员按最长名字对齐
pub fn format_enum(text: &str) -> String {
    let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();

    // 找出每个枚举块的范围
    let mut i = 0;
    while i < lines.len() {
        if lines[i].contains("typedef enum") {
            // 收集成员名，算最长长度
            let mut members: Vec<usize> = Vec::new();
            let mut maxlen = 0usize;
            let mut x = i + 1;
            while x < lines.len() {
                let cur = &lines[x];
                if cur.trim_start().starts_with('}') {
                    break;
                }
                let toks = split_tokens(cur);
                if toks.len() >= 2 && toks[1] == "=" {
                    maxlen = maxlen.max(toks[0].chars().count());
                    members.push(x);
                }
                x += 1;
            }

            // 重排成员行：`    NAME<pad> = <rest>`
            for &mi in &members {
                let raw = &lines[mi];
                if let Some(eq) = raw.find('=') {
                    let toks = split_tokens(raw);
                    let name = toks[0];
                    let tail = &raw[eq..];
                    let pad = maxlen.saturating_sub(name.chars().count());
                    lines[mi] = format!("    {}{} {}", name, " ".repeat(pad), tail);
                }
            }

            i = x;
            continue;
        }
        i += 1;
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_tokens_works() {
        assert_eq!(
            split_tokens("    uint32_t usChId;"),
            vec!["uint32_t", "usChId"]
        );
        assert_eq!(
            split_tokens("    PROTOCOL_A = 0,"),
            vec!["PROTOCOL_A", "=", "0,"]
        );
    }
}
