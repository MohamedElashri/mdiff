import { uiText } from '@/client/i18n/runtime.js';
import { useReviewStore } from '@/client/store.js';
import { useCommands, useCommandAction } from '@/client/commands/CommandProvider.js';
import { CommandTrigger } from '@/client/commands/CommandTrigger.js';
import { Button } from '@/client/ui/Button.js';
import { History, Keyboard, Monitor, Moon, PanelLeft, PanelRight, Settings, Sun } from 'lucide-react';
import { useEffect, useState, type ReactNode } from 'react';
import { repoName } from '@/client/model.js';

import { nextTheme, type ThemeChoice } from '@/client/theme.js';
import { panelOf, useActiveView } from '@/client/views/state.js';
import { ViewSwitch } from '@/client/views/ViewSwitch.js';
import { CommitStepper } from './CommitStepper.js';
import { GithubMenu } from './GithubMenu.js';
import { HistoryNav } from './HistoryNav.js';
import { LspIndicator } from './LspStatus.js';
import { ModePicker } from './ModePicker.js';
import { MovedNotice } from './MovedNotice.js';
import { SettingsDialog } from './SettingsDialog.js';

export function Header({
  desktop = false,
  hostingControl,
  compactToggle,
}: {
  desktop?: boolean;
  hostingControl?: ReactNode;
  compactToggle?: (pane: 'tree' | 'panel') => void;
}) {
  const useStore = useReviewStore();

  const snapshot = useStore((s) => s.snapshot);
  const lsp = useStore((s) => s.lsp);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const theme = useStore((s) => s.theme);
  const setTheme = useStore((s) => s.setTheme);
  const layout = useStore((s) => s.layout);
  const setLayout = useStore((s) => s.setLayout);
  const view = useActiveView();
  const panel = panelOf(layout, view.id);
  const registry = useCommands();
  useEffect(
    () =>
      registry?.register({
        'review.settings': { run: () => setSettingsOpen(true) },
        ...(!desktop ? { 'app.preferences': { run: () => setSettingsOpen(true) } } : {}),
      }),
    [registry, desktop],
  );
  const historyCommand = useCommandAction('app.history', () => {});
  const treeCommand = useCommandAction('review.tree', () =>
    compactToggle ? compactToggle('tree') : setLayout({ treeVisible: !layout.treeVisible }),
  );
  const panelCommand = useCommandAction('review.panel', () =>
    compactToggle ? compactToggle('panel') : useStore.getState().setPanel(view.id, { visible: !panel.visible }),
  );
  const themeCommand = useCommandAction('review.theme', () => setTheme(nextTheme(theme)));
  const helpCommand = useCommandAction('review.help', () => useStore.getState().setHelpOpen(true));
  const settingsCommand = useCommandAction('review.settings', () => setSettingsOpen(true));
  const changed = snapshot?.changed.length ?? 0;
  const adds = snapshot?.changed.reduce((n, f) => n + f.additions, 0) ?? 0;
  const dels = snapshot?.changed.reduce((n, f) => n + f.deletions, 0) ?? 0;

  return (
    <header className="review-toolbar col-span-full flex items-center gap-2 border-b border-b-border px-2.5 py-0">
      <Button
        variant="ghost"
        icon
        className={layout.treeVisible ? '' : 'opacity-45'}
        onClick={treeCommand.run}
        title={uiText('Toggle file tree{value0}', {
          value0: String(treeCommand.shortcut ? ` (${treeCommand.shortcut})` : ''),
        })}
      >
        <PanelLeft size="1rem" />
      </Button>
      <HistoryNav />
      {!desktop && (
        <Button variant="ghost" onClick={historyCommand.run} aria-label={uiText('History')}>
          <History size={15} />
          <span className="max-lg:hidden">{uiText('History')}</span>
        </Button>
      )}
      {!desktop && (
        <>
          <span className="inline-flex items-center gap-1.5 font-bold tracking-[0.02em] max-sm:hidden">
            <svg width="20" height="20" viewBox="0 0 32 32" aria-hidden="true">
              <path d="M5 25V9h8v10h6V9h8v16" fill="none" stroke="currentColor" strokeWidth="4" />
              <path d="M5 4h8M19 29h8" stroke="currentColor" strokeWidth="3" />
            </svg>{' '}
            <span className="max-lg:hidden">{uiText('mdiff')}</span>
          </span>
          <span className="truncate font-mono text-[0.75rem] text-muted max-lg:hidden" title={snapshot?.root}>
            {snapshot ? repoName(snapshot) : ''}
          </span>
        </>
      )}
      <ModePicker />
      <CommitStepper />
      {snapshot?.commit && (
        <span
          className="max-w-[24rem] min-w-0 shrink-[100] truncate text-[0.8125rem] max-md:hidden"
          title={snapshot.commit.message}
        >
          {snapshot.commit.message.split('\n', 1)[0]}
        </span>
      )}
      {hostingControl ?? <GithubMenu />}
      <MovedNotice />
      <span className="shrink-0 whitespace-nowrap text-muted max-md:hidden">
        {changed}
        {uiText(' files · ')}
        <span className="text-add">+{adds}</span> <span className="text-del">−{dels}</span>
      </span>
      <span className="flex-1" />
      {lsp.enabled && <LspIndicator lsp={lsp} />}
      {/* The switch shows in every view, so it holds its place; a view's own controls come and go beside it. */}
      {view.Controls && <view.Controls />}
      <ViewSwitch />
      {!desktop && <CommandTrigger />}
      <Button
        variant="ghost"
        icon
        onClick={themeCommand.run}
        title={uiText('Theme: {value0} (t)', { value0: String(theme) })}
      >
        <ThemeIcon choice={theme} />
      </Button>
      <Button variant="ghost" icon onClick={helpCommand.run} title={uiText('Keyboard shortcuts (?)')}>
        <Keyboard size="1rem" />
      </Button>
      <Button variant="ghost" onClick={settingsCommand.run} title={uiText('Settings')} aria-label={uiText('Settings')}>
        <Settings size="0.9375rem" /> <span className="max-xl:hidden">{uiText('Settings')}</span>
      </Button>
      <Button
        variant="ghost"
        icon
        className={panel.visible ? '' : 'opacity-45'}
        onClick={panelCommand.run}
        title={uiText('Toggle {value0}{value1}', {
          value0: String(view.panelLabel),
          value1: String(panelCommand.shortcut ? ` (${panelCommand.shortcut})` : ''),
        })}
      >
        <PanelRight size="1rem" />
      </Button>
      {settingsOpen && <SettingsDialog onClose={() => setSettingsOpen(false)} />}
    </header>
  );
}

function ThemeIcon({ choice }: { choice: ThemeChoice }) {
  if (choice === 'light') return <Sun size="1rem" />;
  if (choice === 'dark') return <Moon size="1rem" />;
  return <Monitor size="1rem" />;
}
