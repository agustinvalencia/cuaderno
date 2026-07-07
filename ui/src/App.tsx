import { lazy, Suspense } from "react";
import { Route, Routes } from "react-router";
import AppShell from "./shell/AppShell";
import Commitments from "./views/commitments/Commitments";
import Home from "./views/home/Home";
import Placeholder from "./views/Placeholder";

// Code-split the surfaces that aren't on the initial paint and that
// pull heavy deps: Project Detail drags in react-markdown + remark-gfm
// (the note map renderer), so it (and the trivially-splittable Actions
// list) load on navigation rather than sitting in the main chunk. Home
// is the index route and stays eager.
const Actions = lazy(() => import("./views/actions/Actions"));
const ProjectDetail = lazy(() => import("./views/project/ProjectDetail"));

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
        <Route path="weekly" element={<Placeholder view="Weekly Review" milestone="M6" />} />
        <Route path="strategic" element={<Placeholder view="Strategic" milestone="M9" />} />
        <Route path="portfolios" element={<Placeholder view="Portfolios" milestone="M8" />} />
        <Route
          path="stewardships"
          element={<Placeholder view="Stewardships" milestone="M7" />}
        />
        <Route
          path="projects/:slug"
          element={
            <Suspense fallback={<ViewFallback />}>
              <ProjectDetail />
            </Suspense>
          }
        />
      </Route>
    </Routes>
  );
}
