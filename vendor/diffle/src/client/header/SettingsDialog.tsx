import { uiText } from '@/client/i18n/runtime.js';
import { AppearancePanel } from '@/client/appearance/AppearancePanel.js';
import { Dialog, DialogContent } from '@/client/ui/dialog.js';
import { Button } from '@/client/ui/Button.js';
import { useEffect, useRef, useState } from 'react';
import { useReviewStore } from '@/client/store.js';
import type { UserConfig, FollowRefs } from '@/shared/protocol.js';

const FOLLOW_REFS: [FollowRefs, string][] = [
  ['on', uiText('Always')],
  ['auto', uiText('Auto (not on branches)')],
  ['off', uiText('Never')],
];

type ReviewFields = Partial<Pick<UserConfig, 'autoViewed' | 'contextLines' | 'followRefs' | 'lineWrap'>>;
export function ReviewPreferences(props: { onDirty?: (dirty: boolean) => void; onSaved?: () => void }) {
  const useStore = useReviewStore();
  const config = useStore((s) => s.config);
  const saveConfig = useStore((s) => s.saveConfig);
  return <ReviewPreferencesForm config={config} saveConfig={saveConfig} {...props} />;
}
export function ReviewPreferencesForm({
  config,
  saveConfig,
  onDirty,
  onSaved,
  onReload,
  desktop = false,
}: {
  config: UserConfig;
  saveConfig: (patch: ReviewFields) => Promise<boolean>;
  onDirty?: (dirty: boolean) => void;
  onSaved?: () => void;
  onReload?: () => Promise<void>;
  desktop?: boolean;
}) {
  const [text, setText] = useState(config.autoViewed.join('\n'));
  const [context, setContext] = useState(String(config.contextLines));
  const [followRefs, setFollowRefs] = useState(config.followRefs);
  const [lineWrap, setLineWrap] = useState(config.lineWrap);
  const [saving, setSaving] = useState(false);
  const [reloading, setReloading] = useState(false);
  const [reloadVersion, setReloadVersion] = useState(0);
  const busy = saving || reloading;
  const [error, setError] = useState('');
  const [saved, setSaved] = useState(false);
  const edited = useRef(false);
  const dirty =
    edited.current &&
    (text !== config.autoViewed.join('\n') ||
      context !== String(config.contextLines) ||
      followRefs !== config.followRefs ||
      lineWrap !== config.lineWrap);
  useEffect(() => {
    onDirty?.(dirty);
  }, [dirty, onDirty]);
  useEffect(() => {
    if (!edited.current) {
      setText(config.autoViewed.join('\n'));
      setContext(String(config.contextLines));
      setFollowRefs(config.followRefs);
      setLineWrap(config.lineWrap);
    }
  }, [config, reloadVersion]);
  useEffect(() => {
    if (!dirty) edited.current = false;
  }, [dirty]);
  const touch = () => {
    edited.current = true;
    setSaved(false);
  };
  const overrides = Object.entries(config.lspCommands).sort(([a], [b]) => a.localeCompare(b));

  const save = async () => {
    if (!Number.isInteger(Number(context)) || context.trim() === '' || Number(context) < 0 || Number(context) > 10000) {
      setError(uiText('Enter a whole number from 0 to 10000 for context lines.'));
      return;
    }
    setSaving(true);
    setError('');
    setSaved(false);
    let ok = false;
    try {
      ok = await saveConfig({
        autoViewed: text
          .split('\n')
          .map((l) => l.trim())
          .filter(Boolean),
        contextLines: Number(context),
        followRefs,
        lineWrap,
      });
    } catch (error) {
      setError(String(error));
    } finally {
      setSaving(false);
    }
    if (ok) {
      setText(
        text
          .split('\n')
          .map((l) => l.trim())
          .filter(Boolean)
          .join('\n'),
      );
      setContext(String(Number(context)));
      setSaved(true);
      onSaved?.();
    } else
      setError(uiText('Review preferences were not saved. Your edits are still here; retry after checking the error.'));
  };

  return (
    <section data-review-preferences aria-label={uiText('Review preferences')}>
      <p className="text-muted">
        {uiText('Review defaults are shared across repositories. Changes apply to open reviews.')}
      </p>
      <div data-preference-actions className="mb-4 flex flex-wrap gap-2">
        <Button variant="primary" onClick={() => void save()} disabled={busy || !dirty}>
          {saving ? uiText('Saving…') : uiText('Save changes')}
        </Button>
        <Button
          disabled={busy || !dirty}
          onClick={() => {
            edited.current = false;
            setText(config.autoViewed.join('\n'));
            setContext(String(config.contextLines));
            setFollowRefs(config.followRefs);
            setLineWrap(config.lineWrap);
            setError('');
            setSaved(false);
          }}
        >
          {uiText('Discard changes')}
        </Button>
        {onReload && (
          <Button
            disabled={busy}
            onClick={async () => {
              setReloading(true);
              setError('');
              try {
                await onReload();
                edited.current = false;
                setReloadVersion((version) => version + 1);
                setSaved(false);
              } catch (e) {
                setError(String(e));
              } finally {
                setReloading(false);
              }
            }}
          >
            {reloading
              ? uiText('Reloading…')
              : dirty
                ? uiText('Discard and reload')
                : uiText('Reload saved preferences')}
          </Button>
        )}
      </div>
      <h3 className="m-0 mb-2">{uiText('Auto-viewed patterns')}</h3>
      <p className="m-0 mb-2 text-muted">
        {uiText('Matching files start collapsed and viewed. One glob per line: ')}
        <code>*.lock</code>
        {uiText(' matches a file name,')} <code>**/generated/**</code>
        {uiText(' a path.')}
      </p>
      <textarea
        className="min-h-24 w-full rounded-md border border-border bg-surface p-2 font-mono text-[0.75rem]"
        value={text}
        disabled={busy}
        onChange={(e) => {
          touch();
          setText(e.target.value);
        }}
        spellCheck={false}
        aria-label={uiText('Auto-viewed patterns')}
        autoFocus={!desktop}
      />
      <h3 className="m-0 mt-[14px] mb-2">{uiText('Context lines')}</h3>
      <p className="m-0 mb-2 text-muted">{uiText('Unchanged lines around each change.')}</p>
      <input
        type="number"
        aria-label={uiText('Context lines')}
        min={0}
        max={10000}
        value={context}
        disabled={busy}
        onChange={(e) => {
          touch();
          setContext(e.target.value);
        }}
        className="w-[100px]"
      />
      <h3 className="m-0 mt-[14px] mb-2">{uiText('Long lines')}</h3>
      <label className="flex cursor-pointer items-center gap-1.5">
        <input
          type="checkbox"
          checked={lineWrap}
          disabled={busy}
          onChange={(e) => {
            touch();
            setLineWrap(e.target.checked);
          }}
        />
        {uiText('Wrap long lines')}
        <span className="text-muted">
          {uiText('(toggle with ')}
          <code>zw</code>)
        </span>
      </label>
      <h3 className="m-0 mt-[14px] mb-2">{uiText('Reload behavior')}</h3>
      <p className="m-0 mb-2 text-muted">{uiText('Reload when a compared ref moves.')}</p>
      <div role="radiogroup" aria-label={uiText('Reload behavior')} className="flex flex-col gap-1">
        {FOLLOW_REFS.map(([value, label]) => (
          <label key={value} className="flex cursor-pointer items-center gap-1.5">
            <input
              type="radio"
              name="follow-refs"
              value={value}
              checked={followRefs === value}
              disabled={busy}
              onChange={() => {
                touch();
                setFollowRefs(value);
              }}
            />
            {label}
          </label>
        ))}
      </div>
      <h3 className="m-0 mt-[14px] mb-2">{uiText('Language servers')}</h3>
      <p className="m-0 mb-2 text-muted">
        {desktop
          ? uiText('Configure servers in Preferences → Code intelligence. Check status and logs in the toolbar.')
          : uiText('Configure language servers with the diffle CLI. Check status and logs in the toolbar.')}
      </p>
      {overrides.length > 0 && (
        <p className="m-0 mb-2 text-muted">
          {uiText('Overrides:')}{' '}
          {overrides.map(([language, command], i) => (
            <span key={language}>
              {i > 0 && ', '}
              {language} <code>{command || '(off)'}</code>
            </span>
          ))}
        </p>
      )}
      {error && (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      {saved && <p role="status">{uiText('Review preferences saved.')}</p>}
    </section>
  );
}
export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const [section, setSection] = useState<'review' | 'appearance'>('review');
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState('');
  const close = () => {
    if (dirty) {
      setSection('review');
      setError(uiText('Save or discard changes before closing.'));
    } else onClose();
  };
  return (
    <Dialog open onOpenChange={(open) => !open && close()}>
      <DialogContent aria-label={uiText('Preferences')}>
        <h2>{uiText('Preferences')}</h2>
        <div className="mb-4 flex gap-2">
          <Button aria-pressed={section === 'review'} onClick={() => setSection('review')}>
            {uiText('Review')}
          </Button>
          <Button aria-pressed={section === 'appearance'} onClick={() => setSection('appearance')}>
            {uiText('Appearance')}
          </Button>
        </div>
        {error && dirty && <p role="alert">{error}</p>}
        <div hidden={section !== 'review'}>
          <ReviewPreferences onDirty={setDirty} />
        </div>
        <div hidden={section !== 'appearance'}>
          <AppearancePanel />
        </div>
        <Button onClick={close}>{uiText('Done')}</Button>
      </DialogContent>
    </Dialog>
  );
}
