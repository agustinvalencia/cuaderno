// Tokeniser-parity guard (#357): templateTokens must extract the same
// {{placeholder}} names, in the same first-appearance order, as the
// backend's `placeholder_names` (crates/cdno-core/src/template.rs). The
// unknown-token warning compares the editor's tokens against the
// backend-supplied known set, so any divergence here would make the
// editor disagree with what render actually substitutes. These cases
// exercise the tricky rules the view test never hits: trimming,
// adjacency (slice-advance past `}}`), an unclosed `{{`, and dedup.
import { expect, test } from "vitest";
import { templateTokens } from "./Templates";

test("trims whitespace inside the braces", () => {
  // Mirrors Rust `name = after_open[..end].trim()`.
  expect(templateTokens("{{ spaced }}")).toEqual(["spaced"]);
});

test("tokenises adjacent placeholders, advancing past each closing }}", () => {
  expect(templateTokens("{{a}}{{b}}")).toEqual(["a", "b"]);
});

test("skips an unclosed {{ rather than tokenising it", () => {
  // Rust advances past the `{{` and finds no closing `}}`, so no token.
  expect(templateTokens("{{unclosed")).toEqual([]);
});

test("dedups by first appearance", () => {
  expect(templateTokens("{{x}} {{x}}")).toEqual(["x"]);
});

test("ignores an empty {{}} token, matching the backend", () => {
  // Rust guards `!name.is_empty()`.
  expect(templateTokens("{{}}")).toEqual([]);
});

test("returns names in first-appearance order across a mixed body", () => {
  expect(templateTokens("# {{title}}\nctx: {{context}}\nagain {{title}}")).toEqual([
    "title",
    "context",
  ]);
});
