// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { describe, expect, it } from 'vitest';
import { message, translate, pseudoLocalize, uiText, formatDate } from '@/client/i18n/runtime.js';
import { french } from '@/client/i18n/translations/fr.js';
import { languageChoice, resolveLocale, storeLanguage, LANGUAGE_KEY } from '@/client/i18n/locale.js';
import { AppearancePanel } from '@/client/appearance/AppearancePanel.js';

describe('localization boundaries', () => {
  it('uses the translated locale plural categories, with English fallback rules', () => {
    const m = message('items', { one: '{count} item', other: '{count} items' });
    const ar = {
      items: {
        zero: 'zero {count}',
        one: 'one {count}',
        two: 'two {count}',
        few: 'few {count}',
        many: 'many {count}',
        other: 'other {count}',
      },
    };
    expect(translate(m, { count: 0 }, 'ar', ar)).toContain('zero');
    expect(translate(m, { count: 2 }, 'ar', ar)).toContain('two');
    expect(translate(m, { count: 3 }, 'ar', ar)).toContain('few');
    expect(translate(m, { count: 11 }, 'ar', ar)).toContain('many');
    expect(translate(m, { count: 0 }, 'fr', {})).toBe('0 items');
    expect(translate(m, { count: 1 }, 'fr', { items: { other: '{count} élément(s)' } })).toBe('1 élément(s)');
  });
  it('preserves user paths/content and rejects placeholder loss or addition', () => {
    const m = message('path', 'Open {path}');
    const path = "/tmp/repo 'quotes' 世界/<script>{count}";
    expect(translate(m, { path }, 'fr', { path: 'Ouvrir {path}' })).toBe(`Ouvrir ${path}`);
    expect(translate(m, { path }, 'fr', { path: 'Ouvrir' })).toBe(`Open ${path}`);
    expect(translate(m, { path }, 'fr', { path: '{extra} {path}' })).toBe(`Open ${path}`);
    expect(translate(message('constructor', 'constructor'))).toBe('constructor');
  });
  it('expands only templates and never modifies inserted content in pseudo mode', () => {
    const path = '世界/file.ts';
    const translated = translate(message('test', 'Open {path}'), { path }, 'qps-ploc');
    expect(translated).toContain(path);
    expect(translated).toContain('⟦');
    expect(pseudoLocalize('Open {path}')).toContain('{path}');
    const hints = pseudoLocalize('GitHub (o), copy (yp), previous (Ctrl+O), open Cmd+O or ⌘K');
    for (const key of ['(o)', '(yp)', '(Ctrl+O)', 'Cmd+O', '⌘K']) expect(hints).toContain(key);
    expect(pseudoLocalize('Preferences').length).toBeGreaterThan('Preferences'.length * 1.3);
  });
  it('keeps partial French explicit and falls back for untranslated source text', () => {
    expect(translate(message('appearance', 'Appearance'), {}, 'fr')).toBe('Apparence');
    expect(translate(message('untranslated', 'Untranslated feature'), {}, 'fr')).toBe('Untranslated feature');
    expect(
      translate(
        message('commands.palette.results', { one: '{count} result', other: '{count} results' }),
        { count: 0 },
        'fr',
      ),
    ).toBe('0 résultat');
    expect(Object.keys(french).length).toBeGreaterThan(80);
  });
  it('ignores corrupt/unknown settings and fails visibly when storage is unavailable', () => {
    expect(languageChoice({ getItem: () => 'unsupported' })).toBe('system');
    expect(
      languageChoice({
        getItem: () => {
          throw Error('blocked');
        },
      }),
    ).toBe('system');
    expect(resolveLocale('system')).toBe('en');
    expect(resolveLocale('fr')).toBe('fr');
    expect(
      storeLanguage('fr', {
        setItem: () => {
          throw Error('blocked');
        },
      }),
    ).toBe(false);
  });
  it('renders malformed provider timestamps safely and formats valid dates in the chosen locale', () => {
    expect(formatDate(new Date(NaN))).toBe('Unknown date');
    expect(formatDate(Infinity)).toBe('Unknown date');
    expect(formatDate(1e20)).toBe('Unknown date');
    expect(formatDate(new Date('2026-11-22T12:00:00Z'))).toMatch(/2026/);
  });
  it('saves a language choice without replacing the current UI or calling reload', async () => {
    localStorage.removeItem(LANGUAGE_KEY);
    const host = document.createElement('div');
    document.body.append(host);
    const root = createRoot(host);
    await act(() => root.render(createElement(AppearancePanel)));
    const language = host.querySelector<HTMLSelectElement>('select[aria-label="Language"]')!;
    await act(() => {
      language.value = 'fr';
      language.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(localStorage.getItem(LANGUAGE_KEY)).toBe('fr');
    expect(host.textContent).toContain('Restart mdiff');
    expect(uiText('Appearance')).toBe('Appearance');
    expect(host.querySelector('select')).toBe(language);
    await act(() => root.unmount());
    host.remove();
    localStorage.removeItem(LANGUAGE_KEY);
  });
});
