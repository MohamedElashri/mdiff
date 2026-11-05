import { afterEach, expect, it, vi } from 'vitest';
import { completePatchPrefix } from '@/client/patchPages.js';
import { api } from '@/client/api.js';

const header = 'diff --git a/a b/a\n--- a/a\n+++ b/a\n';
const one = '@@ -1 +1 @@\n-before\n+after\n';
const two = '@@ -10 +10 @@\n-old\n+new\n';
afterEach(() => vi.unstubAllGlobals());
it('never renders an incomplete hunk or loses the final no-newline marker', () => {
  expect(completePatchPrefix(header + one, false)).toBe('');
  expect(completePatchPrefix(header + one + two.slice(0, 22), false)).toBe(header + one);
  const full = header + one + two + '\\ No newline at end of file\n';
  expect(completePatchPrefix(full, true)).toBe(full);
});
it('pulls chunks sequentially, renders complete hunks, and preserves exact final text', async () => {
  vi.stubGlobal('location', new URL('http://localhost/'));
  const parts = [header + one + two.slice(0, 20), two.slice(20)];
  let offset = 0;
  const fetcher = vi.fn(async () => {
    const patch = parts.shift()!;
    offset += new TextEncoder().encode(patch).length;
    return new Response(
      JSON.stringify({ version: 1, patch, next: offset, total: 200, digest: 'abc', done: parts.length === 0 }),
    );
  });
  vi.stubGlobal('fetch', fetcher);
  const preview = vi.fn();
  expect(await api.patchPages('a', 1, new AbortController().signal, preview)).toBe(header + one + two);
  expect(preview).toHaveBeenCalledWith(header + one);
  expect(fetcher).toHaveBeenCalledTimes(2);
});
it('stops fetching on cancellation and rejects a chunk from a different snapshot', async () => {
  vi.stubGlobal('location', new URL('http://localhost/'));
  const controller = new AbortController();
  const fetcher = vi.fn(
    async () =>
      new Response(JSON.stringify({ version: 2, patch: 'bad', next: 3, total: 3, digest: 'abc', done: true })),
  );
  vi.stubGlobal('fetch', fetcher);
  await expect(api.patchPages('a', 1, controller.signal, vi.fn())).rejects.toThrow('review changed');
  controller.abort();
  await expect(api.patchPages('a', 1, controller.signal, vi.fn())).rejects.toThrow();
  expect(fetcher).toHaveBeenCalledTimes(1);
});

it('keeps cumulative preview parsing work linear in the final patch size', async () => {
  vi.stubGlobal('location', new URL('http://localhost/'));
  const full = header + one.repeat(40000);
  let offset = 0;
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => {
      const patch = full.slice(offset, offset + 64 * 1024);
      offset += patch.length;
      return new Response(
        JSON.stringify({
          version: 1,
          patch,
          next: offset,
          total: full.length,
          digest: 'abc',
          done: offset === full.length,
        }),
      );
    }),
  );
  let parsed = 0;
  expect(
    await api.patchPages('a', 1, new AbortController().signal, (prefix) => {
      parsed += prefix.length;
    }),
  ).toBe(full);
  expect(parsed).toBeLessThan(full.length * 3);
});
