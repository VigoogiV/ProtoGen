//! 中间表示（IR）：把 proto 描述符归一化成 nanopb 生成器需要的数据结构。
//!
//! 设计原则：尽量贴近 nanopb 0.3.9.1 `nanopb_generator.py` 的模型，
//! 让生成逻辑可以逐行对照源码，便于验证输出一致性。

/// 嵌套名字，对应 nanopb 的 `Names` 类：各部分用 `_` 连接。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Names(pub Vec<String>);

impl Names {
    pub fn new() -> Self {
        Names(Vec::new())
    }

    pub fn from_dotted(s: &str) -> Self {
        if s.is_empty() {
            Names(Vec::new())
        } else {
            Names(s.split('.').map(|p| p.to_string()).collect())
        }
    }

    /// 对应 Python 的 `Names + str`
    pub fn join(&self, part: &str) -> Names {
        let mut parts = self.0.clone();
        parts.push(part.to_string());
        Names(parts)
    }

    /// 对应 Python 的 `Names + Names`
    pub fn extend(&self, other: &Names) -> Names {
        let mut parts = self.0.clone();
        parts.extend(other.0.iter().cloned());
        Names(parts)
    }

    pub fn as_str(&self) -> String {
        self.0.join("_")
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for Names {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 一段注释。protoc 的 descriptor 会剥离 `/**` `**`/`*/` 标记，
/// 这里保留原始内容，渲染时再按需补回。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comment {
    /// 上方注释（独占行），多行时按换行拼接
    pub leading: Vec<String>,
    /// 行尾注释
    pub trailing: Option<String>,
    /// 与代码分离的注释块（`leading_detached_comments`）
    pub detached: Vec<String>,
}

impl Comment {
    pub fn is_empty(&self) -> bool {
        self.leading.iter().all(|s| s.trim().is_empty())
            && self
                .trailing
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
    }

    /// 取第一个非空的上方注释行（用于 max_size 提取等）
    pub fn leading_text(&self) -> String {
        self.leading.join(" ")
    }

    pub fn trailing_text(&self) -> String {
        self.trailing.clone().unwrap_or_default()
    }
}

/// 枚举值
#[derive(Debug, Clone)]
pub struct EnumValue {
    /// C 语言里的名字（已按 long_names 规则拼好前缀）
    pub name: String,
    pub number: i32,
    pub comment: Comment,
    /// 源码行号（0 基）
    pub line: u32,
}

/// 枚举定义
#[derive(Debug, Clone)]
pub struct EnumDef {
    /// 枚举自身名字（含外层作用域）
    pub names: Names,
    pub values: Vec<EnumValue>,
    pub comment: Comment,
    pub line: u32,
}

impl EnumDef {
    /// 对应 `Enum.__str__` 里的 MIN/MAX 取值
    pub fn min_value(&self) -> Option<&EnumValue> {
        self.values.first()
    }

    pub fn max_value(&self) -> Option<&EnumValue> {
        self.values.last()
    }
}

/// 字段的分配方式，对应 nanopb 的 `allocation`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allocation {
    Static,
    Callback,
    Pointer,
}

impl Allocation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Allocation::Static => "STATIC",
            Allocation::Callback => "CALLBACK",
            Allocation::Pointer => "POINTER",
        }
    }
}

/// 字段规则，对应 nanopb 的 `rules`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rules {
    Singular,
    Required,
    Optional,
    Repeated,
}

impl Rules {
    pub fn as_str(&self) -> &'static str {
        match self {
            Rules::Singular => "SINGULAR",
            Rules::Required => "REQUIRED",
            Rules::Optional => "OPTIONAL",
            Rules::Repeated => "REPEATED",
        }
    }
}

/// 字段定义
#[derive(Debug, Clone)]
pub struct FieldDef {
    /// C 结构体成员名
    pub name: String,
    pub tag: i32,
    /// nanopb 的 pbtype，如 `UINT32` / `BYTES` / `MESSAGE`
    pub pbtype: String,
    /// C 类型，如 `uint32_t` / `char` / `BF_PB_CH_BRIEF_T_wcnChName_t`
    pub ctype: String,
    /// 数组声明后缀，如 `[34]`
    pub array_decl: String,
    pub rules: Rules,
    pub allocation: Allocation,
    pub max_size: Option<u32>,
    pub max_count: Option<u32>,
    pub fixed_count: bool,
    pub default: Option<String>,
    /// 引用子消息时的 C 名字（仅 MESSAGE 类型）
    pub submsgname: Option<String>,
    /// 引用枚举时的 C 名字（仅 ENUM 类型）
    pub enumname: Option<String>,
    /// 单个字段的编码长度（不含 tag），对应 nanopb 的 `enc_size`
    pub enc_size: Option<u32>,
    pub comment: Comment,
    pub line: u32,
}

/// 消息定义
#[derive(Debug, Clone)]
pub struct MessageDef {
    pub names: Names,
    /// 点分 proto 全名（含 package 和嵌套层级）。
    /// `.options` 的键用的是这个，不是下划线风格的 C 名。
    pub dotted_name: String,
    pub fields: Vec<FieldDef>,
    pub comment: Comment,
    pub line: u32,
    pub msgid: Option<i32>,
}

impl MessageDef {
    pub fn name(&self) -> String {
        self.names.as_str()
    }

    /// 按 tag 升序排列的字段（对应 `ordered_fields`）
    pub fn ordered_fields(&self) -> Vec<&FieldDef> {
        let mut v: Vec<&FieldDef> = self.fields.iter().collect();
        v.sort_by_key(|f| f.tag);
        v
    }
}

/// 整个 proto 文件
#[derive(Debug, Clone)]
pub struct ProtoFile {
    /// 文件名（不含路径、不含扩展名），如 `bf_ibcp`
    pub base_name: String,
    /// 原始文件名，如 `bf_ibcp.proto`
    pub file_name: String,
    pub package: Option<String>,
    pub syntax: String,
    pub enums: Vec<EnumDef>,
    pub messages: Vec<MessageDef>,
    /// import 的其他 proto（去掉扩展名）
    pub dependencies: Vec<String>,
}
