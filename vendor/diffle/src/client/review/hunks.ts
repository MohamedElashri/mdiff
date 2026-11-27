import type { CodeViewLineSelection, FileDiffMetadata } from '@pierre/diffs';
import { z } from 'zod';
import type { ChangedFile, Snapshot } from '@/shared/protocol.js';
import { sideOf, type ResolvedRange } from '@/client/comments/anchor.js';

export const hunkProgressSchema = z.array(
  z.object({
    path: z.string(),
    review: z.string(),
    version: z.string(),
    total: z.number().int().positive(),
    reviewed: z.array(z.number().int().nonnegative()),
  }),
);

export type HunkProgress = z.infer<typeof hunkProgressSchema>;
export type HunkMark = HunkProgress[number];

// Leave room for the existing navigation fields in the native workspace view.
export const HUNK_PROGRESS_BYTES = 48 * 1024;

export function hunkVersion(snapshot: Snapshot, file: ChangedFile): string {
  return JSON.stringify([
    snapshot.context,
    file.oldPath,
    file.status,
    file.oldBlob,
    file.blob,
    // If a worktree blob could not be read, don't carry marks across successive snapshots.
    (snapshot.oldSha === 'worktree' && !file.oldBlob && file.status !== 'A') ||
    (snapshot.newSha === 'worktree' && !file.blob && file.status !== 'D')
      ? snapshot.version
      : null,
  ]);
}

export function markHunk(marks: HunkProgress, next: HunkMark): HunkProgress | null {
  const result = marks.filter((m) => m.path !== next.path || m.review !== next.review);
  if (next.reviewed.length) result.push(next);
  return new TextEncoder().encode(JSON.stringify(result)).length <= HUNK_PROGRESS_BYTES ? result : null;
}

const byReview = new WeakMap<HunkProgress, Map<string, Map<string, HunkMark>>>();

export function hunkMarks(marks: HunkProgress, review: string): ReadonlyMap<string, HunkMark> | undefined {
  let index = byReview.get(marks);
  if (!index) {
    index = new Map();
    for (const mark of marks) {
      let files = index.get(mark.review);
      if (!files) index.set(mark.review, (files = new Map()));
      files.set(mark.path, mark);
    }
    byReview.set(marks, index);
  }
  return index.get(review);
}

export function hunkMark(marks: HunkProgress, snapshot: Snapshot, file: ChangedFile): HunkMark | undefined {
  const mark = hunkMarks(marks, snapshot.commentKey)?.get(file.path);
  return mark && mark.version === hunkVersion(snapshot, file) ? mark : undefined;
}

export function hunkIndexAt(diff: FileDiffMetadata, sel: CodeViewLineSelection | null): number {
  if (!sel) return -1;
  const line = sel.range.end;
  const old = sideOf(sel) === 'old';
  return diff.hunks.findIndex((hunk) => {
    const start = old ? hunk.deletionStart : hunk.additionStart;
    const count = old ? hunk.deletionCount : hunk.additionCount;
    return line >= start && line < start + count;
  });
}

// Anchor the first and last changed lines on one side. The same lines still
// exist in the provider diff if its surrounding context differs from the viewer's.
export function hunkRange(hunk: FileDiffMetadata['hunks'][number]): ResolvedRange | null {
  const side = hunk.additionLines > 0 ? 'new' : 'old';
  let line = side === 'new' ? hunk.additionStart : hunk.deletionStart;
  let startLine = 0;
  let endLine = 0;
  for (const block of hunk.hunkContent) {
    if (block.type === 'context') {
      line += block.lines;
      continue;
    }
    const changed = side === 'new' ? block.additions : block.deletions;
    if (changed > 0) {
      if (!startLine) startLine = line;
      endLine = line + changed - 1;
      line += changed;
    }
  }
  return startLine > 0 ? { side, startLine, endLine } : null;
}
