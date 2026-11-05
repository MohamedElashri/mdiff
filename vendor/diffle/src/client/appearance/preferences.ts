import { useSyncExternalStore } from 'react';
import { THEMES, type ThemeId } from './catalogue.js';
export const APP_FONTS = [
  ['inter', 'Inter', "'Inter Variable', sans-serif"],
  ['system', 'System UI', 'system-ui, sans-serif'],
  ['source', 'Source Sans 3', "'Source Sans 3 Variable', sans-serif"],
  ['plex', 'IBM Plex Sans', "'IBM Plex Sans', sans-serif"],
  ['noto', 'Noto Sans', "'Noto Sans Variable', sans-serif"],
] as const;
export const CODE_FONTS = [
  ['jetbrains', 'JetBrains Mono', "'JetBrains Mono Variable', monospace"],
  ['fira', 'Fira Code', "'Fira Code Variable', monospace"],
  ['source', 'Source Code Pro', "'Source Code Pro Variable', monospace"],
  ['plex', 'IBM Plex Mono', "'IBM Plex Mono', monospace"],
  ['system', 'System mono', 'ui-monospace, Menlo, Consolas, monospace'],
] as const;
export interface Appearance {
  light: ThemeId;
  dark: ThemeId;
  appFont: string;
  codeFont: string;
  appSize: number;
  codeSize: number;
  codeWeight: number;
  ligatures: boolean;
}
export const DEFAULTS: Appearance = {
  light: 'github-light',
  dark: 'github-dark',
  appFont: 'inter',
  codeFont: 'jetbrains',
  appSize: 13,
  codeSize: 13,
  codeWeight: 400,
  ligatures: false,
};
const KEY = 'mdiff.appearance.v1';
export function normalize(value: Partial<Appearance>): Appearance {
  const theme = (id: unknown, mode: 'light' | 'dark') =>
    THEMES.find((t) => t.id === id && t.mode === mode)?.id ?? DEFAULTS[mode];
  const number = (n: unknown, min: number, max: number, fallback: number) =>
    typeof n === 'number' && Number.isFinite(n) ? Math.max(min, Math.min(max, Math.round(n))) : fallback;
  return {
    light: theme(value.light, 'light'),
    dark: theme(value.dark, 'dark'),
    appFont: APP_FONTS.some((f) => f[0] === value.appFont) ? value.appFont! : DEFAULTS.appFont,
    codeFont: CODE_FONTS.some((f) => f[0] === value.codeFont) ? value.codeFont! : DEFAULTS.codeFont,
    appSize: number(value.appSize, 12, 18, 13),
    codeSize: number(value.codeSize, 11, 22, 13),
    codeWeight: [400, 500, 600].includes(value.codeWeight ?? 0) ? value.codeWeight! : 400,
    ligatures: value.ligatures === true,
  };
}
function read(): Appearance {
  try {
    return normalize(JSON.parse(localStorage.getItem(KEY) ?? '{}') ?? {});
  } catch {
    return DEFAULTS;
  }
}
let current = read();
const listeners = new Set<() => void>();
export const getAppearance = () => current;
export function subscribeAppearance(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
export function setAppearance(patch: Partial<Appearance>) {
  current = normalize({ ...current, ...patch });
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    /* Session settings still work without storage. */
  }
  for (const listener of listeners) listener();
}
export function useAppearance() {
  return useSyncExternalStore(subscribeAppearance, getAppearance, getAppearance);
}
