import { lazy, Suspense } from "react";
import { Navigate, Route, Routes } from "react-router";
import AppShell from "./shell/AppShell";
import Commitments from "./views/commitments/Commitments";
import Home from "./views/home/Home";

// Code-split the surfaces that aren't on the initial paint and that
// pull heavy deps: Project Detail drags in react-markdown + remark-gfm
// (the note map renderer), so it (and the trivially-splittable Actions
// list) load on navigation rather than sitting in the main chunk. Home
// is the index route and stays eager.
const Actions = lazy(() => import("./views/actions/Actions"));
const ProjectDetail = lazy(() => import("./views/project/ProjectDetail"));
// Weekly Review pulls in the shared commitments timeline (and the 5
// step components); load it on navigation rather than the main chunk.
const WeeklyReview = lazy(() => import("./views/weekly/WeeklyReview"));
// Stewardship Detail drags in recharts (the trend charts) and
// react-markdown; the list is trivially splittable alongside it. Both
// load on navigation rather than sitting in the main chunk.
const Stewardships = lazy(() => import("./views/stewardships/Stewardships"));
const StewardshipDetail = lazy(() => import("./views/stewardships/StewardshipDetail"));
// Portfolio Browser (M8): the selector list and the composed detail
// (evidence timeline + quick-add composer + links sidebar). Split onto
// navigation like the other secondary surfaces.
const Portfolios = lazy(() => import("./views/portfolios/Portfolios"));
const PortfolioDetail = lazy(() => import("./views/portfolios/PortfolioDetail"));
// Monthly review (M9; made a stepped ritual in #450): the six-step
// review — questions, portfolios, projects, stewardships, the six-week
// lookahead, and a focus step that writes Wins/Themes/Focus to the
// monthly note. Pulls in recharts (sparklines) and the shared timeline,
// so it splits onto navigation.
const MonthlyReview = lazy(() => import("./views/monthly/MonthlyReview"));
// Questions (#443): RLM's Important Questions as a surface of their own,
// rather than chips inside the monthly dashboard. Splits onto navigation
// like the other secondary surfaces.
const Questions = lazy(() => import("./views/questions/Questions"));
// Calendar (#340): the month grid + embedded daily/weekly/monthly panel.
// Pulls in react-markdown (the note renderer), so it splits onto
// navigation like the other secondary surfaces.
const Calendar = lazy(() => import("./views/calendar/Calendar"));
// Templates (#357): the note-type template browser + editor. Splits onto
// navigation like the other secondary surfaces.
const Templates = lazy(() => import("./views/templates/Templates"));
// Config inspector (#365): the read-only config.toml viewer + dry-run
// validate. Splits onto navigation like the other secondary surfaces.
const Config = lazy(() => import("./views/config/Config"));
// The centred note page (UI request 2026-07-13): the full-page reader/editor
// that replaced the slide-in drawer. Pulls react-markdown + KaTeX + (on Edit)
// CodeMirror, so it splits onto navigation like the other secondary surfaces.
const NotePage = lazy(() => import("./views/note/NotePage"));

/** Calm placeholder while a lazily-loaded view chunk downloads. */
function ViewFallback() {
  return <p className="p-8 text-ink-muted">Loading…</p>;
}

export default function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<Home />} />
        <Route
          path="actions"
          element={
            <Suspense fallback={<ViewFallback />}>
              <Actions />
            </Suspense>
          }
        />
        <Route path="commitments" element={<Commitments />} />
        <Route
          path="calendar"
          element={
            <Suspense fallback={<ViewFallback />}>
              <Calendar />
            </Suspense>
          }
        />
        <Route
          path="weekly"
          element={
            <Suspense fallback={<ViewFallback />}>
              <WeeklyReview />
            </Suspense>
          }
        />
        <Route
          path="monthly"
          element={
            <Suspense fallback={<ViewFallback />}>
              <MonthlyReview />
            </Suspense>
          }
        />
        {/* "Strategic" named a dashboard; "Monthly" names the review in
            the cadence it belongs to (#444). The old path stays as a
            redirect so anything still pointing at it — an in-app link
            written before the rename, a `navigate("/strategic")` left in
            code — lands rather than falling through to a blank shell.
            (Not `cuaderno://` links: that scheme only ever carries note
            paths, see `deeplink.rs`.) `replace` keeps it out of the
            history, so Back does not bounce off it. */}
        <Route path="strategic" element={<Navigate to="/monthly" replace />} />
        <Route
          path="questions"
          element={
            <Suspense fallback={<ViewFallback />}>
              <Questions />
            </Suspense>
          }
        />
        <Route
          path="portfolios"
          element={
            <Suspense fallback={<ViewFallback />}>
              <Portfolios />
            </Suspense>
          }
        />
        <Route
          path="portfolios/:slug"
          element={
            <Suspense fallback={<ViewFallback />}>
              <PortfolioDetail />
            </Suspense>
          }
        />
        <Route
          path="stewardships"
          element={
            <Suspense fallback={<ViewFallback />}>
              <Stewardships />
            </Suspense>
          }
        />
        <Route
          path="stewardships/:slug"
          element={
            <Suspense fallback={<ViewFallback />}>
              <StewardshipDetail />
            </Suspense>
          }
        />
        <Route
          path="projects/:slug"
          element={
            <Suspense fallback={<ViewFallback />}>
              <ProjectDetail />
            </Suspense>
          }
        />
        <Route
          path="templates"
          element={
            <Suspense fallback={<ViewFallback />}>
              <Templates />
            </Suspense>
          }
        />
        <Route
          path="config"
          element={
            <Suspense fallback={<ViewFallback />}>
              <Config />
            </Suspense>
          }
        />
        <Route
          path="note/*"
          element={
            <Suspense fallback={<ViewFallback />}>
              <NotePage />
            </Suspense>
          }
        />
      </Route>
    </Routes>
  );
}
