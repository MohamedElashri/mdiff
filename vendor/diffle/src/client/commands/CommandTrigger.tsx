import { Search } from 'lucide-react';
import { useCommandAction, useCommands } from './CommandProvider.js';
import { Button } from '@/client/ui/Button.js';
import { coreMessages as messages, translate } from '@/client/i18n/runtime.js';

export function CommandTrigger() {
  const registry = useCommands();
  const action = useCommandAction('app.palette', () => {});
  if (!registry) return null;
  return (
    <Button
      variant="ghost"
      icon
      onClick={action.run}
      aria-label={translate(messages.palette)}
      title={`${translate(messages.palette)}${action.shortcut ? ` (${action.shortcut})` : ''}`}
    >
      <Search size="1rem" />
    </Button>
  );
}
