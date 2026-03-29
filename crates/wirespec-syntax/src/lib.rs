//! wirespec-syntax: Parser and AST for the wirespec protocol description language.
//!
//! This crate provides:
//! - `ast`: All AST node types following AST_SCHEMA_SPEC.md
//! - `lexer`: Hand-written tokenizer
//! - `parser`: Recursive descent parser
//! - `span`: Source location tracking

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod span;

pub use parser::parse;
