// @vitest-environment jsdom
import { act, createElement, StrictMode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import type { HistoryRow, HistoryInfo } from '@/shared/protocol.js';
import { mergeFolds } from '@/client/history/folds.js';
import HistoryBrowser from '@/client/history/HistoryBrowser.js';
import type { HistoryTransport } from '@/client/history/api.js';
const row = (sha: string, parents: string[], extra: Partial<HistoryRow> = {}): HistoryRow => ({
  sha,
  parents,
  subject: sha,
  author: 'Ada',
  date: 1700000000,
  refs: [],
  lane: 0,
  incoming: false,
  edges: [],
  boundary: false,
  index: 0,
  ...extra,
});
it('folds only verified side chains and preserves named refs, unknown parents and outside children', () => {
  const rows = [row('M', ['A', 'B']), row('B', ['C']), row('C', ['R']), row('A', ['R']), row('R', [])];
  expect([...mergeFolds(rows, true).get('M')!]).toEqual(['B', 'C']);
  expect(mergeFolds(rows, false).size).toBe(0);
  expect(
    mergeFolds(
      rows.map((r) => (r.sha === 'B' ? { ...r, refs: ['HEAD'] } : r)),
      true,
    ).size,
  ).toBe(0);
  expect(
    mergeFolds(
      rows.filter((r) => r.sha !== 'C'),
      true,
    ).size,
  ).toBe(0);
  expect(mergeFolds([row('X', ['C']), ...rows], true).size).toBe(0);
});
let root: Root, host: HTMLDivElement;
const info: HistoryInfo = {
  root: '/repo',
  gitDir: '/repo/.git',
  commonDir: '/repo/.git',
  head: 'a'.repeat(40),
  branch: 'refs/heads/main',
  refs: [],
  refsTruncated: false,
  shallow: false,
  partial: false,
  stamp: 'pin',
};
const rows = Array.from({ length: 100 }, (_, i) =>
  row(i.toString(16).padStart(40, '0'), [String(i + 1).padStart(40, '0')], { subject: `Commit ${i}`, index: i }),
);
const calls: { path: string; init?: RequestInit }[] = [];
const response = (value: unknown) => ({
  ok: true,
  status: 200,
  statusText: '',
  json: async () => value,
  text: async () => JSON.stringify(value),
  blob: async () => new Blob(),
});
const transport: HistoryTransport = async (path, init) => {
  calls.push({ path, init });
  if (path.endsWith('/info')) return response(info);
  if (path === 'api/history/query') {
    const query = JSON.parse(String(init?.body));
    return response({
      generation: query.generation,
      page: 0,
      rows: query.search ? rows.filter((r) => r.subject.includes(query.search)) : rows,
      next: 1,
      scanned: 100,
      limited: false,
      notice: null,
    });
  }
  if (path.includes('/query/')) return response(true);
  if (path.includes('/commit/')) {
    const sha = path.split('/').at(-1)!;
    return response({ sha, tree: 'b'.repeat(40), parents: [], message: 'Details', author: 'Ada', date: 1700000000 });
  }
  if (path.includes('/tree/')) return response({ entries: [], truncated: false });
  if (path.includes('/compare'))
    return response({ kind: 'comparison', old: null, new: 'a'.repeat(40), base: 'direct' });
  if (path.includes('/page/'))
    return response({
      generation: path.split('/').at(-2),
      page: 1,
      rows: rows.map((r) => ({ ...r, sha: 'f' + r.sha.slice(1), index: r.index + 100 })),
      next: 2,
      scanned: 200,
      limited: false,
      notice: null,
    });
  throw Error(path);
};
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    },
  );
  calls.length = 0;
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});
async function render(strict = false) {
  await act(() =>
    root.render(
      createElement(
        strict ? StrictMode : 'div',
        null,
        createElement(HistoryBrowser, { transport, onClose: () => {}, onReview: async () => {} }),
      ),
    ),
  );
}
it('virtualizes loaded metadata and reads details/trees only on selection', async () => {
  await render();
  expect(host.querySelectorAll('[role="option"]').length).toBeLessThan(40);
  expect(calls.some((c) => c.path.includes('/commit/') || c.path.includes('/tree/'))).toBe(false);
  await act(() => host.querySelector<HTMLElement>('[role="option"]')!.click());
  expect(calls.some((c) => c.path.includes('/commit/'))).toBe(true);
  expect(calls.some((c) => c.path.includes('/tree/'))).toBe(true);
  expect(host.textContent).toContain('Empty tree (root commit)');
});
it('uses generation-specific cleanup during Strict Mode effect replay', async () => {
  await render(true);
  const starts = calls
    .filter((c) => c.path === 'api/history/query')
    .map((c) => JSON.parse(String(c.init?.body)).generation);
  expect(starts.length).toBeGreaterThan(1);
  expect(new Set(starts).size).toBe(starts.length);
  const closes = calls.filter((c) => c.path.includes('/query/')).map((c) => c.path.split('/').at(-1));
  expect(closes).toContain(starts[0]);
  expect(closes).not.toContain(starts.at(-1));
});
it('pins server filtering to a new generation instead of hiding rows in the old cursor', async () => {
  await render();
  const first = calls.find((c) => c.path === 'api/history/query')!;
  const input = host.querySelector<HTMLInputElement>('[aria-label="Subject or SHA"]')!;
  await act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'Commit 5');
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await act(() => host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  const starts = calls.filter((c) => c.path === 'api/history/query');
  expect(starts).toHaveLength(2);
  expect(JSON.parse(String(starts[1]!.init?.body)).generation).not.toBe(
    JSON.parse(String(first.init?.body)).generation,
  );
  expect(host.querySelectorAll('[role="option"]')).toHaveLength(11);
});

it('bounds the history window by serialized bytes as well as pages', async () => {
  const { historyWindow, HISTORY_WINDOW_BYTES } = await import('@/client/history/window.js');
  const page = (index: number, size: number) => ({
    generation: 'q',
    page: index,
    rows: [],
    next: index + 1,
    scanned: 0,
    limited: false,
    notice: 'x'.repeat(size),
  });
  expect(
    historyWindow(
      Array.from({ length: 12 }, (_, i) => page(i, 0)),
      1,
    ).map((p) => p.page),
  ).toEqual([2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
  const large = historyWindow([page(0, HISTORY_WINDOW_BYTES / 2), page(1, HISTORY_WINDOW_BYTES / 2), page(2, 1)], 1);
  expect(large.map((p) => p.page)).toEqual([1, 2]);
});

it('uses local calendar boundaries rather than assuming every date has 86400 seconds', async () => {
  const { historyDate } = await import('@/client/history/dates.js');
  expect(historyDate('2025-03-30')).toBe(new Date(2025, 2, 30).getTime() / 1000);
  expect(historyDate('2025-03-30', true)).toBe(new Date(2025, 2, 31).getTime() / 1000 - 1);
  expect(historyDate('2025-10-26', true)).toBe(new Date(2025, 9, 27).getTime() / 1000 - 1);
  expect(historyDate('')).toBeNull();
});

it('waits for the matching pushed comparison and never closes a replacement navigation', async () => {
  const { waitForHistoryReview } = await import('@/client/history/navigation.js');
  const request = { kind: 'revspec' as const, args: ['abc^!'] };
  let review: import('@/shared/protocol.js').Review | undefined;
  let listener!: (value?: import('@/shared/protocol.js').Review) => void;
  const dispose = vi.fn();
  const pending = waitForHistoryReview(
    request,
    () => review,
    (callback) => {
      listener = callback;
      return dispose;
    },
    new AbortController().signal,
  );
  review = { kind: 'commit', sha: 'abc' };
  listener(review);
  await pending;
  expect(dispose).toHaveBeenCalledOnce();
  review = undefined;
  const abort = new AbortController();
  const cancelled = waitForHistoryReview(
    request,
    () => review,
    () => dispose,
    abort.signal,
  );
  abort.abort();
  await expect(cancelled).rejects.toMatchObject({ name: 'AbortError' });
  const replaced = waitForHistoryReview(
    request,
    () => review,
    (callback) => {
      listener = callback;
      return dispose;
    },
    new AbortController().signal,
  );
  listener({ kind: 'commit', sha: 'different' });
  await expect(replaced).rejects.toThrow('Another comparison');
});
