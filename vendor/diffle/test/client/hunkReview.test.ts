import { parsePatchFiles } from '@pierre/diffs';
import { expect, it } from 'vitest';
import { hunkIndexAt, hunkMark, hunkRange, hunkVersion, markHunk } from '@/client/review/hunks.js';
import type { ChangedFile, Snapshot } from '@/shared/protocol.js';

const patch = `diff --git a/example.txt b/example.txt
--- a/example.txt
+++ b/example.txt
@@ -1,5 +1,7 @@
 context
-old
+new
+extra
 context
-second
+another
+last added
 trailing context
@@ -15,3 +17,2 @@
 leading context
-removed
 trailing context
`;
const diff = parsePatchFiles(patch)[0]!.files[0]!;

it('anchors a hunk to its changed line span, including deletion-only hunks', () => {
  expect(hunkRange(diff.hunks[0]!)).toEqual({ side: 'new', startLine: 2, endLine: 6 });
  expect(hunkRange(diff.hunks[1]!)).toEqual({ side: 'old', startLine: 16, endLine: 16 });
  expect(hunkIndexAt(diff, { id: 'example', range: { side: 'deletions', start: 16, end: 16 } })).toBe(1);
  expect(hunkIndexAt(diff, { id: 'example', range: { side: 'additions', start: 10, end: 10 } })).toBe(-1);
});

it('keeps review progress only for the same review, blobs, and hunk layout', () => {
  const snap = { commentKey: 'review', context: 3, oldSha: 'base', newSha: 'head' } as Snapshot;
  const file = { path: 'example.txt', oldBlob: 'old', blob: 'new', status: 'M' } as ChangedFile;
  const key = { path: file.path, review: snap.commentKey, version: hunkVersion(snap, file), total: 2 };
  const marks = markHunk([], { ...key, reviewed: [0] })!;
  expect(hunkMark(marks, snap, file)?.reviewed).toEqual([0]);
  expect(hunkMark(marks, { ...snap, newSha: 'unrelated-commit' }, file)?.reviewed).toEqual([0]);
  expect(hunkMark(marks, snap, { ...file, oldBlob: 'rebased' })).toBeUndefined();
  expect(hunkMark(marks, snap, { ...file, blob: 'amended' })).toBeUndefined();
  expect(hunkMark(marks, { ...snap, commentKey: 'other-review' }, file)).toBeUndefined();
  expect(hunkMark(marks, { ...snap, context: 7 }, file)).toBeUndefined();
  expect(markHunk(marks, { ...key, reviewed: [] })).toEqual([]);
});
