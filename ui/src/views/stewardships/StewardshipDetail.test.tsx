// Stewardship Detail: charts appear only for an expanded stewardship
// with series; a flat one has no charts pane; recent entries open the
// reader; the log form submits with the template-derived vars.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useParams } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { StewardshipDetail as StewardshipDetailData } from "../../api/bindings/StewardshipDetail";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import StewardshipDetail from "./StewardshipDetail";

const EXPANDED: StewardshipDetailData = {
  slug: "health",
  name: "Health",
  context: "personal",
  variant: "expanded",
  body_markdown: "## Current Status\nConsistent.",
  series: [
    {
      name: "gym · Sets",
      points: [
        { date: "2026-07-01", value: 6 },
        { date: "2026-07-05", value: 4 },
      ],
    },
  ],
  recent: [
    {
      path: "stewardships/health/tracking/2026-07-05-gym.md",
      stewardship: "health",
      activity: "gym",
      date: "2026-07-05",
      duration_min: 55,
      routine: null,
      body_excerpt: "Felt strong",
    },
  ],
  tracking_count: 2,
};

// Two series that exercise the mark heuristic: an all-integer count
// (draws as a column) alongside a fractional measure (keeps the line).
const MIXED: StewardshipDetailData = {
  slug: "health",
  name: "Health",
  context: "personal",
  variant: "expanded",
  body_markdown: "## Current Status\nConsistent.",
  series: [
    {
      name: "gym · Sets",
      points: [
        { date: "2026-07-01", value: 6 },
        { date: "2026-07-05", value: 4 },
      ],
    },
    {
      name: "weigh-in · Weight (kg)",
      points: [
        { date: "2026-07-01", value: 78.4 },
        { date: "2026-07-05", value: 77.9 },
      ],
    },
  ],
  recent: [],
  tracking_count: 4,
};

const FLAT: StewardshipDetailData = {
  slug: "finances",
  name: "Finances",
  context: "household",
  variant: "flat",
  body_markdown: "## Current Status\nSteady.",
  series: [],
  recent: [],
  tracking_count: 0,
};

// The note page opening on `path` is now a navigation to `/note/<path>`;
// this stand-in route surfaces the navigated path so a test can assert a
// click opened the right note.
function NotePathProbe() {
  return <div data-testid="reader-path">{useParams()["*"] ?? ""}</div>;
}

function renderDetail(
  fixture: StewardshipDetailData,
  onCall?: (cmd: string, args: unknown) => void,
) {
  mockIPC((cmd, args) => {
    onCall?.(cmd, args);
    if (cmd === "get_stewardship_detail") return fixture;
    if (cmd === "get_tracking_template_fields")
      return [{ name: "mood", prompt: "How did it feel?" }];
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={[`/stewardships/${fixture.slug}`]}>
          {/* ReaderProvider needs a Router above it (it navigates); the
              `/note/*` stand-in route surfaces the opened path. */}
          <ReaderProvider>
            <Routes>
              <Route path="/stewardships/:slug" element={<StewardshipDetail />} />
              <Route path="/note/*" element={<NotePathProbe />} />
            </Routes>
          </ReaderProvider>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("an expanded stewardship with series shows the charts pane", async () => {
  renderDetail(EXPANDED);
  expect(await screen.findByText("Health")).toBeDefined();
  // The Trends section and its series caption render.
  expect(screen.getByRole("region", { name: "Trends" })).toBeDefined();
  expect(screen.getByText("gym · Sets")).toBeDefined();
});

test("an all-integer series draws as a column and a fractional series keeps the line", async () => {
  renderDetail(MIXED);
  await screen.findByText("Health");
  // The mark shows through as the figure's data-chart-kind — a
  // DOM-level signal that does not depend on Recharts' SVG internals
  // (which do not lay out under jsdom's zero-size container).
  const integerFigure = screen.getByText("gym · Sets").closest("figure");
  const fractionalFigure = screen.getByText("weigh-in · Weight (kg)").closest("figure");
  expect(integerFigure?.getAttribute("data-chart-kind")).toBe("column");
  expect(fractionalFigure?.getAttribute("data-chart-kind")).toBe("line");
});

test("a flat stewardship has no charts pane", async () => {
  renderDetail(FLAT);
  expect(await screen.findByText("Finances")).toBeDefined();
  // No Trends region at all — absent, not an empty frame.
  expect(screen.queryByRole("region", { name: "Trends" })).toBeNull();
  // And the flat variant offers no log form (no tracking/ subdir).
  expect(screen.queryByRole("button", { name: "Log entry" })).toBeNull();
});

test("a recent entry opens the note page at its path", async () => {
  renderDetail(EXPANDED);
  fireEvent.click(await screen.findByText("Felt strong"));
  expect((await screen.findByTestId("reader-path")).textContent).toBe(
    "stewardships/health/tracking/2026-07-05-gym.md",
  );
});

test("the log form fetches template fields and submits with the vars map", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderDetail(EXPANDED, (cmd, args) => calls.push({ cmd, args }));

  fireEvent.click(await screen.findByRole("button", { name: "Log entry" }));
  fireEvent.change(screen.getByLabelText("Activity"), { target: { value: "gym" } });

  // The debounced fetch populates the dynamic "mood" field.
  const mood = await screen.findByLabelText("How did it feel?");
  fireEvent.change(mood, { target: { value: "strong" } });
  fireEvent.change(screen.getByLabelText("Notes"), { target: { value: "Good one." } });

  fireEvent.click(screen.getByRole("button", { name: "Log it" }));
  expect(await screen.findByText(/one more on the record/)).toBeDefined();

  const logged = calls.find((c) => c.cmd === "log_tracking_entry");
  expect(logged?.args).toMatchObject({
    stewardship: "health",
    activity: "gym",
    content: "Good one.",
    vars: { mood: "strong" },
  });
});

test("switching activity clears prior field values and submits only the new activity's vars", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  // Both activities' templates return a field of the SAME name ("mood"),
  // so an un-reset value would silently ride across the switch.
  renderDetail(EXPANDED, (cmd, args) => calls.push({ cmd, args }));

  fireEvent.click(await screen.findByRole("button", { name: "Log entry" }));

  // Activity A: fill the "mood" field.
  fireEvent.change(screen.getByLabelText("Activity"), { target: { value: "gym" } });
  const moodA = await screen.findByLabelText("How did it feel?");
  fireEvent.change(moodA, { target: { value: "strong" } });
  expect((moodA as HTMLInputElement).value).toBe("strong");

  // Switch to activity B — the same-named field must come up empty.
  fireEvent.change(screen.getByLabelText("Activity"), { target: { value: "swim" } });
  await waitFor(() =>
    expect((screen.getByLabelText("How did it feel?") as HTMLInputElement).value).toBe(""),
  );

  const moodB = screen.getByLabelText("How did it feel?");
  fireEvent.change(moodB, { target: { value: "calm" } });
  fireEvent.click(screen.getByRole("button", { name: "Log it" }));
  expect(await screen.findByText(/one more on the record/)).toBeDefined();

  const logged = calls.find((c) => c.cmd === "log_tracking_entry");
  expect(logged?.args).toMatchObject({
    activity: "swim",
    vars: { mood: "calm" },
  });
  // A's value never rode along.
  expect((logged?.args as { vars: Record<string, string> }).vars).toEqual({ mood: "calm" });
});
