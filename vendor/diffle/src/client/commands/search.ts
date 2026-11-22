/** Small transposition/one-edit tolerance, without a history index or fuzzy-search dependency. */
function oneEdit(a: string, b: string): boolean {
  if (Math.abs(a.length - b.length) > 1) return false;
  let i = 0;
  while (i < a.length && a[i] === b[i]) i++;
  if (a.length === b.length)
    return (
      a.slice(i + 1) === b.slice(i + 1) || (a[i] === b[i + 1] && a[i + 1] === b[i] && a.slice(i + 2) === b.slice(i + 2))
    );
  return a.length > b.length ? a.slice(i + 1) === b.slice(i) : a.slice(i) === b.slice(i + 1);
}
export function fuzzyScore(query: string, text: string): number {
  const needle = query.trim().toLowerCase().slice(0, 128);
  const haystack = text.toLowerCase().slice(0, 2048);
  if (!needle) return 0;
  const exact = haystack.indexOf(needle);
  if (exact >= 0) return 1000 - exact - Math.min(haystack.length, 200) / 100;
  let previous = -1;
  let gaps = 0;
  for (const letter of needle) {
    const index = haystack.indexOf(letter, previous + 1);
    if (index < 0) {
      previous = -1;
      break;
    }
    gaps += index - previous - 1;
    previous = index;
  }
  if (previous >= 0) return 500 - gaps;
  if (needle.length >= 3 && needle.length <= 40 && haystack.split(/[\s/._:-]+/).some((word) => oneEdit(needle, word)))
    return 200;
  return -Infinity;
}

/** Retain only the best K while scanning. No 20k-element result/closure allocation per keypress. */
export function rankMatches<T>(items: readonly T[], query: string, text: (item: T) => string, limit = 20): T[] {
  if (limit <= 0) return [];
  const best: { item: T; score: number; order: number }[] = [];
  for (let order = 0; order < items.length; order++) {
    const item = items[order]!;
    const score = fuzzyScore(query, text(item));
    if (!Number.isFinite(score) || (best.length >= limit && score <= best.at(-1)!.score)) continue;
    const index = best.findIndex((entry) => score > entry.score);
    best.splice(index < 0 ? best.length : index, 0, { item, score, order });
    if (best.length > limit) best.pop();
  }
  return best.map((entry) => entry.item);
}
