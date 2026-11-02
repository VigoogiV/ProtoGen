//! 用 `protox` 解析 proto，构造 IR。
//!
//! 类型映射、allocation 判定等规则严格对照 nanopb 0.3.9.1
//! `nanopb_generator.py` 的 `Field.__init__`。
//!
//! 行号来自 `FileDescriptorProto.source_code_info` 的 `Location.span`，
//! 路径索引规则见 <https://protobuf.dev/programming-guides/encoding/#source-code-info>。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use prost_types::field_descriptor_proto::{Label, Type};
use prost_types::{DescriptorProto, EnumDescriptorProto, FileDescriptorProto, SourceCodeInfo};

use crate::comments::{self, ScanResult};
use crate::model::*;

#[derive(Debug)]
pub enum ParseError {
    Io(String),
    Compile(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Io(s) => write!(f, "读取文件失败: {}", s),
            ParseError::Compile(s) => write!(f, "proto 编译失败: {}", s),
        }
    }
}

impl std::error::Error for ParseError {}

/// 单个字段的 nanopb 选项
#[derive(Debug, Clone, Default)]
pub struct FieldOptions {
    pub max_size: Option<u32>,
    pub max_count: Option<u32>,
    pub fixed_length: bool,
    pub fixed_count: bool,
    /// 强制分配方式：`static` / `callback` / `pointer`
    pub alloc: Option<String>,
}

/// `.options` 文件内容：`Msg.field` → 选项
pub type OptionsMap = HashMap<String, FieldOptions>;

/// 解析 .options 文件。格式：`Message.field <空白> max_size:34 [max_count:5]`
pub fn read_options_file(path: &Path) -> Result<OptionsMap, ParseError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ParseError::Io(format!("{}: {}", path.display(), e)))?;
    let mut map = OptionsMap::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else { continue };
        let mut opt = FieldOptions::default();
        for token in parts {
            let (name, val) = match token.split_once(':') {
                Some((n, v)) => (n, Some(v)),
                None => (token, None),
            };
            match name {
                "max_size" => opt.max_size = val.and_then(|v| v.parse().ok()),
                "max_count" => opt.max_count = val.and_then(|v| v.parse().ok()),
                "fixed_length" => opt.fixed_length = val.map(|v| v == "true").unwrap_or(true),
                "fixed_count" => opt.fixed_count = val.map(|v| v == "true").unwrap_or(true),
                "type" => opt.alloc = val.map(|v| v.to_string()),
                _ => {}
            }
        }
        map.insert(key.to_string(), opt);
    }

    Ok(map)
}

/// `SourceCodeInfo` 的路径 → 起始行号
struct LineMap {
    map: HashMap<Vec<i32>, u32>,
}

impl LineMap {
    fn new(sci: Option<&SourceCodeInfo>) -> Self {
        let mut map = HashMap::new();
        if let Some(sci) = sci {
            for loc in &sci.location {
                if let Some(&line) = loc.span.first() {
                    map.insert(loc.path.clone(), line as u32);
                }
            }
        }
        LineMap { map }
    }

    fn line(&self, path: &[i32]) -> Option<u32> {
        self.map.get(path).copied()
    }
}

/// 解析入口：编译 proto、扫描注释、构造 IR。
pub fn parse_proto(
    proto_path: &Path,
    include_dirs: &[PathBuf],
    options_map: &OptionsMap,
) -> Result<ProtoFile, ParseError> {
    let mut includes: Vec<PathBuf> = include_dirs.to_vec();
    if let Some(parent) = proto_path.parent() {
        let parent = parent.to_path_buf();
        if !includes.contains(&parent) {
            includes.push(parent);
        }
    }

    let fds =
        protox::compile([proto_path], &includes).map_err(|e| ParseError::Compile(e.to_string()))?;

    let target_name = proto_path
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or_default();

    let fdesc = fds
        .file
        .iter()
        .find(|f| f.name().ends_with(target_name))
        .or_else(|| fds.file.last())
        .ok_or_else(|| ParseError::Compile("descriptor 为空".into()))?;

    let src = std::fs::read_to_string(proto_path)
        .map_err(|e| ParseError::Io(format!("{}: {}", proto_path.display(), e)))?;
    let scan = comments::scan(&src);

    build_file(fdesc, proto_path, &scan, options_map)
}

fn build_file(
    fdesc: &FileDescriptorProto,
    proto_path: &Path,
    scan: &ScanResult,
    options_map: &OptionsMap,
) -> Result<ProtoFile, ParseError> {
    let file_name = proto_path
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("unknown.proto")
        .to_string();
    let base_name = file_name
        .strip_suffix(".proto")
        .unwrap_or(&file_name)
        .to_string();

    let base_names = match &fdesc.package {
        Some(p) if !p.is_empty() => Names::from_dotted(p),
        _ => Names::new(),
    };

    let lines = LineMap::new(fdesc.source_code_info.as_ref());
    let is_proto3 = fdesc.syntax.as_deref() == Some("proto3");

    let mut enums = Vec::new();
    for (ei, e) in fdesc.enum_type.iter().enumerate() {
        enums.push(build_enum(e, &base_names, &lines, &[5, ei as i32], scan));
    }

    // .options 里的键用的是点分 proto 全名（含 package），
    // 和 C 标识符的下划线风格不同，需要单独维护
    let dotted_base = fdesc.package.clone().unwrap_or_default();

    let mut messages = Vec::new();
    for (mi, m) in fdesc.message_type.iter().enumerate() {
        collect_messages(
            m,
            &base_names,
            &dotted_base,
            &lines,
            scan,
            options_map,
            &[4, mi as i32],
            is_proto3,
            &mut messages,
            &mut enums,
        );
    }

    let dependencies = fdesc
        .dependency
        .iter()
        .map(|d| d.trim_end_matches(".proto").to_string())
        .collect();

    // 枚举没有负值时，引用它的字段从 ENUM 改成 UENUM（nanopb 的 add_dependency 逻辑）
    let enum_has_negative: HashMap<String, bool> = enums
        .iter()
        .map(|e| {
            (
                e.names.as_str(),
                e.values.iter().any(|v| v.number < 0),
            )
        })
        .collect();
    for m in &mut messages {
        for f in &mut m.fields {
            if f.pbtype == "ENUM" {
                if let Some(&has_neg) = enum_has_negative.get(&f.ctype) {
                    if !has_neg {
                        f.pbtype = "UENUM".to_string();
                    }
                }
            }
        }
    }

    Ok(ProtoFile {
        base_name,
        file_name,
        package: fdesc.package.clone().filter(|p| !p.is_empty()),
        syntax: fdesc.syntax.clone().unwrap_or_else(|| "proto2".into()),
        enums,
        messages,
        dependencies,
    })
}

/// 递归收集 message（对应 nanopb 的 `iterate_messages`）
#[allow(clippy::too_many_arguments)]
fn collect_messages(
    desc: &DescriptorProto,
    parent: &Names,
    dotted_parent: &str,
    lines: &LineMap,
    scan: &ScanResult,
    options_map: &OptionsMap,
    path: &[i32],
    is_proto3: bool,
    out_messages: &mut Vec<MessageDef>,
    out_enums: &mut Vec<EnumDef>,
) {
    let names = parent.join(desc.name());
    let dotted = if dotted_parent.is_empty() {
        desc.name().to_string()
    } else {
        format!("{}.{}", dotted_parent, desc.name())
    };
    let (comment, line) = comment_of(scan, lines.line(path));

    let mut fields = Vec::new();
    for (fi, f) in desc.field.iter().enumerate() {
        let mut fpath = path.to_vec();
        fpath.extend_from_slice(&[2, fi as i32]);
        if let Some(field) = build_field(
            f,
            &names,
            &dotted,
            lines.line(&fpath),
            scan,
            options_map,
            is_proto3,
        ) {
            fields.push(field);
        }
    }

    out_messages.push(MessageDef {
        names: names.clone(),
        dotted_name: dotted.clone(),
        fields,
        comment,
        line,
        msgid: None,
    });

    // 嵌套 enum 先于嵌套 message，顺序与 nanopb 的 parse() 一致
    for (ei, e) in desc.enum_type.iter().enumerate() {
        let mut epath = path.to_vec();
        epath.extend_from_slice(&[4, ei as i32]);
        out_enums.push(build_enum(e, &names, lines, &epath, scan));
    }
    for (ni, nested) in desc.nested_type.iter().enumerate() {
        let mut npath = path.to_vec();
        npath.extend_from_slice(&[3, ni as i32]);
        collect_messages(
            nested,
            &names,
            &dotted,
            lines,
            scan,
            options_map,
            &npath,
            is_proto3,
            out_messages,
            out_enums,
        );
    }
}

fn build_enum(
    desc: &EnumDescriptorProto,
    parent: &Names,
    lines: &LineMap,
    path: &[i32],
    scan: &ScanResult,
) -> EnumDef {
    let names = parent.join(desc.name());
    let (comment, line) = comment_of(scan, lines.line(path));

    // nanopb 的 long_names 默认 true：枚举值名 = 枚举全名 + "_" + 原值名
    let values = desc
        .value
        .iter()
        .enumerate()
        .map(|(vi, v)| {
            let mut vpath = path.to_vec();
            vpath.extend_from_slice(&[2, vi as i32]);
            let cname = format!("{}_{}", names.as_str(), v.name());
            let (c, l) = comment_of(scan, lines.line(&vpath));
            EnumValue {
                name: cname,
                number: v.number(),
                comment: c,
                line: l,
            }
        })
        .collect();

    EnumDef {
        names,
        values,
        comment,
        line,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_field(
    desc: &prost_types::FieldDescriptorProto,
    struct_names: &Names,
    dotted_scope: &str,
    line: Option<u32>,
    scan: &ScanResult,
    options_map: &OptionsMap,
    is_proto3: bool,
) -> Option<FieldDef> {
    let name = desc.name().to_string();
    // .options 的键用点分 proto 全名
    let key = format!("{}.{}", dotted_scope, name);
    let opt = options_map.get(&key).cloned().unwrap_or_default();

    let ty = desc.r#type();
    let label = desc.label();
    let max_size = opt.max_size;
    let max_count = opt.max_count;

    // 规则判定，对照 nanopb Field.__init__
    let rules = if label == Label::Repeated {
        Rules::Repeated
    } else if is_proto3 {
        Rules::Singular
    } else if label == Label::Required {
        Rules::Required
    } else {
        Rules::Optional
    };

    let mut array_decl = String::new();
    let fixed_count = opt.fixed_count;
    if rules == Rules::Repeated {
        if let Some(mc) = max_count {
            array_decl = format!("[{}]", mc);
        }
    }

    // 能否静态分配
    let mut can_be_static = true;
    if rules == Rules::Repeated && max_count.is_none() {
        can_be_static = false;
    }
    if ty == Type::String && max_size.is_none() {
        can_be_static = false;
    }
    if ty == Type::Bytes && max_size.is_none() {
        can_be_static = false;
    }
    if opt.alloc.as_deref() == Some("callback") || opt.alloc.as_deref() == Some("pointer") {
        can_be_static = false;
    }

    let allocation = if can_be_static {
        Allocation::Static
    } else if opt.alloc.as_deref() == Some("pointer") {
        Allocation::Pointer
    } else {
        Allocation::Callback
    };

    let type_name = desc.type_name();
    let type_name = if type_name.is_empty() {
        None
    } else {
        Some(type_name)
    };

    let (ctype, pbtype, enc_size, submsgname, enumname) = map_type(
        ty,
        type_name,
        struct_names,
        &name,
        max_size,
        allocation,
        opt.fixed_length,
    );

    if allocation == Allocation::Static {
        match pbtype.as_str() {
            "STRING" => {
                if let Some(ms) = max_size {
                    array_decl = format!("[{}]", ms);
                }
            }
            "FIXED_LENGTH_BYTES" => {
                if let Some(ms) = max_size {
                    array_decl = format!("[{}]", ms);
                }
            }
            _ => {}
        }
    }

    let (comment, l) = comment_of(scan, line);

    // nanopb：枚举字段的默认值要加枚举类型前缀（`GREEN` → `edge_test_Color_GREEN`）
    let default = match (desc.default_value.clone(), ty) {
        (Some(d), Type::Enum) => Some(format!("{}_{}", ctype, d)),
        (d, _) => d,
    };

    Some(FieldDef {
        name,
        tag: desc.number(),
        pbtype,
        ctype,
        array_decl,
        rules,
        allocation,
        max_size,
        max_count,
        fixed_count,
        default,
        submsgname,
        enumname,
        enc_size,
        comment,
        line: l,
    })
}

#[allow(clippy::too_many_arguments)]
fn map_type(
    ty: Type,
    type_name: Option<&str>,
    struct_names: &Names,
    field_name: &str,
    max_size: Option<u32>,
    allocation: Allocation,
    fixed_length: bool,
) -> (
    String,
    String,
    Option<u32>,
    Option<String>,
    Option<String>,
) {
    // 基础类型表，对照 nanopb_generator.py 的 `datatypes`
    let base = |c: &str, pb: &str, es: u32| (c.to_string(), pb.to_string(), Some(es), None, None);

    match ty {
        Type::Bool => base("bool", "BOOL", 1),
        Type::Double => base("double", "DOUBLE", 8),
        Type::Fixed32 => base("uint32_t", "FIXED32", 4),
        Type::Fixed64 => base("uint64_t", "FIXED64", 8),
        Type::Float => base("float", "FLOAT", 4),
        Type::Int32 => base("int32_t", "INT32", 10),
        Type::Int64 => base("int64_t", "INT64", 10),
        Type::Sfixed32 => base("int32_t", "SFIXED32", 4),
        Type::Sfixed64 => base("int64_t", "SFIXED64", 8),
        Type::Sint32 => base("int32_t", "SINT32", 5),
        Type::Sint64 => base("int64_t", "SINT64", 10),
        Type::Uint32 => base("uint32_t", "UINT32", 5),
        Type::Uint64 => base("uint64_t", "UINT64", 10),
        Type::Enum => {
            let n = type_name.map(names_from_type_name).unwrap_or_default();
            let s = n.as_str();
            (s.clone(), "ENUM".to_string(), None, None, Some(s))
        }
        Type::String => {
            let es = max_size.map(|ms| varint_max_size(ms as u64) + ms);
            ("char".to_string(), "STRING".to_string(), es, None, None)
        }
        Type::Bytes => {
            if fixed_length {
                let es = max_size.map(|ms| varint_max_size(ms as u64) + ms);
                (
                    "pb_byte_t".to_string(),
                    "FIXED_LENGTH_BYTES".to_string(),
                    es,
                    None,
                    None,
                )
            } else if allocation == Allocation::Static {
                // nanopb: ctype = struct_name + field_name + 't'
                let cname = struct_names.join(field_name).join("t").as_str();
                let es = max_size.map(|ms| varint_max_size(ms as u64) + ms);
                (cname, "BYTES".to_string(), es, None, None)
            } else {
                (
                    "pb_bytes_array_t".to_string(),
                    "BYTES".to_string(),
                    None,
                    None,
                    None,
                )
            }
        }
        Type::Message | Type::Group => {
            let n = type_name.map(names_from_type_name).unwrap_or_default();
            let s = n.as_str();
            (s.clone(), "MESSAGE".to_string(), None, Some(s), None)
        }
    }
}

pub fn names_from_type_name(type_name: &str) -> Names {
    let t = type_name.strip_prefix('.').unwrap_or(type_name);
    Names::from_dotted(t)
}

/// 对应 nanopb 的 `varint_max_size`
pub fn varint_max_size(max_value: u64) -> u32 {
    for i in 1..=10u32 {
        if (max_value >> (i * 7)) == 0 {
            return i;
        }
    }
    10
}

fn comment_of(scan: &ScanResult, line: Option<u32>) -> (Comment, u32) {
    let Some(line) = line else {
        return (Comment::default(), 0);
    };
    let Some(idx) = scan.index_of(line) else {
        return (Comment::default(), line);
    };
    let leading = scan.leading_comments(idx);
    let trailing = scan.trailing_comment(idx);
    (
        Comment {
            leading,
            trailing,
            detached: Vec::new(),
        },
        line,
    )
}
