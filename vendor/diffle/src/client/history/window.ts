import type { HistoryPage } from '@/shared/protocol.js';
const sizes = new WeakMap<HistoryPage, number>();
export const HISTORY_WINDOW_BYTES = 8 * 1024 * 1024;
/** Bound serialized payload as well as row count; this is not a JS heap measurement. */
export function historyWindow(pages: readonly HistoryPage[], direction: -1 | 1): HistoryPage[] {
  const kept = [...pages];
  const bytes = (page: HistoryPage) => {
    let size = sizes.get(page);
    if (size === undefined) {
      size = new TextEncoder().encode(JSON.stringify(page)).byteLength;
      sizes.set(page, size);
    }
    return size;
  };
  let total = kept.reduce((sum, page) => sum + bytes(page), 0);
  while (kept.length > 1 && (kept.length > 10 || total > HISTORY_WINDOW_BYTES)) {
    total -= bytes((direction === 1 ? kept.shift() : kept.pop())!);
  }
  return kept;
}
