//! cdno-core: Core library for Cuaderno.
//!
//! File I/O, markdown parsing, SQLite indexing, and file watching.
//! No domain knowledge — reusable in any markdown vault tool.

pub mod config;
pub mod error;
pub mod file_meta;
pub mod frontmatter;
pub mod hash;
pub mod index;
pub mod markdown;
pub mod path;
pub mod reconcile;
pub mod store;
pub mod template;
pub mod transaction;
