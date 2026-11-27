import { uiText } from '@/client/i18n/runtime.js';
import { pathFromItemId, rangeLabel } from '@/client/model.js';
import { useStore } from '@/client/store.js';
import { Button } from '@/client/ui/Button.js';
import { FloatingPanel } from '@/client/ui/FloatingPanel.js';
import { useMemo, type RefObject } from 'react';
import type { FileDiffMetadata } from '@pierre/diffs';
import { hunkIndexAt, hunkRange } from './hunks.js';

export function HunkReviewBar({
  path,
  diff,
  reviewed,
  commentable,
  trigger,
  panel,
  close,
}: {
  path: string;
  diff: FileDiffMetadata;
  reviewed: readonly number[];
  commentable: boolean;
  trigger: RefObject<HTMLButtonElement | null>;
  panel: RefObject<HTMLDivElement | null>;
  close: () => void;
}) {
  const index = useStore((s) => {
    const sel = s.selection;
    return sel && pathFromItemId(sel.id) === path ? Math.max(0, hunkIndexAt(diff, sel)) : 0;
  });
  const goToHunk = useStore((s) => s.goToHunk);
  const toggle = useStore((s) => s.toggleHunkReviewed);
  const ranges = useMemo(() => diff.hunks.map(hunkRange), [diff.hunks]);
  const range = ranges[index];
  const marks = useMemo(() => new Set(reviewed), [reviewed]);
  const options = useMemo(
    () =>
      ranges.map((anchor, i) => (
        <option key={i} value={i}>
          {marks.has(i) ? '✓ ' : ''}
          {uiText('Hunk {number}', { number: String(i + 1) })} · {anchor && rangeLabel(anchor)}
        </option>
      )),
    [ranges, marks],
  );
  return (
    <FloatingPanel
      anchor={trigger}
      panelRef={panel}
      align="end"
      className="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-canvas px-2.5 py-1.5 text-xs shadow-lg"
      role="group"
      aria-label={uiText('Hunks')}
    >
      <select
        className="rounded-sm border border-border bg-surface px-1.5 py-0.75"
        aria-label={uiText('Select a hunk')}
        value={index}
        onChange={(e) => goToHunk(path, Number(e.target.value))}
      >
        {options}
      </select>
      <Button aria-pressed={marks.has(index)} onClick={() => toggle(path, index)}>
        {marks.has(index) ? uiText('Mark hunk unreviewed') : uiText('Mark hunk reviewed')}
      </Button>
      {commentable && range && (
        <Button
          onClick={() => {
            close();
            goToHunk(path, index, true);
          }}
        >
          {uiText('Comment on this hunk')} · <span className="font-mono">{rangeLabel(range)}</span>
        </Button>
      )}
    </FloatingPanel>
  );
}
