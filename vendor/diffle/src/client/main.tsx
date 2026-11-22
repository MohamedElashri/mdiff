import { formatLocale } from './i18n/locale.js';
import DiffsWorker from '@pierre/diffs/worker/worker.js?worker';
import { WorkerPoolContextProvider } from '@pierre/diffs/react';
import { StrictMode } from 'react';
import { CommandProvider } from './commands/CommandProvider.js';
import { createRoot } from 'react-dom/client';
import { App } from './App.js';
import { LegacyDraftRecovery } from './LegacyDraftRecovery.js';
import './styles.css';
import { applyTheme, CODE_THEMES, HIGHLIGHTER, readTheme } from './theme.js';

// Apply the saved theme before first paint.
applyTheme(readTheme());
document.documentElement.lang = formatLocale;
document.documentElement.dir = 'ltr';

performance.mark('diffle:boot');

const poolSize = Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1));

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <WorkerPoolContextProvider
      poolOptions={{ poolSize, workerFactory: () => new DiffsWorker() }}
      // useTokenTransformer: token spans get hit-test data so symbol clicks work; the pool's options win over the viewer's.
      highlighterOptions={{ preferredHighlighter: HIGHLIGHTER, useTokenTransformer: true, theme: CODE_THEMES }}
    >
      <CommandProvider>
        <div className="flex h-full min-h-0 flex-col">
          <LegacyDraftRecovery />
          <div className="min-h-0 flex-1">
            <App />
          </div>
        </div>
      </CommandProvider>
    </WorkerPoolContextProvider>
  </StrictMode>,
);
