import { uiText } from '@/client/i18n/runtime.js';
import { useCommandAction } from '@/client/commands/CommandProvider.js';
import { ArrowLeft, ArrowRight } from 'lucide-react';
import { canJumpBack, canJumpForward } from '@/client/model.js';
import { useStore } from '@/client/store.js';
import { Button } from '@/client/ui/Button.js';

/**
 * Back and forward through the jump list, the mouse's share of Ctrl+o / Ctrl+i. Always in place
 * and greyed out where the walk has nowhere to go. The wording is neutral because a jump can lead
 * to a diff, a whole file, or a file outside the repository.
 */
export function HistoryNav() {
  const back = useStore(canJumpBack);
  const forward = useStore(canJumpForward);
  const jumpBack = useStore((s) => s.jumpBack);
  const jumpForward = useStore((s) => s.jumpForward);
  const backCommand = useCommandAction('review.back', jumpBack);
  const forwardCommand = useCommandAction('review.forward', jumpForward);
  return (
    <span className="inline-flex items-center gap-0.5">
      <Button
        variant="ghost"
        icon
        disabled={!back}
        onClick={backCommand.run}
        title={uiText('Back{value0}', { value0: String(backCommand.shortcut ? ` (${backCommand.shortcut})` : '') })}
        aria-label={uiText('Back')}
      >
        <ArrowLeft size="1rem" />
      </Button>
      <Button
        variant="ghost"
        icon
        disabled={!forward}
        onClick={forwardCommand.run}
        title={uiText('Forward{value0}', {
          value0: String(forwardCommand.shortcut ? ` (${forwardCommand.shortcut})` : ''),
        })}
        aria-label={uiText('Forward')}
      >
        <ArrowRight size="1rem" />
      </Button>
    </span>
  );
}
