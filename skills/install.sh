#!/usr/bin/env bash
# install.sh — Install all agent-skills for Claude Code.
#
# What it does:
#   Creates symlinks for each skill directory into ~/.claude/skills/
#
# Safe to run multiple times (idempotent).

set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
CLAUDE_DIR="$HOME/.claude"
SKILLS_DIR="$CLAUDE_DIR/skills"

# ── Helpers ──────────────────────────────────────────────────────────────────

info()  { echo "  [ok]   $1"; }
skip()  { echo "  [skip] $1"; }
warn()  { echo "  [warn] $1"; }

# Create a symlink if it doesn't already exist.
# Usage: ensure_symlink <target> <link_path>
ensure_symlink() {
    local target="$1" link="$2"
    if [ -L "$link" ]; then
        skip "$link already exists"
    elif [ -e "$link" ]; then
        warn "$link exists but is not a symlink — skipping"
    else
        ln -s "$target" "$link"
        info "$link -> $target"
    fi
}

# ── Symlinks ─────────────────────────────────────────────────────────────────

echo "Installing agent-skills from: $REPO_DIR"
echo ""

mkdir -p "$SKILLS_DIR"

echo "Symlinks:"

count=0
for skill_dir in "$REPO_DIR"/*/; do
    skill_name="$(basename "$skill_dir")"

    # Skip non-skill directories.
    [ -f "$skill_dir/SKILL.md" ] || continue

    ensure_symlink "$skill_dir" "$SKILLS_DIR/$skill_name"
    count=$((count + 1))
done

echo ""
echo "Done. Installed $count skills."
