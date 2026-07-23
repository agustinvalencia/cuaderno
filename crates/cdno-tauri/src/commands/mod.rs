//! One module per view surface; each command is a thin async wrapper
//! (`#[tauri::command]`) over a synchronous `*_impl(&Vault, …)`
//! function. The split is the test seam: `*_impl` runs under plain
//! `cargo test` against the Memory doubles, no Tauri runtime needed.

pub mod actions;
pub mod calendar;
pub mod capture;
pub mod commitments;
pub mod config;
pub mod custom_css;
pub mod notes;
pub mod orientation;
pub mod portfolios;
pub mod projects;
pub mod questions;
pub mod search;
pub mod stewardships;
pub mod strategic;
pub mod templates;
pub mod weekly;
