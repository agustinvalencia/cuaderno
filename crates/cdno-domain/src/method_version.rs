//! The version of the Research Logbook Method this binary implements
//! (RFC 0006 §7.4, §8.4).
//!
//! The method version tracks the method's text, not cdno's crate version.
//! `cdno init` stamps a new vault with it, and it is what a vault's own
//! `[vault] method_version` is compared against. A binary never stamps a
//! version whose practices it lacks, so this stays at 1.0 until parked maps
//! accept writes and handing off are both in place, and moves to 1.1 only
//! then (RFC 0006 plan, T23; decision D4).

use cdno_core::config::MethodVersion;

/// The Research Logbook Method version this binary implements.
pub const METHOD_VERSION: MethodVersion = MethodVersion { major: 1, minor: 0 };
