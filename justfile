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
