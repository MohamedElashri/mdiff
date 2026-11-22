import { uiText } from '@/client/i18n/runtime.js';
import { twMerge } from 'tailwind-merge';
import { Button, TOGGLE_ON } from '@/client/ui/Button.js';
import { Link2, Search, WholeWord, X } from 'lucide-react';
import { useEffect, useRef } from 'react';
import { useStore } from '@/client/store.js';

/** Content search (/ in the current file, g/ across changed files), the references of a symbol (gA), or a word's occurrences (* / #). Enter runs a search; n / N step through matches. */
export function SearchBar({ path }: { path?: string }) {
  const visible = useStore((s) => {
    const local = s.search.kind === 'text' && s.search.scope === 'file';
    return s.search.open && (local ? s.search.path === path : path === undefined);
  });
  return visible ? <SearchForm /> : null;
}

function SearchForm() {
  const search = useStore((s) => s.search);
  const runSearch = useStore((s) => s.runSearch);
  const closeSearch = useStore((s) => s.closeSearch);
  const setSearchOptions = useStore((s) => s.setSearchOptions);
  const setSearchInput = useStore((s) => s.setSearchInput);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    // Virtualization can remount this input during a wheel gesture; focus must not move the viewport.
    if (!search.editing) return;
    ref.current?.focus({ preventScroll: true });
    if (search.selected) ref.current?.select();
  }, [search.editing, search.focusNonce, search.selected]);

  const n = search.matches.length;
  if (search.kind !== 'text') {
    return (
      <div className="flex items-center gap-2 border-b border-b-border bg-surface px-2.5 py-1.5">
        {search.kind === 'references' ? <Link2 size="0.875rem" /> : <WholeWord size="0.875rem" />}
        <span className="flex-1 font-mono text-[0.75rem] leading-[normal] text-muted [&_code]:text-foreground">
          {search.kind === 'references'
            ? uiText('references to')
            : search.direction === 1
              ? uiText('occurrences of')
              : uiText('occurrences (upwards) of')}{' '}
          <code>{search.query}</code>
        </span>
        <span className="min-w-17.5 text-right font-mono text-[0.75rem] leading-[normal] text-muted">
          {search.loading
            ? uiText('searching…')
            : n
              ? `${search.index + 1} / ${n}${search.truncated ? '+' : ''}`
              : uiText('none')}
        </span>
        <Button type="button" variant="ghost" icon onClick={closeSearch} title={uiText('Close (Esc)')}>
          <X size="0.875rem" />
        </Button>
      </div>
    );
  }
  const fullFile = search.content[search.scope] === 'full';
  return (
    <form
      className="flex items-center gap-2 border-b border-b-border bg-surface px-2.5 py-1.5"
      onClick={(e) => e.stopPropagation()}
      onSubmit={(e) => {
        e.preventDefault();
        void runSearch(search.input).then(() => {
          ref.current?.blur();
          document.querySelector<HTMLElement>('main[tabindex]')?.focus({ preventScroll: true });
        });
      }}
    >
      <Search size="0.875rem" />
      <input
        data-content-search=""
        className="min-w-0 flex-1 rounded-md border border-border bg-canvas px-2 py-1 font-mono text-[0.75rem]"
        ref={ref}
        value={search.input}
        onFocus={() => {
          if (!search.editing) setSearchInput(search.input);
        }}
        onChange={(e) => setSearchInput(e.target.value)}
        placeholder={
          search.scope === 'file'
            ? uiText('Search this file… (Enter, then n / N)')
            : uiText('Search all files… (Enter, then n / N)')
        }
        spellCheck={false}
      />
      <Button
        type="button"
        variant="ghost"
        className={twMerge(
          `rounded-sm border border-transparent px-1.5 py-[2px] font-mono text-[0.6875rem] leading-[normal] text-muted ${search.ignoreCase ? '' : TOGGLE_ON}`,
        )}
        aria-pressed={!search.ignoreCase}
        onClick={() => setSearchOptions({ ignoreCase: !search.ignoreCase })}
        title={
          search.ignoreCase
            ? uiText('Ignoring case; click to match case')
            : uiText('Matching case; click to ignore case')
        }
      >
        {uiText('Aa')}
      </Button>
      <Button
        type="button"
        variant="ghost"
        className={twMerge(
          `rounded-sm border border-transparent px-1.5 py-[2px] font-mono text-[0.6875rem] leading-[normal] text-muted ${search.regex ? TOGGLE_ON : ''}`,
        )}
        aria-pressed={search.regex}
        onClick={() => setSearchOptions({ regex: !search.regex })}
        title={
          search.regex ? uiText('Extended regex; click for plain text') : uiText('Plain text; click for extended regex')
        }
      >
        .*
      </Button>
      <select
        aria-label={uiText('Search content')}
        className="max-w-28 min-w-0 rounded-sm border border-border bg-canvas text-xs"
        value={search.content[search.scope]}
        onChange={(e) =>
          setSearchOptions({
            content: e.target.value as typeof search.content.file,
          })
        }
      >
        <option value="diff">{uiText('Diff + context')}</option>
        <option value="added">{uiText('Added')}</option>
        <option value="removed">{uiText('Removed')}</option>
        <option value="both">{uiText('Both sides')}</option>
        <option value="full">{uiText('Full files')}</option>
      </select>
      <span className="min-w-17.5 text-right font-mono text-[0.75rem] leading-[normal] text-muted">
        {search.loading
          ? uiText('searching…')
          : n === 0 && search.query
            ? search.scope === 'file'
              ? uiText('none in file')
              : fullFile
                ? uiText('no matches')
                : uiText('none in diff')
            : n
              ? `${search.index + 1} / ${n}${search.truncated ? '+' : ''}`
              : ''}
      </span>
      <Button type="button" variant="ghost" icon onClick={closeSearch} title={uiText('Close (Esc)')}>
        <X size="0.875rem" />
      </Button>
    </form>
  );
}
