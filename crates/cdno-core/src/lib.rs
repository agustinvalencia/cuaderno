//! cdno-core: Core library for Cuaderno.
//!
//! File I/O, markdown parsing, SQLite indexing, and file watching.
//! No domain knowledge — reusable in any markdown vault tool.

pub mod adoption;
pub mod artefacts;
pub mod classify;
pub mod config;
pub mod config_edit;
pub mod error;
pub mod extractors;
pub mod file_meta;
pub mod frontmatter;
pub mod frontmatter_edit;
pub mod git;
pub mod hash;
pub mod index;
pub mod markdown;
pub mod note;
pub mod path;
pub mod paths;
pub mod reconcile;
pub mod slug;
pub mod store;
pub mod template;
pub mod transaction;
pub mod typst;
pub mod watcher;
