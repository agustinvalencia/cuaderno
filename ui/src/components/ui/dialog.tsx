// Dialog — a centred modal overlay.
//
// The centred sibling of `sheet.tsx`: same hand-vendored Radix Dialog
// composition (github.com/shadcn-ui/ui), same swap of shadcn's utility
// classes for cuaderno's semantic tokens and plain class concatenation
// (no cn/cva, no extra deps). Where Sheet slides in from an edge, this
// centres a small panel — the home of the Strategic allocator's gentle
// "room for five, park one to make space" cap modal (M9, plan §1.5).
// Radix owns the focus trap, Esc-to-close, and focus return; the scrim
// is a soft dim, never an alarm (no red token exists).
import type { ComponentProps, ReactNode } from "react";
import * as RadixDialog from "@radix-ui/react-dialog";

/** Join class fragments, dropping falsy ones. */
function classes(...parts: (string | false | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

export const Dialog = RadixDialog.Root;
export const DialogTitle = RadixDialog.Title;
export const DialogDescription = RadixDialog.Description;
export const DialogClose = RadixDialog.Close;

/** The centred panel, portalled over a calm scrim. The caller drives
 * open state via `Dialog`'s `open`/`onOpenChange`. */
export function DialogContent({
  children,
  className,
  ...props
}: ComponentProps<typeof RadixDialog.Content> & { children: ReactNode }) {
  return (
    <RadixDialog.Portal>
      {/* Scrim: a soft dim, no red — the panel carries the surface. */}
      <RadixDialog.Overlay className="fixed inset-0 z-40 bg-black/20 data-[state=open]:animate-[dialog-overlay-in_160ms_ease-out] data-[state=closed]:animate-[dialog-overlay-out_120ms_ease-in]" />
      <RadixDialog.Content
        className={classes(
          "fixed left-1/2 top-1/2 z-50 flex w-[min(28rem,calc(100vw-2rem))] -translate-x-1/2 -translate-y-1/2 flex-col rounded-lg border border-line bg-bg-surface p-6 shadow-lg outline-none",
          "data-[state=open]:animate-[dialog-in_160ms_ease-out] data-[state=closed]:animate-[dialog-out_120ms_ease-in]",
          className,
        )}
        {...props}
      >
        {children}
      </RadixDialog.Content>
    </RadixDialog.Portal>
  );
}
