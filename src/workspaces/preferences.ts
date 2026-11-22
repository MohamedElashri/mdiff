import { useEffect, useRef } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  getAppearance,
  setAppearance,
  normalize,
  subscribeAppearance,
  type Appearance,
} from "../../vendor/diffle/src/client/appearance/preferences";
import {
  getTheme,
  storeTheme,
  applyTheme,
  subscribeTheme,
  type ThemeChoice,
} from "../../vendor/diffle/src/client/theme";
import type {
  CommandRegistry,
  ShortcutOverrides,
} from "../../vendor/diffle/src/client/commands/registry";
interface Preferences {
  version: 1;
  revision: number;
  appearance: Appearance;
  shortcuts: ShortcutOverrides;
  theme: ThemeChoice;
}
export function useShellPreferences(
  registry: CommandRegistry | null,
  onError: (error: string) => void,
) {
  const flush = useRef<() => Promise<void>>(async () => {});
  const reload = useRef<() => Promise<void>>(async () => {});
  const error = useRef(onError);
  error.current = onError;
  useEffect(() => {
    if (!isTauri() || !registry) return;
    let live = true,
      applying = false,
      ready = false,
      timer: ReturnType<typeof setTimeout> | null = null,
      stop: (() => void) | undefined;
    let revision = 0,
      saved = "",
      pending = Promise.resolve();
    const snapshot = (): Preferences => ({
      version: 1,
      revision,
      appearance: getAppearance(),
      shortcuts: registry.shortcutOverrides(),
      theme: getTheme(),
    });
    const content = (p: Preferences) =>
      JSON.stringify([p.appearance, p.shortcuts, p.theme]);
    const initial = content(snapshot());
    const persist = async () => {
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      if (!ready) return;
      const value = snapshot();
      if (content(value) === saved) return;
      const run = pending.then(async () => {
        if (content(value) === saved) return;
        const next = await invoke<Preferences>("save_shell_preferences", {
          revision,
          preferences: value,
        });
        revision = next.revision;
        saved = content(value);
      });
      pending = run.then(
        () => {},
        () => {},
      );
      await run;
    };
    flush.current = async () => {
      await persist();
      await pending;
    };
    const changed = () => {
      if (applying || !ready || content(snapshot()) === saved) return;
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        void persist().catch((e) =>
          error.current(`Desktop preferences could not be saved: ${String(e)}`),
        );
      }, 200);
    };
    const hydrate = (value: Preferences) => {
      if (!live || value.version !== 1 || value.revision < revision) return;
      revision = value.revision;
      applying = true;
      try {
        setAppearance(normalize(value.appearance));
        registry.replaceShortcuts(value.shortcuts);
        storeTheme(value.theme);
        applyTheme(value.theme);
        saved = content(snapshot());
      } finally {
        applying = false;
      }
      ready = true;
    };
    reload.current = async () => {
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      await pending;
      hydrate(
        await invoke<Preferences>("shell_preferences", { legacy: snapshot() }),
      );
    };
    void invoke<Preferences>("shell_preferences", { legacy: snapshot() })
      .then((value) => {
        if (!live) return;
        if (content(snapshot()) === initial) hydrate(value);
        else {
          revision = value.revision;
          saved = content(value);
          ready = true;
          changed();
        }
      })
      .catch((e) => error.current(String(e)));
    const appearance = subscribeAppearance(changed),
      shortcuts = registry.subscribe(changed),
      theme = subscribeTheme(changed);
    void listen<number>("shell-preferences", (event) => {
      if (!ready || event.payload <= revision) return;
      // A local queued edit is explicit user intent; do not silently replace it with another writer.
      if (content(snapshot()) !== saved) return;
      void invoke<Preferences>("shell_preferences", { legacy: snapshot() })
        .then(hydrate)
        .catch((e) => error.current(String(e)));
    })
      .then((unlisten) => {
        if (live) stop = unlisten;
        else unlisten();
      })
      .catch((e) => error.current(String(e)));
    return () => {
      live = false;
      appearance();
      shortcuts();
      theme();
      stop?.();
      if (timer) clearTimeout(timer);
      flush.current = async () => {};
    };
  }, [registry]);
  return { flush, reload };
}
