//! dat's front end: a lossless lexer, a recursive-descent parser and the arena AST.

#![forbid(unsafe_code)]

pub mod ast;
pub mod lexer;
mod parser;

pub use parser::{E_SYNTAX, Parsed, parse};
