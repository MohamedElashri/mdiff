import { useEffect, useId, useRef, useState, type RefObject } from 'react';
import {
  Command,
  File,
  GitBranch,
  GitCommitHorizontal,
  History,
  GitPullRequest,
  GitFork,
  Search,
  X,
} from 'lucide-react';
import { Dialog, DialogContent } from '@/client/ui/dialog.js';
import { messages, translate } from '@/client/i18n/messages.js';
import { useCommandVersion } from './CommandProvider.js';
import { rankMatches } from './search.js';
import { shortcutLabel } from './shortcuts.js';
import type { CommandRegistry, PaletteEntry, SearchGroup } from './registry.js';

const GROUPS: readonly ['all' | SearchGroup, string][] = [
  ['all', translate(messages.groupsAll)],
  ['commands', translate(messages.groupsCommands)],
  ['files', translate(messages.groupsFiles)],
  ['refs', translate(messages.groupsRefs)],
  ['commits', translate(messages.groupsCommits)],
  ['recent', translate(messages.groupsRecent)],
  ['hosting', translate(messages.groupsHosting)],
  ['worktrees', translate(messages.groupsWorktrees)],
];
const ICONS = {
  commands: Command,
  files: File,
  refs: GitBranch,
  commits: GitCommitHorizontal,
  recent: History,
  hosting: GitPullRequest,
  worktrees: GitFork,
};
const LIMIT = 60;

export default function CommandPalette({
  registry,
  open,
  close,
  previousFocus,
  reportError,
}: {
  registry: CommandRegistry;
  open: boolean;
  close(): void;
  previousFocus: RefObject<HTMLElement | null>;
  reportError(message: string): void;
}) {
  const version = useCommandVersion(registry);
  const sourceVersion = registry.sourceVersion();
  const [query, setQuery] = useState('');
  const [group, setGroup] = useState<'all' | SearchGroup>('all');
  const [items, setItems] = useState<PaletteEntry[]>([]);
  const [selected, setSelected] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const input = useRef<HTMLInputElement>(null);
  const pending = useRef<(() => unknown) | null>(null);
  const sources = useRef<ReturnType<CommandRegistry['openSources']>>([]);
  const list = useId();

  useEffect(() => {
    if (open) {
      setQuery('');
      setGroup('all');
      setError('');
    } else {
      setItems([]);
      setLoading(false);
    }
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const opened = registry.openSources();
    sources.current = opened;
    return () => {
      opened.forEach((source) => source.dispose?.());
      if (sources.current === opened) sources.current = [];
    };
  }, [registry, open, sourceVersion]);
  useEffect(() => {
    if (!open) return;
    const abort = new AbortController();
    setItems([]);
    setSelected(0);
    setError('');
    setLoading(true);
    const timer = setTimeout(
      () => {
        const local =
          group === 'all' || group === 'commands'
            ? rankMatches(registry.entries(), query, (entry) => entry.label, group === 'commands' ? LIMIT : 20)
            : [];
        setItems(local);
        const active = sources.current.filter((source) =>
          group === 'all' ? !!query.trim() || source.group === 'recent' : source.group === group,
        );
        void Promise.allSettled(
          active.map((source) => Promise.resolve().then(() => source.search(query, abort.signal))),
        ).then((results) => {
          if (abort.signal.aborted) return;
          const entries = [
            ...local,
            ...results.flatMap((result) => (result.status === 'fulfilled' ? result.value.slice(0, 20) : [])),
          ];
          setItems(rankMatches(entries, query, (entry) => `${entry.label} ${entry.detail ?? ''}`, LIMIT));
          setSelected(0);
          setLoading(false);
          if (results.some((result) => result.status === 'rejected')) setError(translate(messages.failed));
        });
      },
      query ? 120 : 0,
    );
    return () => {
      clearTimeout(timer);
      abort.abort();
    };
  }, [registry, open, version, group, query]);
  useEffect(() => {
    document.getElementById(`${list}-${selected}`)?.scrollIntoView?.({ block: 'nearest' });
  }, [selected, list]);

  const choose = (entry: PaletteEntry) => {
    if (entry.reason) {
      setError(entry.reason);
      return;
    }
    pending.current = entry.run;
    close();
  };
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!value) close();
      }}
      onOpenChangeComplete={(value) => {
        if (value) return;
        if (!pending.current) {
          // Base UI otherwise focuses the first tabbable child of a negative-tabindex
          // review pane. Restore that pane itself after its focus trap has closed.
          if (previousFocus.current?.isConnected) previousFocus.current.focus({ preventScroll: true });
          return;
        }
        const run = pending.current;
        pending.current = null;
        // The focus trap is gone before a command focuses a file/menu or opens another dialog.
        try {
          void Promise.resolve(run())
            .then((result) => {
              if (result === false) reportError(translate(messages.changed));
            })
            .catch(() => reportError(translate(messages.actionFailed)));
        } catch {
          reportError(translate(messages.actionFailed));
        }
      }}
    >
      <DialogContent
        className="command-palette w-160 overflow-hidden p-0"
        aria-label={translate(messages.palette)}
        initialFocus={input}
        finalFocus={false}
      >
        <div className="command-search">
          <Search size={18} aria-hidden="true" />
          <input
            ref={input}
            role="combobox"
            aria-label={translate(messages.palette)}
            aria-expanded="true"
            aria-controls={list}
            aria-activedescendant={items[selected] ? `${list}-${selected}` : undefined}
            autoComplete="off"
            spellCheck={false}
            maxLength={128}
            value={query}
            placeholder={translate(messages.search)}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.nativeEvent.isComposing) return;
              if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
                event.preventDefault();
                setSelected((index) =>
                  items.length ? (index + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length : 0,
                );
              }
              if (event.key === 'Enter') {
                event.preventDefault();
                const entry = items[selected];
                if (entry) choose(entry);
              }
            }}
          />
          <button aria-label={translate(messages.close)} onClick={close}>
            <X size={16} />
          </button>
        </div>
        <nav className="command-groups" aria-label={translate(messages.categories)}>
          {GROUPS.filter(([value]) =>
            value === 'hosting' || value === 'worktrees' ? registry.hasSourceGroup(value) : true,
          ).map(([id, label]) => (
            <button
              key={id}
              aria-pressed={group === id}
              onClick={() => {
                setGroup(id);
                input.current?.focus();
              }}
            >
              {label}
            </button>
          ))}
        </nav>
        <div
          id={list}
          role="listbox"
          aria-label={translate(messages.resultList)}
          aria-busy={loading}
          className="command-results"
        >
          {items.map((entry, index) => {
            const Icon = ICONS[entry.group];
            return (
              <div
                id={`${list}-${index}`}
                key={entry.id}
                role="option"
                aria-selected={selected === index}
                aria-disabled={!!entry.reason}
                className="command-result"
                onPointerMove={() => setSelected(index)}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => choose(entry)}
              >
                <Icon size={16} aria-hidden="true" />
                <span>
                  <strong>{entry.label}</strong>
                  {(entry.reason || entry.detail) && <small>{entry.reason || entry.detail}</small>}
                </span>
                {entry.shortcut && <kbd>{shortcutLabel(entry.shortcut, registry.platform)}</kbd>}
              </div>
            );
          })}
          {!items.length && (
            <p className="command-empty" role="status">
              {translate(loading ? messages.loading : messages.empty)}
            </p>
          )}
        </div>
        {error && (
          <p className="command-status" role="status">
            {error}
          </p>
        )}
        <footer>
          <span>{translate(messages.results, { count: items.length })}</span>
          <span>
            <kbd>↑</kbd>
            <kbd>↓</kbd> {translate(messages.select)} <kbd>Enter</kbd> {translate(messages.run)} <kbd>Esc</kbd>{' '}
            {translate(messages.dismiss)}
          </span>
        </footer>
      </DialogContent>
    </Dialog>
  );
}
