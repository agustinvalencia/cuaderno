import { Route, Routes } from "react-router";
import AppShell from "./shell/AppShell";
import Commitments from "./views/commitments/Commitments";
import Home from "./views/home/Home";
import Placeholder from "./views/Placeholder";

export default function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<Home />} />
        <Route path="actions" element={<Placeholder view="Actions" milestone="M5" />} />
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
          element={<Placeholder view="Project detail" milestone="M5" />}
        />
      </Route>
    </Routes>
  );
}
