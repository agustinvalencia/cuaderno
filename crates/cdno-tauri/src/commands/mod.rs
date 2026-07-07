//! One module per view surface; each command is a thin async wrapper
//! (`#[tauri::command]`) over a synchronous `*_impl(&Vault, …)`
//! function. The split is the test seam: `*_impl` runs under plain
//! `cargo test` against the Memory doubles, no Tauri runtime needed.

pub mod actions;
pub mod orientation;
pub mod projects;
