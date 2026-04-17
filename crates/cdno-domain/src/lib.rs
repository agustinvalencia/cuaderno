//! cdno-domain: Domain logic for Cuaderno.
//!
//! Note types, business rules, queries, and state transitions.
//! Pure logic — no file I/O, no networking. Receives dependencies via constructor injection.

pub mod error;
pub mod note_type;
