import type { HistoryRow } from '@/shared/protocol.js';
/** Only fully loaded, linear side branches joining a known first-parent spine are folded.
 * With topo order and the complete prefix, all possible children were already observed.
 * Never infer a fold through an unloaded parent or independently named ref. */
export function mergeFolds(rows: readonly HistoryRow[], completePrefix: boolean): Map<string, Set<string>> {
  const result = new Map<string, Set<string>>();
  if (!completePrefix) return result;
  const byId = new Map(rows.map((row) => [row.sha, row]));
  const used = new Set<string>();
  for (const merge of rows) {
    if (merge.parents.length !== 2) continue;
    const spine = new Set<string>();
    let sha: string | undefined = merge.parents[0];
    while (sha && !spine.has(sha)) {
      spine.add(sha);
      sha = byId.get(sha)?.parents[0];
    }
    const side = new Set<string>();
    sha = merge.parents[1];
    let valid = true;
    while (sha && !spine.has(sha)) {
      const row = byId.get(sha);
      if (
        !row ||
        row.parents.length !== 1 ||
        row.refs.length ||
        row.boundary ||
        used.has(sha) ||
        side.has(sha) ||
        side.size >= 200
      ) {
        valid = false;
        break;
      }
      side.add(sha);
      sha = row.parents[0];
    }
    if (!valid || !sha || !byId.has(sha) || !side.size) continue;
    if (
      rows.some((row) => row.sha !== merge.sha && !side.has(row.sha) && row.parents.some((parent) => side.has(parent)))
    )
      continue;
    result.set(merge.sha, side);
    side.forEach((id) => used.add(id));
  }
  return result;
}
