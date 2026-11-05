import { expect, it } from 'vitest';
import { patchBatches, PATCH_BATCH_FILES, PATCH_BATCH_LINES, reuseThreads } from '@/client/model.js';
import type { ChangedFile, CommentThread, Snapshot } from '@/shared/protocol.js';

it('schedules a 20,000-file review within a desktop responsiveness budget', () => {
  const files: ChangedFile[] = Array.from({ length: 20_000 }, (_, i) => ({
    path: `src/group${i % 100}/file${i}.ts`,
    status: 'M',
    additions: 30,
    deletions: 10,
    binary: false,
    generated: false,
    blob: 'new',
    oldBlob: 'old',
  }));
  const first = files.slice(-10_000).map((file) => file.path);
  const start = performance.now();
  const batches = patchBatches({ changed: files } as Snapshot, files, first, () => false);
  const elapsed = performance.now() - start;
  expect(new Set(batches.flat()).size).toBe(files.length);
  expect(first).toContain(batches[0]![0]);
  expect(batches.every((batch) => batch.length <= PATCH_BATCH_FILES && batch.length * 40 <= PATCH_BATCH_LINES)).toBe(
    true,
  );
  // A deliberately generous CI ceiling catches quadratic growth, not machine-speed differences.
  expect(elapsed).toBeLessThan(2000);
  console.info(`20k-file scheduling: ${elapsed.toFixed(1)} ms`);
});

it('keeps 5,000 unchanged threads stable across repeated refreshes', () => {
  const threads = Array.from({ length: 5000 }, (_, i) => ({
    id: String(i),
    anchor: { kind: 'file', path: `file${i}.ts` },
    resolved: false,
    stale: false,
    messages: [{ id: `m${i}`, body: 'Review fixture', createdAt: 0, updatedAt: 0 }],
  })) as CommentThread[];
  const start = performance.now();
  for (let i = 0; i < 10; i++) expect(reuseThreads(threads, structuredClone(threads))).toBe(threads);
  expect(performance.now() - start).toBeLessThan(2000);
});
