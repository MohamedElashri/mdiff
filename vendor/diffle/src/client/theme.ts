import { getAppearance, subscribeAppearance } from './appearance/preferences.js';
import { applyPalette } from './appearance/palette.js';
import type { ThemesType } from '@pierre/diffs';

export type ThemeChoice = 'system' | 'light' | 'dark';

/**
 * The themes the viewer, the markdown renderer and our own tokenizers all highlight with.
 * The app palette in `styles/theme.css` is derived from these two, so code and chrome share one look.
 */
export let CODE_THEMES: ThemesType = { light: getAppearance().light, dark: getAppearance().dark };

/**
 * The highlighter every renderer names: one thread holds one type at a time, and a renderer that
 * names none loads Shiki. It needs WebAssembly SIMD, which every engine that parses the bundle has.
 */
export const HIGHLIGHTER = 'highlights' as const;
const KEY = 'diffle:theme';

export function readTheme(): ThemeChoice {
  try {
    const v = localStorage.getItem(KEY);
    if (v === 'light' || v === 'dark' || v === 'system') return v;
  } catch {
    /* storage unavailable */
  }
  return 'system';
}

const themeListeners = new Set<() => void>();
let selectedTheme = readTheme();
export const getTheme = () => selectedTheme;
export function subscribeTheme(listener: () => void) {
  themeListeners.add(listener);
  return () => {
    themeListeners.delete(listener);
  };
}
export function storeTheme(choice: ThemeChoice): void {
  selectedTheme = choice;
  try {
    localStorage.setItem(KEY, choice);
  } catch {
    /* ignore */
  }
  for (const listener of themeListeners) listener();
}

/** Applies the choice to the document so our CSS, the tree, and the diff viewer agree. */
export function applyTheme(choice: ThemeChoice): void {
  const root = document.documentElement;
  applyPalette(
    choice === 'system'
      ? typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light'
      : choice,
  );
  if (choice === 'system') {
    delete root.dataset.theme;
    root.style.colorScheme = 'light dark';
  } else {
    root.dataset.theme = choice;
    root.style.colorScheme = choice;
  }
}

export function nextTheme(c: ThemeChoice): ThemeChoice {
  return c === 'system' ? 'light' : c === 'light' ? 'dark' : 'system';
}

subscribeAppearance(() => {
  CODE_THEMES = { light: getAppearance().light, dark: getAppearance().dark };
  if (typeof document !== 'undefined') applyTheme(getTheme());
});
if (typeof matchMedia === 'function')
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    if (getTheme() === 'system') applyTheme('system');
  });
