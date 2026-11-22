import { useId, useState, type RefObject } from 'react';
import { useCommands, useCommandVersion } from './CommandProvider.js';
import { shortcutFromEvent, shortcutLabel } from './shortcuts.js';
import { rankMatches } from './search.js';
import { messages, translate } from '@/client/i18n/messages.js';
import { Dialog, DialogContent } from '@/client/ui/dialog.js';
import { Button } from '@/client/ui/Button.js';

export function ShortcutSettings() {
  const registry = useCommands();
  useCommandVersion(registry);
  const [query, setQuery] = useState('');
  const [recording, setRecording] = useState<string | null>(null);
  const [error, setError] = useState('');
  const status = useId();
  if (!registry) return null;
  const commands = rankMatches(registry.definitions(), query, (command) => translate(command.label), 60);
  return (
    <section className="shortcut-settings" aria-label={translate(messages.shortcuts)}>
      <p>
        {translate(messages.saved)} {translate(messages.shortcutScope)}
      </p>
      <input
        className="shortcut-filter"
        aria-label={translate(messages.filter)}
        placeholder={translate(messages.filter)}
        value={query}
        onChange={(event) => setQuery(event.target.value)}
      />
      <div className="shortcut-list">
        {commands.map((command) => (
          <div className="shortcut-row" key={command.id}>
            <span>
              {translate(command.label)}
              {command.key && <small>{translate(messages.vim, { key: command.key })}</small>}
            </span>
            {recording === command.id ? (
              <input
                autoFocus
                readOnly
                data-shortcut-recorder
                aria-describedby={status}
                aria-label={translate(messages.record, { command: translate(command.label) })}
                placeholder={translate(messages.press)}
                onKeyDown={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  if (event.nativeEvent.isComposing || ['Control', 'Meta', 'Shift', 'Alt'].includes(event.key)) return;
                  if (event.key === 'Escape') {
                    setRecording(null);
                    return;
                  }
                  const binding = shortcutFromEvent(event.nativeEvent, registry.platform);
                  const reason = binding ? registry.setShortcut(command.id, binding) : translate(messages.invalid);
                  setError(reason ?? '');
                  if (!reason) setRecording(null);
                }}
                onBlur={() => setRecording(null)}
              />
            ) : (
              <Button
                aria-label={translate(messages.record, { command: translate(command.label) })}
                onClick={() => {
                  setRecording(command.id);
                  setError('');
                }}
              >
                {shortcutLabel(registry.shortcut(command.id), registry.platform) || translate(messages.none)}
              </Button>
            )}
            <Button
              variant="ghost"
              disabled={!registry.shortcut(command.id)}
              aria-label={translate(messages.clearCommand, { command: translate(command.label) })}
              onClick={() => {
                registry.setShortcut(command.id, null);
                setError('');
              }}
            >
              {translate(messages.clear)}
            </Button>
          </div>
        ))}
      </div>
      <p id={status} role="status">
        {error || (recording ? translate(messages.recording) : '')}
      </p>
      <Button
        onClick={() => {
          registry.resetShortcuts();
          setRecording(null);
          setError('');
        }}
      >
        {translate(messages.reset)}
      </Button>
    </section>
  );
}

export function ShortcutDialog({
  open,
  close,
  previousFocus,
}: {
  open: boolean;
  close(): void;
  previousFocus: RefObject<HTMLElement | null>;
}) {
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!value) close();
      }}
      onOpenChangeComplete={(value) => {
        if (!value && previousFocus.current?.isConnected) previousFocus.current.focus({ preventScroll: true });
      }}
    >
      <DialogContent aria-label={translate(messages.shortcuts)} finalFocus={false}>
        <div className="shortcut-heading">
          <h3>{translate(messages.shortcuts)}</h3>
          <Button onClick={close}>{translate(messages.done)}</Button>
        </div>
        <ShortcutSettings />
      </DialogContent>
    </Dialog>
  );
}
