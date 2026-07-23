// The seam a settings panel uses to say "I am holding an unsaved draft".
//
// Every preference in the Settings dialog applies the instant it is
// clicked, so "closing is always safe" was baked into its design. Vault
// config and Templates break that assumption: both are real editors with
// a draft/baseline model and an explicit Save, and Radix would hand a
// silent discard to Esc, an overlay click, or Done (#444).
//
// A panel reports through this context rather than by prop, so the same
// component works unchanged as a full page — where there is no close to
// guard and the context is absent.
import { createContext, useContext, useEffect } from "react";

/** Report the panel under `key` as dirty (a human label naming what is
 * unsaved) or clean (`null`). */
type Report = (key: string, label: string | null) => void;

const DirtyContext = createContext<Report>(() => {});

export const SettingsDirtyProvider = DirtyContext.Provider;

/** Keep the hosting Settings dialog informed of this panel's unsaved
 * state. A no-op outside the dialog.
 *
 * The cleanup reports clean: a panel that has gone away cannot be holding
 * anything, and leaving a stale `true` behind would wedge the dialog shut. */
export function useReportDirty(key: string, label: string, dirty: boolean) {
  const report = useContext(DirtyContext);
  useEffect(() => {
    report(key, dirty ? label : null);
    return () => report(key, null);
  }, [report, key, label, dirty]);
}
