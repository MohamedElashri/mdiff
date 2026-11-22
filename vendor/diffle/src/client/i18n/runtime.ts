import { currentLocale, formatLocale } from './locale.js';
import { french } from './translations/fr.js';
export type PluralText = Partial<Record<Intl.LDMLPluralRule, string>> & { other: string };
/** IDs are stable for named messages. UI text uses gettext-style English source keys. */
export interface Message {
  id: string;
  text: string | PluralText;
}
export const message = (id: string, text: Message['text']): Message => ({ id, text });
export type Values = Record<string, string | number>;
const plurals = new Map<string, Intl.PluralRules>();
const numbers = new Map<string, Intl.NumberFormat>();
const dates = new Map<string, Intl.DateTimeFormat>();
const accents: Record<string, string> = {
  a: 'á',
  e: 'ë',
  i: 'ï',
  o: 'ö',
  u: 'ü',
  A: 'Á',
  E: 'Ë',
  I: 'Ï',
  O: 'Ö',
  U: 'Ü',
};
// Keep interpolation and literal key hints readable while pseudo-expanding surrounding prose.
const pseudoTokens =
  /(\{\w+\}|\((?:[⌘⌃⌥⇧]*[A-Za-z?<>]{1,3}|(?:Cmd|Ctrl|Alt|Shift|Command|Meta)(?:\+\w+)+)\)|[⌘⌃⌥⇧]+\w+|(?:Cmd|Ctrl|Alt|Shift|Command|Meta)(?:\+\w+)+)/g;
const isPseudoToken = new RegExp(`^(?:${pseudoTokens.source})$`);
/** Preserve placeholders/key hints; expansion exposes clipping without changing user content. */
export function pseudoLocalize(text: string): string {
  return (
    '⟦' +
    text
      .split(pseudoTokens)
      .map((part) =>
        isPseudoToken.test(part)
          ? part
          : part.replace(/[aeiouAEIOU]/g, (letter) => accents[letter]!) +
            (part.trim() ? '~'.repeat(Math.ceil(part.length * 0.3)) : ''),
      )
      .join('') +
    '⟧'
  );
}
function validTranslation(source: Message['text'], candidate: Message['text']): boolean {
  const forms = typeof candidate === 'string' ? [candidate] : Object.values(candidate);
  const originals = typeof source === 'string' ? [source] : Object.values(source);
  const placeholders = (s: string) =>
    [...s.matchAll(/\{(\w+)\}/g)]
      .map((m) => m[1])
      .sort()
      .join(',');
  return forms.every((form) => originals.some((original) => placeholders(original) === placeholders(form)));
}
/** Plain text only. Missing/invalid translations use English plural rules and source text. */
export function translate(
  definition: Message,
  values: Values = {},
  locale = currentLocale as string,
  translations?: Readonly<Record<string, Message['text']>>,
): string {
  const dictionary: Readonly<Record<string, Message['text']>> = translations ?? (locale === 'fr' ? french : {});
  const key = typeof definition.text === 'string' ? definition.text : definition.id;
  const candidate = Object.hasOwn(dictionary, definition.id)
    ? dictionary[definition.id]
    : Object.hasOwn(dictionary, key)
      ? dictionary[key]
      : undefined;
  const translation = candidate != null && validTranslation(definition.text, candidate) ? candidate : undefined;
  const translated = translation ?? definition.text;
  const selectedLocale = locale === 'qps-ploc' ? 'en' : locale;
  const pluralLocale = translation == null ? 'en' : selectedLocale;
  let template: string;
  if (typeof translated === 'string') template = translated;
  else {
    let rules = plurals.get(pluralLocale);
    if (!rules) {
      rules = new Intl.PluralRules(pluralLocale);
      if (plurals.size < 16) plurals.set(pluralLocale, rules);
    }
    template = translated[rules.select(Number(values.count ?? 0))] ?? translated.other;
  }
  if (locale === 'qps-ploc') template = pseudoLocalize(template);
  return template.replace(/\{(\w+)\}/g, (token: string, name: string) => {
    const value = values[name];
    if (value == null) return token;
    if (typeof value !== 'number') return value;
    let formatter = numbers.get(selectedLocale);
    if (!formatter) {
      formatter = new Intl.NumberFormat(selectedLocale);
      if (numbers.size < 16) numbers.set(selectedLocale, formatter);
    }
    return formatter.format(value);
  });
}
/** English text is its translation key; callers retain original spelling and whitespace. */
export function uiText(text: string, values: Values = {}): string {
  return translate({ id: text, text }, values);
}
export function formatDate(
  value: number | Date,
  options: Intl.DateTimeFormatOptions = { dateStyle: 'medium' },
): string {
  const at = value instanceof Date ? value.getTime() : value;
  if (!Number.isFinite(at) || Math.abs(at) > 8.64e15) return uiText('Unknown date');
  const key = JSON.stringify(options);
  let formatter = dates.get(key);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(formatLocale, options);
    if (dates.size < 16) dates.set(key, formatter);
  }
  return formatter.format(at);
}

export const coreMessages = {
  reviewSuspended: message('commands.unavailable.history', 'Return to Changes to use this command.'),
  dismiss: message('commands.dismiss', 'Dismiss'),
  opening: message('commands.unavailable.opening', 'Wait for the review to open.'),
  home: message('commands.unavailable.home', 'Already showing recent reviews.'),
  actionFailed: message('commands.failed', 'The command could not be completed.'),
  palette: message('commands.palette.title', 'Command palette'),
  unavailable: message('commands.unavailable.review', 'Open a review to use this command.'),
  changed: message('commands.unavailable.changed', 'The review changed. Search again.'),
  conflict: message('commands.shortcuts.conflict', 'Already assigned to {command}.'),
  invalid: message(
    'commands.shortcuts.invalid',
    'Use Ctrl, Command, or Alt with a key. System editing shortcuts are reserved.',
  ),
};
