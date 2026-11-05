import { APP_FONTS, CODE_FONTS, getAppearance } from './preferences.js';
import { THEMES } from './catalogue.js';
const rgb = (hex: string) => [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16));
const hex = (values: number[]) => '#' + values.map((n) => Math.round(n).toString(16).padStart(2, '0')).join('');
export function blend(a: string, b: string, amount: number) {
  return hex(rgb(a).map((n, i) => n * (1 - amount) + rgb(b)[i]! * amount));
}
export function contrast(a: string, b: string) {
  const luminance = (color: string) =>
    rgb(color)
      .map((n) => {
        n /= 255;
        return n <= 0.04045 ? n / 12.92 : ((n + 0.055) / 1.055) ** 2.4;
      })
      .reduce((sum, n, i) => sum + n * [0.2126, 0.7152, 0.0722][i]!, 0);
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}
export function readable(color: string, bg: string, fg: string) {
  if (!/^#[a-f\d]{6}$/i.test(color)) return fg;
  for (let i = 0; i <= 10; i++) {
    const c = blend(color, fg, i / 10);
    if (contrast(c, bg) >= 4.5) return c;
  }
  return fg;
}
export function applyPalette(mode: 'light' | 'dark') {
  const settings = getAppearance();
  const theme = THEMES.find((t) => t.id === settings[mode])!;
  const root = document.documentElement;
  const { bg } = theme;
  const fg = readable(theme.fg, bg, mode === 'dark' ? '#ffffff' : '#000000');
  const accent = readable(theme.accent.slice(0, 7), bg, fg);
  const vars: Record<string, string> = {
    bg,
    fg,
    'bg-2': blend(bg, fg, 0.035),
    'bg-3': blend(bg, fg, 0.09),
    'bg-raised': blend(bg, fg, 0.06),
    'fg-2': readable(blend(bg, fg, 0.58), bg, fg),
    border: blend(bg, fg, 0.2),
    accent,
    'accent-fg': contrast(accent, '#ffffff') > contrast(accent, '#000000') ? '#ffffff' : '#000000',
    add: readable(mode === 'dark' ? '#6fcf97' : '#237a46', bg, fg),
    del: readable(mode === 'dark' ? '#f4878c' : '#b93645', bg, fg),
    danger: readable(mode === 'dark' ? '#f4878c' : '#b93645', bg, fg),
    warn: readable(mode === 'dark' ? '#e5c07b' : '#966b08', bg, fg),
    'kind-type': accent,
    'kind-callable': readable(mode === 'dark' ? '#cba6f7' : '#7951a8', bg, fg),
    'kind-value': accent,
    'bg-card': blend(bg, fg, 0.025),
    'search-match': accent + '40',
    'search-current': accent,
    'search-current-fg': bg,
    mono: CODE_FONTS.find((f) => f[0] === settings.codeFont)![2],
    sans: APP_FONTS.find((f) => f[0] === settings.appFont)![2],
  };
  for (const [key, value] of Object.entries(vars)) root.style.setProperty('--' + key, value);
  root.style.fontSize = `${settings.appSize / 0.8125}px`;
  root.style.setProperty('--code-size', settings.codeSize + 'px');
  root.style.setProperty('--code-weight', String(settings.codeWeight));
  root.style.setProperty('--code-ligatures', settings.ligatures ? 'common-ligatures contextual' : 'none');
  root.dataset.palette = theme.id;
}
