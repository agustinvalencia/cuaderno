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
