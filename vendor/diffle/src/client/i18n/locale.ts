/** Locale is fixed for a page lifetime: applying it must not remount an active review or PTY. */
export type Locale = 'en' | 'fr' | 'qps-ploc';
export type LanguageChoice = 'system' | Locale;
export const LANGUAGE_KEY = 'mdiff.language.v1';
export function languageChoice(storage?: Pick<Storage, 'getItem'>): LanguageChoice {
  if (!storage && !('document' in globalThis)) return 'system';
  try {
    const value = (storage ?? globalThis.localStorage)?.getItem(LANGUAGE_KEY);
    if (
      value === 'en' ||
      value === 'fr' ||
      ((import.meta as ImportMeta & { env?: { DEV?: boolean } }).env?.DEV && value === 'qps-ploc')
    )
      return value;
  } catch {
    /* Restricted storage falls back to English. */
  }
  return 'system';
}
export function resolveLocale(choice: LanguageChoice): Locale {
  if (choice !== 'system') return choice;
  // French is a preview; automatic selection only uses fully supported English.
  return 'en';
}
export const currentLocale = resolveLocale(languageChoice());
export const formatLocale = currentLocale === 'qps-ploc' ? 'en' : currentLocale;
export function storeLanguage(choice: LanguageChoice, storage?: Pick<Storage, 'setItem'>): boolean {
  try {
    (storage ?? globalThis.localStorage).setItem(LANGUAGE_KEY, choice);
    return true;
  } catch {
    return false;
  }
}
