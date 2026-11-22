import type { Worktree } from "./api";
import type { PaletteEntry } from "../../vendor/diffle/src/client/commands/registry";
import { rankMatches } from "../../vendor/diffle/src/client/commands/search";
export function worktreeEntries(
  rows: Worktree[],
  query: string,
  busy: boolean,
  open: (row: Worktree) => unknown,
): PaletteEntry[] {
  return rankMatches(
    rows.slice(0, 256),
    query,
    (row) => `${row.branch ?? "Detached"} ${row.path}`,
  ).map((row) => ({
    id: `worktree:${row.id}`,
    group: "worktrees",
    label: row.branch ?? (row.bare ? "Bare repository" : "Detached HEAD"),
    detail: row.path,
    reason: busy
      ? "Wait for the current review to finish opening."
      : row.canOpen
        ? null
        : (row.reason ?? "Working folder unavailable."),
    run: () => (!busy && row.canOpen ? open(row) : false),
  }));
}
