// Shared date formatting. `shortDate` was copied verbatim into seven
// views; one home for it keeps the app's date presentation consistent and
// makes a format tweak a single edit. Dates are parsed at local midnight
// (`T00:00:00`) so a `YYYY-MM-DD` never slips a day across a timezone —
// never `new Date("YYYY-MM-DD")`, which parses as UTC.

/** `2 Jul` — day and short month, in the viewer's locale. */
export function shortDate(date: string): string {
  return new Date(`${date}T00:00:00`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
}
