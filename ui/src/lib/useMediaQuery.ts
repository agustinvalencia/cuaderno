// A media query as React state.
//
// Layout that only CSS knows about is layout no component can reason
// about: the Calendar's month grid is a pinned column on a wide window
// and a collapsible block on a narrow one, and choosing a day should
// collapse it in the second case but not the first (#446). A Tailwind
// `lg:` class cannot answer that, and it cannot be asserted in jsdom
// either, where no stylesheet is applied.
//
// `matchMedia` is absent in some test environments; treated as "no match"
// rather than thrown on, so a component using this still renders.
import { useSyncExternalStore } from "react";

export function useMediaQuery(query: string): boolean {
  return useSyncExternalStore(
    (onChange) => {
      const list = window.matchMedia?.(query);
      if (!list) return () => {};
      // `addListener` is the pre-2019 spelling; jsdom stubs in this repo
      // supply the modern pair, and Safari < 14 only the old one.
      list.addEventListener?.("change", onChange);
      return () => list.removeEventListener?.("change", onChange);
    },
    () => window.matchMedia?.(query).matches ?? false,
    () => false,
  );
}

/** The breakpoint the app treats as "there is room for two columns".
 *
 * A media query measures the *viewport*, and the shell spends the first
 * 15-17.5rem of it on the sidebar — so this is Tailwind's `lg` plus that
 * allowance rather than `lg` itself, or the second column would start
 * appearing while the content pane still had `lg`-minus-a-sidebar to
 * give it. The window minimum is 800px wide, so the narrow case is real,
 * not theoretical. */
export const WIDE = "(min-width: 1300px)";
