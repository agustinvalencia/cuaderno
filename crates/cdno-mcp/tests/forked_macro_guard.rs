//! A tripwire on the one dependency this crate has forked code from.
//!
//! `CuadernoServer::call_tool` (`src/server.rs`) is a verbatim copy of what
//! `rmcp-macros` generates, plus `.or_else(decode)`. The workspace declares
//! `rmcp = "1.7"` — a caret range — so a later 1.x could add a step to the
//! generated body that this copy silently loses: dispatch still works, so
//! every test here stays green while whatever the new step did is simply
//! gone. That is undetectable from the outside, which is the whole problem.
//!
//! `Cargo.lock` is committed, so rmcp cannot move on a build, a fresh clone
//! or in CI — only on a deliberate `cargo update`. Pinning `=1.7` was
//! considered and rejected: it would charge a manual bump on every patch
//! release, including a security patch, to guard against a deliberate act by
//! somebody already reviewing dependency movement. What was missing was not
//! a pin but a signal *at the moment of the upgrade*, which is this test.
//!
//! A patch release (1.7.1) passes silently. A minor bump fails here, naming
//! what to re-read. Raised from the first review of #560.

/// Read from the committed lockfile rather than a build script: the lockfile
/// is the version that actually gets compiled, and `include_str!` resolves it
/// at compile time so the test does not depend on the working directory.
const LOCKFILE: &str = include_str!("../../../Cargo.lock");

/// The rmcp minor series `src/server.rs`'s `call_tool` was forked from.
const FORKED_FROM: &str = "1.7.";

#[test]
fn rmcp_has_not_moved_off_the_forked_minor_series() {
    let version = resolved_version("rmcp").expect("rmcp is a dependency, so it is in Cargo.lock");

    assert!(
        version.starts_with(FORKED_FROM),
        "rmcp moved to {version}, off the {FORKED_FROM}x series that \
         `CuadernoServer::call_tool` was forked from.\n\n\
         That override is a hand-copy of the `#[tool_handler]` body plus \
         `.or_else(decode)`. If the macro's generated `call_tool` gained a \
         step in {version} — output-schema validation, `_meta` propagation, \
         progress plumbing — this crate has silently lost it, and nothing \
         else will fail.\n\n\
         Re-diff `CuadernoServer::call_tool` in crates/cdno-mcp/src/server.rs \
         against `rmcp-macros-{version}/src/tool_handler.rs`, port anything \
         new, then update FORKED_FROM in this file and the version named in \
         that function's doc comment."
    );
}

/// Pulls a package's version out of the lockfile's TOML without a toml
/// dependency: find the `name = "<pkg>"` line inside a `[[package]]` block,
/// then take the `version` that follows it.
fn resolved_version(package: &str) -> Option<String> {
    let needle = format!("name = \"{package}\"");
    let rest = LOCKFILE.split_once(&needle)?.1;
    rest.lines()
        .take_while(|l| !l.starts_with("[[package]]"))
        .find_map(|l| l.strip_prefix("version = "))
        .map(|v| v.trim().trim_matches('"').to_owned())
}

/// The parser is doing real work on a file nobody edits by hand, so pin it
/// against a package whose absence would mean the lockfile shape changed.
#[test]
fn the_lockfile_parser_finds_a_known_package() {
    assert!(
        resolved_version("serde_json").is_some(),
        "could not find serde_json in Cargo.lock — the lockfile format \
         changed and `resolved_version` needs updating, which would \
         otherwise make the rmcp guard above silently vacuous"
    );
    assert_eq!(
        resolved_version("definitely-not-a-real-package-xyz"),
        None,
        "the parser must not invent a version for an absent package"
    );
}
