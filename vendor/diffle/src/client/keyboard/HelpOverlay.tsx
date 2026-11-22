import { uiText } from '@/client/i18n/runtime.js';
import { useRef } from 'react';
import { useCommands, useCommandVersion } from '@/client/commands/CommandProvider.js';
import { shortcutLabel } from '@/client/commands/shortcuts.js';
import { messages, translate } from '@/client/i18n/messages.js';
import { BookOpen } from 'lucide-react';
import { Dialog, DialogContent } from '@/client/ui/dialog.js';
import { Button } from '@/client/ui/Button.js';
import { useStore } from '@/client/store.js';
import { type HelpSection, useActiveView } from '@/client/views/state.js';

/**
 * The diff's shortcuts, one array per column. Columns are explicit because CSS multi-column overflows
 * into a clipped third column once the dialog height is capped.
 */
export const DIFF_HELP: HelpSection[][] = [
  [
    {
      title: uiText('Move'),
      rows: [
        ['j / k  or  ↓ / ↑', uiText('next / previous line')],
        ['{n}j / {n}k', uiText('n lines down / up')],
        ['Ctrl+d / Ctrl+u', uiText('half a page down / up')],
        ['Ctrl+o / Ctrl+i', uiText('jump back / forward in history')],
        ['←  or  ⌘/Ctrl+Shift+e  /  →', uiText('focus the file tree / back to the diff')],
        ['V, then j / k', uiText('extend a block selection')],
      ],
    },
    {
      title: uiText('Files and hunks'),
      rows: [
        ['J / K', uiText('next / previous file')],
        ['gg / G', uiText('first / last file')],
        ['{n}gg / {n}G', uiText('line n of the current file')],
        ['] / [   n / N', uiText('next / previous hunk (n / N follow matches while searching)')],
        ['v', uiText('toggle viewed on the current file')],
        ['gv', uiText('toggle viewed on the file above')],
        ['zo / zc', uiText('expand / collapse the current file (zo loads a large diff)')],
        ['zO / zC', uiText('open / close all files')],
        ['F', uiText('view the current file whole; Ctrl+o returns')],
      ],
    },
    {
      title: uiText('Comments'),
      rows: [
        ['c', uiText('comment on the current line or selection')],
        ['C', uiText('comment on the current file as a whole')],
        ['e', uiText('edit the newest message of the thread under the cursor')],
        ['dd', uiText('delete the thread under the cursor')],
        ['R', uiText('resolve / reopen the thread under the cursor')],
        ['yy or Y', uiText('copy all comments')],
        ['yp', uiText('copy the current file’s path')],
        ['yl', uiText('copy a GitHub link to the line or selection')],
      ],
    },
    {
      title: uiText('View'),
      rows: [
        ['zz', uiText('scroll the current line to eye level')],
        ['zt / zb', uiText('scroll the current line to the top / bottom')],
        ['s', uiText('toggle split / unified')],
        ['zw', uiText('toggle line wrapping')],
        ['zh / zl', uiText('scroll long lines left / right; hold z to repeat')],
        ['t', uiText('cycle theme')],
        ['⌘/Ctrl+b', uiText('toggle the file tree (inspector has its own shortcut)')],
        ['o', uiText('GitHub repository and pull request')],
        ['m, then 1–6', uiText('open the mode picker and choose an entry')],
        ['< / >', uiText('older / newer commit or range-diff pair')],
      ],
    },
  ],
  [
    {
      title: uiText('Search'),
      rows: [
        ['/', uiText('search the current file, then n / N between matches')],
        ['g/', uiText('search changed files (toggle: diff + context / full file)')],
        ['⌘/Ctrl+p  or  gf', uiText('search files by name')],
        ['w / b', uiText('focus the next / previous symbol on the line')],
        ['0 / $', uiText('focus the first / last symbol on the line')],
        ['* / #', uiText('next / previous occurrence of the focused word')],
      ],
    },
    {
      title: uiText('Code intelligence (language server)'),
      rows: [
        ['hover a symbol  or  gh', uiText('signature and docs tooltip (gh: focused word); Esc closes')],
        ['click a symbol', uiText('definition / type definition / references popover')],
        ['gd  or  ⌘/Ctrl+click', uiText('go to the definition, or its references from there')],
        ['gy', uiText('go to the definition of the hovered symbol’s type')],
        ['gA', uiText('list references; j / k, Enter to jump, n / N to step')],
        ['gs / gS', uiText('symbols in the current file / across the repository')],
      ],
    },
    {
      title: uiText('Change graph (language server)'),
      rows: [
        ['M', uiText('graph of changed symbols; ? there lists its keys')],
        ['right-click a folder', uiText('Show in graph: only the changes under it')],
      ],
    },
    {
      title: uiText('Iterations'),
      rows: [
        ['r', uiText('reload once the compared refs moved')],
        ['ii', uiText('latest iteration vs. the one before; again: the range')],
        ['ij / ik', uiText('span’s lower end one iteration older / newer')],
        ['iJ / iK', uiText('span’s higher end one iteration older / newer')],
        ['i, then 1–9', uiText('compare that iteration with the latest')],
      ],
    },
    {
      title: uiText('General'),
      rows: [
        ['?', uiText('toggle this help')],
        ['Esc', uiText('close composer, search, or menu; clear selection')],
      ],
    },
  ],
];

/** GitHub's mark (Octicons `mark-github`); Lucide ships no brand icons. */
function GithubMark() {
  return (
    <svg viewBox="0 0 16 16" width="1rem" height="1rem" fill="currentColor" aria-hidden="true">
      <path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z" />
    </svg>
  );
}

/** Icon-only link in the dialog header, the height of the Close button. */
const LINK =
  'inline-flex size-6 items-center justify-center rounded-md text-muted hover:bg-hover hover:text-foreground';

export function HelpOverlay() {
  const open = useStore((s) => s.helpOpen);
  const setOpen = useStore((s) => s.setHelpOpen);
  const view = useActiveView();
  const registry = useCommands();
  useCommandVersion(registry);
  const customize = useRef(false);
  const binding = (id: string) => shortcutLabel(registry?.shortcut(id), registry?.platform ?? 'other') || '—';
  const keyLabel = (key: string) =>
    !registry
      ? key
      : key
          .replace('Ctrl+F', binding('review.find'))
          .replace('Ctrl+o / Ctrl+i', `${binding('review.back')} / ${binding('review.forward')}`)
          .replace('⌘/Ctrl+Shift+e', binding('review.focusTree'))
          .replace('⌘/Ctrl+p', binding('review.files'))
          .replace('⌘/Ctrl+b', binding('review.tree'));

  return (
    <Dialog
      open={open}
      onOpenChange={setOpen}
      onOpenChangeComplete={(value) => {
        if (!value && customize.current) {
          customize.current = false;
          registry?.execute('app.shortcuts');
        }
      }}
    >
      {/* Nothing here takes input, and the first tabbable (the Docs link) would open ringed. */}
      <DialogContent
        initialFocus={false}
        aria-label={uiText('Keyboard shortcuts')}
        className="flex max-h-[calc(100vh_-_2rem)] w-max max-w-[96vw] flex-col overflow-hidden p-0"
      >
        <div className="flex items-center justify-between border-b border-b-border px-4.5 pt-3 pb-2.5">
          <h3 className="m-0 text-[0.9375rem]">
            {uiText('Keyboard shortcuts')}
            <span className="font-normal text-muted"> · {view.label}</span>
          </h3>
          <div className="flex items-center">
            {registry && (
              <Button
                onClick={() => {
                  customize.current = true;
                  setOpen(false);
                }}
              >
                {translate(messages.customize)}
              </Button>
            )}
            <a
              className={LINK}
              href="https://diffle.app"
              target="_blank"
              rel="noreferrer"
              aria-label={uiText('Review engine documentation')}
              title={uiText('Review engine documentation')}
            >
              <BookOpen size="1rem" aria-hidden="true" />
            </a>
            <a
              className={LINK}
              href="https://github.com/moritzwilksch/diffle"
              target="_blank"
              rel="noreferrer"
              aria-label={uiText('Upstream Diffle source')}
              title={uiText('Upstream Diffle source')}
            >
              <GithubMark />
            </a>
            <Button className="ml-2" onClick={() => setOpen(false)}>
              {uiText('Close')}
            </Button>
          </div>
        </div>
        <div className="grid grid-cols-[auto_auto] items-start gap-x-10 overflow-auto px-4.5 pt-2.5 pb-3.5 text-[0.75rem] whitespace-nowrap">
          {registry && (
            <section className="col-span-full mb-3 grid grid-cols-[max-content_1fr] gap-x-3">
              <h4 className="col-span-full mb-1 font-semibold text-muted">{translate(messages.modifiers)}</h4>
              {registry
                .definitions()
                .filter((command) => registry.handler(command.id) && registry.shortcut(command.id))
                .map((command) => (
                  <div key={command.id} className="col-span-full grid grid-cols-subgrid leading-[1.7]">
                    <kbd>{binding(command.id)}</kbd>
                    <span>{translate(command.label)}</span>
                  </div>
                ))}
            </section>
          )}
          {view.help.map((sections, col) => (
            <div key={col} className="grid grid-cols-[max-content_1fr] content-start gap-x-3">
              {sections.map((section) => (
                <section className="col-span-full mb-2.5 grid grid-cols-subgrid last:mb-0" key={section.title}>
                  <h4 className="col-span-full m-0 mb-0.5 text-[0.75rem] font-semibold text-muted">{section.title}</h4>
                  {section.rows.map(([k, a]) => (
                    <div key={k} className="col-span-full grid grid-cols-subgrid items-baseline leading-[1.7]">
                      <span className="inline-flex gap-1.5">
                        {keyLabel(k)
                          .split(/\s{2,}/)
                          .map((part, i) => (
                            <kbd key={i}>{part}</kbd>
                          ))}
                      </span>
                      <span>{a}</span>
                    </div>
                  ))}
                </section>
              ))}
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
