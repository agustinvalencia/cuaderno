// Disambiguation resolver + picker (#338): an ambiguous error opens a
// picker of the candidates; choosing one re-invokes the captured retry
// with the chosen string; an ambiguous *slug* (no candidates) is not a
// pickable ambiguity and falls through; the open picker is axe-clean.
import { afterEach, expect, test, vi } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { CuadernoError } from "../../api/commands";
import type { CmdError } from "../../api/bindings/CmdError";
import AmbiguityPicker from "./AmbiguityPicker";
import { useAmbiguityResolver } from "./useAmbiguityResolver";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

function ambiguousError(query: string, candidates: string[]): CuadernoError {
  const payload: CmdError = { kind: "ambiguous", data: { query, candidates } };
  return new CuadernoError(payload);
}

/** A minimal host: a button that feeds a canned error into the resolver,
 * plus the picker. `retry` stands in for the re-invoked command;
 * `onHandled` reports the `handle` return so a test can assert the
 * branch. */
function Harness({
  error,
  retry,
  onHandled,
}: {
  error: CuadernoError;
  retry: (choice: string) => Promise<unknown>;
  onHandled?: (handled: boolean) => void;
}) {
  const ambiguity = useAmbiguityResolver();
  return (
    <>
      <button
        type="button"
        onClick={() => {
          // Call handle unconditionally, then report — an optional-call
          // (`onHandled?.(handle(...))`) would short-circuit the argument
          // when no reporter is passed and never invoke handle.
          const handled = ambiguity.handle(error, retry, "action");
          onHandled?.(handled);
        }}
      >
        trigger
      </button>
      <AmbiguityPicker
        state={ambiguity.state}
        resolving={ambiguity.resolving}
        choose={ambiguity.choose}
        close={ambiguity.close}
      />
    </>
  );
}

afterEach(cleanup);

test("an ambiguous error opens the picker listing the candidates", async () => {
  render(
    <Harness
      error={ambiguousError("rev", ["Review the draft", "Revise the intro"])}
      retry={() => Promise.resolve()}
    />,
  );
  fireEvent.click(screen.getByText("trigger"));

  expect(await screen.findByRole("dialog")).toBeDefined();
  expect(screen.getByText("More than one action matched.")).toBeDefined();
  // The query is echoed and every candidate is offered as a button.
  expect(screen.getByText(/rev/)).toBeDefined();
  expect(screen.getByRole("button", { name: "Review the draft" })).toBeDefined();
  expect(screen.getByRole("button", { name: "Revise the intro" })).toBeDefined();
});

test("choosing a candidate re-invokes the retry with the chosen string", async () => {
  const retry = vi.fn(() => Promise.resolve());
  render(
    <Harness
      error={ambiguousError("rev", ["Review the draft", "Revise the intro"])}
      retry={retry}
    />,
  );
  fireEvent.click(screen.getByText("trigger"));
  fireEvent.click(await screen.findByRole("button", { name: "Revise the intro" }));

  expect(retry).toHaveBeenCalledWith("Revise the intro");
  // The picker closes once the re-invoke settles.
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

test("an ambiguous slug (no candidates) is not pickable — handle returns false", () => {
  const handled: boolean[] = [];
  render(
    <Harness
      error={ambiguousError("health", [])}
      retry={() => Promise.resolve()}
      onHandled={(h) => handled.push(h)}
    />,
  );
  fireEvent.click(screen.getByText("trigger"));

  expect(handled).toEqual([false]);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("the open picker has no axe violations", async () => {
  render(
    <main>
      <Harness
        error={ambiguousError("rev", ["Review the draft", "Revise the intro"])}
        retry={() => Promise.resolve()}
      />
    </main>,
  );
  fireEvent.click(screen.getByText("trigger"));
  await screen.findByRole("dialog");
  // Radix portals the dialog to document.body, so scope axe to the body.
  expect(
    await axe(document.body, { rules: { "color-contrast": { enabled: false } } }),
  ).toHaveNoViolations();
});
