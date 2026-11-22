import {
  createContext,
  lazy,
  Suspense,
  useContext,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react';
import { CommandRegistry } from './registry.js';
import { keyboardTarget, shortcutLabel } from './shortcuts.js';
import { coreMessages as messages, translate } from '@/client/i18n/runtime.js';

const Palette = lazy(() => import('./CommandPalette.js'));
const Shortcuts = lazy(() => import('./ShortcutSettings.js').then((module) => ({ default: module.ShortcutDialog })));
const Context = createContext<CommandRegistry | null>(null);
const noopSubscribe = () => () => {};
const zero = () => 0;

export function useCommands(): CommandRegistry | null {
  return useContext(Context);
}
export function useCommandVersion(registry: CommandRegistry | null): number {
  return useSyncExternalStore(registry?.subscribe ?? noopSubscribe, registry?.version ?? zero, zero);
}
export function useCommandAction(id: string, fallback: () => unknown) {
  const registry = useCommands();
  useCommandVersion(registry);
  return {
    run: () => (registry?.handler(id) ? registry.execute(id) : fallback()),
    shortcut: registry ? shortcutLabel(registry.shortcut(id), registry.platform) : '',
  };
}

export function CommandProvider({ children }: { children: ReactNode }) {
  const [registry] = useState(() => {
    let storage: Storage | undefined;
    try {
      storage = window.localStorage;
    } catch {
      /* Preferences can be session-only. */
    }
    return new CommandRegistry(/Mac|iPhone|iPad/.test(navigator.platform) ? 'mac' : 'other', storage);
  });
  const [surface, setSurface] = useState<'palette' | 'shortcuts' | null>(null);
  const [loaded, setLoaded] = useState({ palette: false, shortcuts: false });
  const [error, setError] = useState('');
  const previousFocus = useRef<HTMLElement | null>(null);
  const open = (kind: 'palette' | 'shortcuts') => {
    const active = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (active !== document.body || !previousFocus.current?.isConnected) previousFocus.current = active;
    setLoaded((value) => ({ ...value, [kind]: true }));
    setSurface(kind);
    setError('');
  };
  useEffect(
    () =>
      registry.register({
        'app.palette': { run: () => open('palette') },
        'app.shortcuts': { run: () => open('shortcuts') },
      }),
    [registry],
  );
  useEffect(() => {
    const handle = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return;
      const target = keyboardTarget(event);
      if (target.blocked) return;
      const command = registry.match(event);
      if (!command || (target.input && !command.allowInInput)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (!event.repeat) {
        const reason = registry.reason(command.id);
        if (reason) {
          setError(reason);
          return;
        }
        try {
          void Promise.resolve(registry.execute(command.id)).catch(() => setError(translate(messages.actionFailed)));
        } catch {
          setError(translate(messages.actionFailed));
        }
      }
    };
    window.addEventListener('keydown', handle, true);
    return () => window.removeEventListener('keydown', handle, true);
  }, [registry]);
  return (
    <Context.Provider value={registry}>
      {children}
      <Suspense fallback={null}>
        {loaded.palette && (
          <Palette
            registry={registry}
            open={surface === 'palette'}
            close={() => setSurface(null)}
            previousFocus={previousFocus}
            reportError={setError}
          />
        )}
        {loaded.shortcuts && (
          <Shortcuts open={surface === 'shortcuts'} close={() => setSurface(null)} previousFocus={previousFocus} />
        )}
      </Suspense>
      {error && (
        <div role="alert" className="command-error">
          {error}
          <button onClick={() => setError('')}>{translate(messages.dismiss)}</button>
        </div>
      )}
    </Context.Provider>
  );
}
