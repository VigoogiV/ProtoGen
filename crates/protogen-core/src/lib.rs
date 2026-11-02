//! protogen-core：proto → nanopb C 代码的纯 Rust 实现。
//!
//! 模块划分：
//! - [`model`]：中间表示
//! - [`comments`]：源码注释扫描（不依赖 descriptor 的注释归属）
//! - [`parse`]：protox → IR
//! - [`gen`]：C 代码渲染 + 注释注入
//! - [`options`]：`.options` 文件生成与校验
//! - [`encode`]：GB2312 / 换行处理

pub mod comments;
pub mod encode;
pub mod gen;
pub mod model;
pub mod options;
pub mod parse;

pub use gen::{generate, GenOptions, GenOutput};
pub use model::ProtoFile;
pub use parse::{parse_proto, read_options_file, FieldOptions, OptionsMap, ParseError};
