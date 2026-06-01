//! Library facade for the `cdno` binary.
//!
//! Splitting the CLI into a library plus a thin binary lets tests
//! exercise command implementations in-process — `cargo tarpaulin`
//! and other coverage tools instrument library calls but not
//! subprocess execs of the built binary.
//!
//! `main.rs` is the only consumer that lives in the binary
//! crate. Everything else — argument parsing types, command logic,
//! vault bootstrap — sits here.

pub mod bootstrap;
pub mod commands;
pub mod completions;
pub mod prompt;
