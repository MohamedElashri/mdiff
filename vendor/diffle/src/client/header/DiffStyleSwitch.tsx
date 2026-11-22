import { uiText } from '@/client/i18n/runtime.js';
import { useCommandAction } from '@/client/commands/CommandProvider.js';
import { Columns2, Rows3 } from 'lucide-react';
import { useStore } from '@/client/store.js';
import { SegmentedControl, ToggleButton } from '@/client/ui/ToggleButton.js';

/** Split or unified rows in the diff. */
export function DiffStyleSwitch() {
  const diffStyle = useStore((s) => s.diffStyle);
  const setDiffStyle = useStore((s) => s.setDiffStyle);
  const split = useCommandAction('review.split', () => setDiffStyle('split'));
  const unified = useCommandAction('review.unified', () => setDiffStyle('unified'));
  return (
    <SegmentedControl title={uiText('Diff layout (s)')}>
      <ToggleButton selected={diffStyle === 'split'} onClick={split.run} aria-label={uiText('Split')}>
        <Columns2 size="0.875rem" /> <span className="max-xl:hidden">{uiText('Split')}</span>
      </ToggleButton>
      <ToggleButton selected={diffStyle === 'unified'} onClick={unified.run} aria-label={uiText('Unified')}>
        <Rows3 size="0.875rem" /> <span className="max-xl:hidden">{uiText('Unified')}</span>
      </ToggleButton>
    </SegmentedControl>
  );
}
