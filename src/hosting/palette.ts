import type { Summary, ReviewRef } from "./api";
import type { PaletteEntry } from "../../vendor/diffle/src/client/commands/registry";
import { rankMatches } from "../../vendor/diffle/src/client/commands/search";
/** Search the last bounded discovery page only; opening/typing in the palette performs no provider I/O. */
export function hostingEntries(
  items: Summary[],
  query: string,
  busy: () => boolean,
  open: (review: ReviewRef, title: string) => unknown,
): PaletteEntry[] {
  return rankMatches(
    items.slice(0, 20),
    query,
    (row) =>
      `${row.title} ${row.review.repository} ${row.review.number} ${row.review.site.host}`,
  ).map((row) => ({
    id: `hosting:${row.url}`,
    group: "hosting",
    label: row.title,
    detail: `${row.review.site.host}/${row.review.repository} #${row.review.number}`,
    reason: busy() ? "Wait for the review to finish opening." : null,
    run: () => (busy() ? false : open(row.review, row.title)),
  }));
}
