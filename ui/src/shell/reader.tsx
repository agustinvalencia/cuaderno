// Note-reader hosting (plan §6). One reader panel lives in the shell;
// anything inside it — the commitments timeline's origin chips, the
// command palette's search results, Project Detail's backlinks — opens
// a note by calling `useReader().openReader(path)`. Keeping the open
// path in context (rather than threading callbacks through every view)
// lets those distant surfaces summon the reader without prop drilling.
import { createContext, useContext, useMemo, useState } from "react";
import type { ReactNode } from "react";

interface ReaderApi {
  /** The vault-relative note path the reader is showing, or null. */
  openPath: string | null;
  /** Open (or replace, when already open) the reader on `path`. */
  openReader: (path: string) => void;
  closeReader: () => void;
}

const ReaderContext = createContext<ReaderApi | null>(null);

export function ReaderProvider({ children }: { children: ReactNode }) {
  const [openPath, setOpenPath] = useState<string | null>(null);
  const api = useMemo<ReaderApi>(
    () => ({
      openPath,
      openReader: (path) => setOpenPath(path),
      closeReader: () => setOpenPath(null),
    }),
    [openPath],
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
