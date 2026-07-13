// Note-reading entry point (UI request 2026-07-13). Anything inside the app
// — the commitments timeline's origin chips, the command palette's search
// results, Project Detail's backlinks, an evidence row — opens a note by
// calling `useReader().openReader(path)`. It navigates to the centred note
// page (`/note/<path>`); keeping the route behind this hook (rather than
// threading `useNavigate` and the path shape through every view) lets those
// distant surfaces summon the reader without knowing the route.
import { createContext, useContext, useMemo } from "react";
import type { ReactNode } from "react";
import { useNavigate } from "react-router";

interface ReaderApi {
  /** Open the centred note page on `path` (a vault-relative note path). */
  openReader: (path: string) => void;
}

const ReaderContext = createContext<ReaderApi | null>(null);

export function ReaderProvider({ children }: { children: ReactNode }) {
  const navigate = useNavigate();
  const api = useMemo<ReaderApi>(
    // The path's slashes pass straight through the `/note/*` splat route,
    // so no encoding is needed (vault paths are slug segments, never spaces).
    () => ({ openReader: (path) => navigate(`/note/${path}`) }),
    [navigate],
  );
  return <ReaderContext.Provider value={api}>{children}</ReaderContext.Provider>;
}

export function useReader(): ReaderApi {
  const api = useContext(ReaderContext);
  if (!api) {
    throw new Error("useReader requires a ReaderProvider above it");
  }
  return api;
}
