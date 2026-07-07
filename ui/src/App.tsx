import { Route, Routes } from "react-router";
import AppShell from "./shell/AppShell";
import Actions from "./views/actions/Actions";
import Commitments from "./views/commitments/Commitments";
import Home from "./views/home/Home";
import ProjectDetail from "./views/project/ProjectDetail";
import Placeholder from "./views/Placeholder";

export default function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<Home />} />
        <Route path="actions" element={<Actions />} />
        <Route path="commitments" element={<Commitments />} />
        <Route path="weekly" element={<Placeholder view="Weekly Review" milestone="M6" />} />
        <Route path="strategic" element={<Placeholder view="Strategic" milestone="M9" />} />
        <Route path="portfolios" element={<Placeholder view="Portfolios" milestone="M8" />} />
        <Route
          path="stewardships"
          element={<Placeholder view="Stewardships" milestone="M7" />}
        />
        <Route path="projects/:slug" element={<ProjectDetail />} />
      </Route>
    </Routes>
  );
}
