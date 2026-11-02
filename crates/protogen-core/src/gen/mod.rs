//! 自研 nanopb C 代码生成器。
//!
//! 输出格式严格对照 nanopb 0.3.9.1 `nanopb_generator.py`，
//! 目的是**替代** `protoc.exe` + `protoc-gen-nanopb.exe`，做到零外部依赖。

pub mod comments_inject;
pub mod render;

pub use render::{generate, GenOptions, GenOutput};
