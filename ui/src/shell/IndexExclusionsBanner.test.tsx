import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import type { IndexExclusions } from "../api/bindings/IndexExclusions";
import IndexExclusionsBanner from "./IndexExclusionsBanner";

const SILENT: IndexExclusions = {
  ignored: 1,
  artefacts: 6,
  indexed: 160,
  ignore_looks_over_broad: false,
};

const OVER_BROAD: IndexExclusions = {
  ignored: 54,
  artefacts: 6,
  indexed: 161,
  ignore_looks_over_broad: true,
};

function renderBanner(exclusions: IndexExclusions) {
  mockIPC((cmd) => {
    if (cmd === "get_index_exclusions") return exclusions;
    throw new Error(`unexpected command: ${cmd}`);
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <IndexExclusionsBanner />
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("says nothing when the ignore list is ordinary housekeeping", async () => {
  renderBanner(SILENT);
  // Give the query a turn to settle; the banner must still render nothing
  // rather than an empty frame.
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(screen.queryByRole("status")).toBeNull();
});

test("reports the count and the total when a glob looks over-broad", async () => {
  renderBanner(OVER_BROAD);
  const notice = await screen.findByRole("status");
  // 54 excluded out of 54 + 161 + 6 walked.
  expect(notice.textContent).toContain("54 of 221 notes");
});

test("explains that excluded notes vanish from search rather than being lost", async () => {
  renderBanner(OVER_BROAD);
  const notice = await screen.findByRole("status");
  expect(notice.textContent).toContain("search, lint and backlinks");
  expect(notice.textContent).toContain("untouched on disk");
});

test("can be dismissed", async () => {
  renderBanner(OVER_BROAD);
  await screen.findByRole("status");
  fireEvent.click(screen.getByRole("button", { name: /dismiss/i }));
  expect(screen.queryByRole("status")).toBeNull();
});

test("a dismissal survives the counts drifting", async () => {
  // In the vault this banner exists for, a glob swallows a whole tree — so
  // every note written into that tree bumps `ignored` by one. A dismissal
  // keyed on the count would pop the banner back up on each filing. It
  // covers the condition, which has not changed.
  const exclusions = { ...OVER_BROAD };
  mockIPC((cmd) => {
    if (cmd === "get_index_exclusions") return exclusions;
    throw new Error(`unexpected command: ${cmd}`);
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const tree = () => (
    <QueryClientProvider client={client}>
      <IndexExclusionsBanner />
    </QueryClientProvider>
  );
  const { rerender } = render(tree());

  await screen.findByRole("status");
  fireEvent.click(screen.getByRole("button", { name: /dismiss/i }));
  expect(screen.queryByRole("status")).toBeNull();

  // A note is filed under the over-broad glob; the count moves, the
  // condition does not.
  exclusions.ignored = 55;
  exclusions.indexed = 161;
  await client.invalidateQueries({ queryKey: ["get_index_exclusions"] });
  rerender(tree());

  expect(screen.queryByRole("status")).toBeNull();
});

test("a dismissal lifts once the condition clears and returns", async () => {
  // Fix the glob and the notice goes away on its own. Introduce a new
  // over-broad one later and it has earned a fresh hearing.
  const exclusions = { ...OVER_BROAD };
  mockIPC((cmd) => {
    if (cmd === "get_index_exclusions") return exclusions;
    throw new Error(`unexpected command: ${cmd}`);
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const tree = () => (
    <QueryClientProvider client={client}>
      <IndexExclusionsBanner />
    </QueryClientProvider>
  );
  const { rerender } = render(tree());

  await screen.findByRole("status");
  fireEvent.click(screen.getByRole("button", { name: /dismiss/i }));

  // The user narrows the glob: the condition clears.
  exclusions.ignore_looks_over_broad = false;
  exclusions.ignored = 1;
  await client.invalidateQueries({ queryKey: ["get_index_exclusions"] });
  rerender(tree());
  expect(screen.queryByRole("status")).toBeNull();

  // Later, a different over-broad glob appears.
  exclusions.ignore_looks_over_broad = true;
  exclusions.ignored = 80;
  await client.invalidateQueries({ queryKey: ["get_index_exclusions"] });
  rerender(tree());

  const notice = await screen.findByRole("status");
  expect(notice.textContent).toContain("80 of");
});
