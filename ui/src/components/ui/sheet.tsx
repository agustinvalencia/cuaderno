// Sheet — a dialog-based slide-in side panel.
//
// Hand-vendored from the shadcn/ui v4 registry's `sheet` pattern
// (github.com/shadcn-ui/ui, components/ui/sheet.tsx) — the Radix Dialog
// composition, not the CLI or the whole registry. Only what the M5 note
// reader needs is kept: a right-side panel with the focus trap, Esc
// close, and return-focus that Radix Dialog gives for free. Styling is
// swapped from shadcn's own utility set to cuaderno's semantic tokens,
// and the `cn`/cva machinery is dropped in favour of plain class
// concatenation so no extra deps come along.
import type { ComponentProps, ReactNode } from "react";
import * as Dialog from "@radix-ui/react-dialog";

/** Join class fragments, dropping falsy ones — the one thing `cn` did
 * for us here, without pulling in clsx + tailwind-merge. */
function classes(...parts: (string | false | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

export const Sheet = Dialog.Root;
export const SheetTitle = Dialog.Title;
export const SheetDescription = Dialog.Description;
export const SheetClose = Dialog.Close;

/** The slide-in panel itself, portalled over a calm scrim. `side`
 * defaults to the right edge (the note reader's home). Radix owns the
 * focus trap, Esc-to-close, and focus return on unmount; the caller
 * drives open state via `Sheet`'s `open`/`onOpenChange`. */
export function SheetContent({
  children,
  className,
  side = "right",
  ...props
}: ComponentProps<typeof Dialog.Content> & {
  children: ReactNode;
  side?: "right" | "left";
}) {
  return (
    <Dialog.Portal>
      {/* Scrim: no red, just a soft dim; the panel carries the surface. */}
      <Dialog.Overlay className="fixed inset-0 z-40 bg-black/20 data-[state=open]:animate-in" />
      <Dialog.Content
        className={classes(
          "fixed inset-y-0 z-50 flex flex-col border-line bg-bg-surface shadow-lg outline-none",
          side === "right" ? "right-0 border-l" : "left-0 border-r",
          className,
        )}
        {...props}
      >
        {children}
      </Dialog.Content>
    </Dialog.Portal>
  );
}
