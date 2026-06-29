# Installation

Cuaderno ships two binaries:

- **`cdno`** — the command-line tool for the daily loop.
- **`cdno-mcp`** — the MCP server that lets Claude (and other MCP clients) read and write your vault.

Both are installed together.

## Homebrew (macOS and Linux)

The recommended path. Pre-built bottles exist for macOS (Apple Silicon + Intel) and Linux (x86_64 +
aarch64).

```bash
brew install agustinvalencia/tap/cuaderno
```

Verify:

```bash
cdno --version      # -> cdno 0.1.24 (or newer)
cdno-mcp --version
```

To upgrade later:

```bash
brew upgrade cuaderno
```

## From source

Use this on platforms without a bottle, or to track the latest `main`. You need a
[Rust toolchain](https://rustup.rs).

```bash
git clone https://github.com/agustinvalencia/cuaderno
cd cuaderno
cargo build --release --bins
# Binaries land at target/release/cdno and target/release/cdno-mcp
```

Put them on your `PATH` — for example:

```bash
ln -s "$PWD/target/release/cdno"     /usr/local/bin/cdno
ln -s "$PWD/target/release/cdno-mcp" /usr/local/bin/cdno-mcp
```

## Shell completions (optional)

`cdno` can print a completion script for your shell, with **vault-aware** suggestions (it completes
project, portfolio, and stewardship slugs by re-invoking the binary on TAB):

```bash
# zsh — add to your ~/.zshrc:
source <(cdno completions zsh)
```

Supported shells: `bash`, `zsh`, `fish`, `elvish`, `powershell`. See
[`completions`](../reference/cli/completions.md) for per-shell setup.

## Uninstall

```bash
brew uninstall cuaderno          # if installed via Homebrew
# or, for a source build, remove the symlinks you created:
rm -f /usr/local/bin/cdno /usr/local/bin/cdno-mcp
```

Uninstalling the binaries never touches your vault — it's just Markdown files on disk. Delete the
vault directory yourself if you want it gone.

## Next step

Create your vault: [Initialise a vault](initialise-a-vault.md).
