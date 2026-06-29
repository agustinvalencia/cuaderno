# Cuaderno documentation site

The user-facing guide at <https://agustinvalencia.github.io/cuaderno>, built with
[mdBook](https://rust-lang.github.io/mdBook/) and deployed automatically by
[`.github/workflows/docs.yml`](../.github/workflows/docs.yml) on every push to `main` that
touches `docs-site/`.

This is the **user guide** (concepts, tutorials, command + MCP reference). It is distinct from
the internal design notes under [`../docs/`](../docs/) (`design.md`, `implementation-plan.md`,
`cli-ergonomics.md`), which target contributors.

## Preview locally

```bash
# Install mdBook once (either works):
cargo install mdbook        # Rust toolchain
brew install mdbook         # Homebrew

# From the repo root:
mdbook serve docs-site      # live preview at http://localhost:3000 (auto-reloads)
mdbook build docs-site      # one-off build into docs-site/book/ (gitignored)
```

`mdbook build` warns on broken intra-book links — keep the build clean.

## Layout

- `book.toml` — site config (title, theme, `site-url = "/cuaderno/"`, search, edit links).
- `src/SUMMARY.md` — the table of contents; **this file defines the sidebar and page order**.
  Every page must be listed here or mdBook won't include it.
- `src/**` — the Markdown pages, grouped: `getting-started/`, `concepts/`, `tutorials/`,
  `reference/cli/`, `reference/mcp/`, and top-level reference pages.

## Adding or editing a page

1. Create/edit the `.md` file under `src/`.
2. If it's a new page, add a line for it in `src/SUMMARY.md` (indentation sets nesting).
3. `mdbook serve docs-site` and check it renders + links resolve.
4. Keep examples to **shipped behaviour** — verify commands against `cdno <cmd> --help`.

## One-time GitHub setup (maintainer)

GitHub Pages must be told to serve from Actions: repository **Settings → Pages → Build and
deployment → Source → "GitHub Actions"**. After that, every push to `main` redeploys the site.
