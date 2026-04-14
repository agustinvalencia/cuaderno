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

# Coverage report
coverage:
    cargo tarpaulin --workspace --out html
    @echo "Report: tarpaulin-report.html"
