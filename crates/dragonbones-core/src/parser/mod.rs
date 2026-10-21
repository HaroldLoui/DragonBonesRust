//! 解析器层，对齐 `parser/*.ts`。

pub mod binary_data_parser;
pub mod data_parser;
pub mod js_compat;
pub mod object_data_parser;

pub use object_data_parser::ObjectDataParser;
