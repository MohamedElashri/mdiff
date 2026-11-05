import { AppearancePanel } from '@/client/appearance/AppearancePanel.js';
import { Dialog, DialogContent } from '@/client/ui/dialog.js';
import { Button } from '@/client/ui/Button.js';
import { useEffect, useRef, useState } from 'react';
import { useStore } from '@/client/store.js';
import type { FollowRefs } from '@/shared/protocol.js';

const FOLLOW_REFS: [FollowRefs, string][] = [
  ['on', 'Always'],
  ['auto', 'Auto (not on branches)'],
  ['off', 'Never'],
];

export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const config = useStore((s) => s.config);
  const saveConfig = useStore((s) => s.saveConfig);
  const [text, setText] = useState(config.autoViewed.join('\n'));
  const [context, setContext] = useState(String(config.contextLines));
  const [followRefs, setFollowRefs] = useState(config.followRefs);
  const [lineWrap, setLineWrap] = useState(config.lineWrap);
  const [saving, setSaving] = useState(false);
  const [section, setSection] = useState<'review' | 'appearance'>('review');
  const overrides = Object.entries(config.lspCommands).sort(([a], [b]) => a.localeCompare(b));

  const save = async () => {
    setSaving(true);
    await saveConfig({
      autoViewed: text
        .split('\n')
        .map((l) => l.trim())
        .filter(Boolean),
      contextLines: Math.min(10000, Math.max(0, Number.parseInt(context, 10) || 0)),
      followRefs,
      lineWrap,
    });
    setSaving(false);
    onClose();
  };

  // Escape closes, Enter saves unless the cursor is in the textarea, where it
  // inserts a line. Registered on window in the capture phase so the dialog
  // wins over the global keymap, which also captures Escape on document.
  const latest = useRef({ save, onClose, saving });
  latest.current = { save, onClose, saving };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        latest.current.onClose();
        return;
      }
      if (e.key === 'Enter' && !e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey) {
        if (
          (e.target as HTMLElement | null)?.closest('.appearance-panel') ||
          (e.target as HTMLElement | null)?.closest('textarea, button, select')
        )
          return;
        e.preventDefault();
        e.stopPropagation();
        if (!latest.current.saving) void latest.current.save();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  return (
    <Dialog open onOpenChange={onClose}>
      <DialogContent
        aria-label="Settings"
        className="[&_code]:rounded-sm [&_code]:bg-hover [&_code]:px-1 [&_code]:font-mono"
      >
        <div className="mb-4 flex items-center gap-2" aria-label="Settings sections">
          <Button
            variant={section === 'review' ? 'primary' : 'default'}
            aria-pressed={section === 'review'}
            onClick={() => setSection('review')}
          >
            Review
          </Button>
          <Button
            variant={section === 'appearance' ? 'primary' : 'default'}
            aria-pressed={section === 'appearance'}
            onClick={() => setSection('appearance')}
          >
            Appearance
          </Button>
        </div>
        {section === 'appearance' ? (
          <>
            <AppearancePanel />
            <Button onClick={onClose}>Done</Button>
          </>
        ) : (
          <>
            <h3 className="m-0 mb-2">Auto-viewed patterns</h3>
            <p className="m-0 mb-2 text-muted">
              Matching files start collapsed and viewed. One glob per line: <code>*.lock</code> matches a file name,{' '}
              <code>**/generated/**</code> a path.
            </p>
            <textarea
              className="min-h-24 w-full rounded-md border border-border bg-surface p-2 font-mono text-[0.75rem]"
              value={text}
              onChange={(e) => setText(e.target.value)}
              spellCheck={false}
              aria-label="Auto-viewed patterns"
              autoFocus
            />
            <h3 className="m-0 mt-[14px] mb-2">Context lines</h3>
            <p className="m-0 mb-2 text-muted">Unchanged lines around each change.</p>
            <input
              type="number"
              aria-label="Context lines"
              min={0}
              max={10000}
              value={context}
              onChange={(e) => setContext(e.target.value)}
              className="w-[100px]"
            />
            <h3 className="m-0 mt-[14px] mb-2">Long lines</h3>
            <label className="flex cursor-pointer items-center gap-1.5">
              <input type="checkbox" checked={lineWrap} onChange={(e) => setLineWrap(e.target.checked)} />
              Wrap long lines
              <span className="text-muted">
                (toggle with <code>zw</code>)
              </span>
            </label>
            <h3 className="m-0 mt-[14px] mb-2">Reload behavior</h3>
            <p className="m-0 mb-2 text-muted">Reload when a compared ref moves.</p>
            <div role="radiogroup" aria-label="Reload behavior" className="flex flex-col gap-1">
              {FOLLOW_REFS.map(([value, label]) => (
                <label key={value} className="flex cursor-pointer items-center gap-1.5">
                  <input
                    type="radio"
                    name="follow-refs"
                    value={value}
                    checked={followRefs === value}
                    onChange={() => setFollowRefs(value)}
                  />
                  {label}
                </label>
              ))}
            </div>
            <h3 className="m-0 mt-[14px] mb-2">Language servers</h3>
            <p className="m-0 mb-2 text-muted">
              Configure servers in Preferences → Code intelligence. Check status and logs in the toolbar.
            </p>
            {overrides.length > 0 && (
              <p className="m-0 mb-2 text-muted">
                Overrides:{' '}
                {overrides.map(([language, command], i) => (
                  <span key={language}>
                    {i > 0 && ', '}
                    {language} <code>{command || '(off)'}</code>
                  </span>
                ))}
              </p>
            )}
            <div className="mt-2.5 flex items-center gap-1.5">
              <span className="flex-1 text-[0.75rem] text-muted" title="Bundled review engine version">
                mdiff · review engine v{__DIFFLE_VERSION__}
              </span>
              <Button onClick={onClose}>Cancel</Button>
              <Button variant="primary" onClick={save} disabled={saving}>
                Save
              </Button>
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
