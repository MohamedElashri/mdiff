export type Platform = 'mac' | 'other';
const MODIFIERS = ['Mod', 'Ctrl', 'Alt', 'Shift', 'Meta'];
const KEY_NAMES: Record<string, string> = { ' ': 'Space', '+': 'Plus', Esc: 'Escape' };
const SHIFTED_PUNCTUATION: Record<string, string> = {
  Comma: ',',
  Period: '.',
  Slash: '/',
  Semicolon: ';',
  Quote: "'",
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Minus: '-',
  Equal: '=',
  Backquote: '`',
};
const RESERVED = new Set(['a', 'c', 'v', 'x', 'z', 'y', 'q', 'w', 'h', 'm', 'l', 'r', 't', 'n']);

export function normalizeShortcut(input: string): string | null {
  const parts = input.split('+').map((part) => part.trim());
  const raw = parts.pop() ?? '';
  const key = KEY_NAMES[raw] ?? (raw.length === 1 ? raw.toUpperCase() : raw);
  if (!key || !parts.length || new Set(parts).size !== parts.length) return null;
  if (parts.some((part) => !MODIFIERS.includes(part)) || !parts.some((part) => part !== 'Shift')) return null;
  if (!/^(?:[\p{L}\p{N}\p{P}\p{S}]|F(?:[1-9]|1[0-2])|Space|Plus|Arrow(?:Up|Down|Left|Right))$/u.test(key)) return null;
  return [...MODIFIERS.filter((part) => parts.includes(part)), key].join('+');
}

export function physicalShortcut(binding: string, platform: Platform): string {
  const parts = binding.split('+');
  const key = parts.pop()!;
  const modifiers = parts.map((part) => (part === 'Mod' ? (platform === 'mac' ? 'Meta' : 'Ctrl') : part));
  return [...MODIFIERS.filter((part) => modifiers.includes(part)), key.toUpperCase()].join('+');
}

export function isReserved(binding: string, platform: Platform): boolean {
  const physical = physicalShortcut(binding, platform).split('+');
  const key = physical.pop()!.toLowerCase();
  const primary = platform === 'mac' ? 'Meta' : 'Ctrl';
  return physical.includes(primary) && !physical.includes('Alt') && RESERVED.has(key);
}

export function shortcutFromEvent(event: KeyboardEvent, platform: Platform): string | null {
  if (event.isComposing || event.getModifierState('AltGraph') || (event.ctrlKey && event.altKey && !event.metaKey))
    return null;
  const mods: string[] = [];
  if (platform === 'mac' ? event.metaKey : event.ctrlKey) mods.push('Mod');
  if (platform === 'mac' && event.ctrlKey) mods.push('Ctrl');
  if (platform !== 'mac' && event.metaKey) mods.push('Meta');
  if (event.altKey) mods.push('Alt');
  if (event.shiftKey) mods.push('Shift');
  const key = event.shiftKey ? (SHIFTED_PUNCTUATION[event.code] ?? event.key) : event.key;
  return normalizeShortcut([...mods, KEY_NAMES[key] ?? key].join('+'));
}

export function shortcutLabel(binding: string | null | undefined, platform: Platform): string {
  if (!binding) return '';
  if (platform !== 'mac') return binding.replace('Mod', 'Ctrl');
  return binding
    .replace('Mod+', '⌘')
    .replace('Ctrl+', '⌃')
    .replace('Alt+', '⌥')
    .replace('Shift+', '⇧')
    .replace('Meta+', '⌘');
}

/** Look through shadow roots; dialogs and terminals always own their input. */
export function keyboardTarget(event: Event): { input: boolean; blocked: boolean } {
  const path = event.composedPath();
  const elements = path.filter((item): item is HTMLElement => item instanceof HTMLElement);
  return {
    input: elements.some((el) => el.matches('input, textarea, select') || el.isContentEditable),
    blocked:
      !!document.querySelector('[role="dialog"][aria-modal="true"]') ||
      elements.some((el) =>
        el.matches('[role="dialog"], [role="alertdialog"], [data-terminal], .xterm, [data-shortcut-recorder]'),
      ),
  };
}
