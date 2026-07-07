# Run the full CI check locally
ci: fmt clippy test

# Format
fmt:
    cargo fmt --all

# Lint
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Test
test:
    cargo test --workspace

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check

# Build all crates
build:
    cargo build --workspace

# Type-check all crates without producing binaries
check:
    cargo check --workspace --all-targets

# Install the `cdno` binary into ~/.cargo/bin
install:
    cargo install --path crates/cdno-cli --locked

# Remove the installed `cdno` binary
uninstall:
    cargo uninstall cdno-cli

# Remove build artefacts
clean:
    cargo clean

# Coverage report
coverage:
    cargo tarpaulin --workspace --out html
    @echo "Report: tarpaulin-report.html"

# Regenerate the TypeScript bindings under ui/src/api/bindings/ from
# the Rust wire types (ts-rs, plan §3.5). Run after changing any type
# a Tauri command returns.
#
# Two passes: the cdno-tauri pass emits the tauri-owned types plus
# every domain type reachable from a command return (OrientationView
# and friends pull their dependencies transitively). The cdno-domain
# pass covers domain-owned exported types a command returns *directly*
# in a container ts-rs can't follow from tauri — e.g. `Vec<InboxItem>`,
# whose element type would otherwise never be generated.
gen-bindings:
    TS_RS_EXPORT_DIR="{{justfile_directory()}}/ui/src/api/bindings" \
        cargo test -p cdno-tauri --features ts-bindings export_bindings
    TS_RS_EXPORT_DIR="{{justfile_directory()}}/ui/src/api/bindings" \
        cargo test -p cdno-domain --features ts-bindings export_bindings

# Run the desktop app in dev mode against the vault at CUADERNO_VAULT_PATH
app-dev:
    cd ui && bunx tauri dev
