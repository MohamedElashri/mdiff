import { COMMANDS, COMMAND_BY_ID, type CommandDefinition } from './catalogue.js';
import { isReserved, normalizeShortcut, physicalShortcut, shortcutFromEvent, type Platform } from './shortcuts.js';
import { coreMessages as messages, translate } from '@/client/i18n/runtime.js';

export type SearchGroup = 'commands' | 'files' | 'refs' | 'commits' | 'recent' | 'hosting' | 'worktrees';
export interface PaletteEntry {
  id: string;
  label: string;
  detail?: string;
  group: SearchGroup;
  reason?: string | null;
  shortcut?: string;
  run(): unknown;
}
export interface SearchSource {
  id: string;
  group: Exclude<SearchGroup, 'commands'>;
  /** Called once per palette session; expensive work starts only inside search. */
  open(): {
    search(query: string, signal: AbortSignal): Promise<PaletteEntry[]> | PaletteEntry[];
    dispose?(): void;
  };
}
export interface CommandHandler {
  run(): unknown;
  unavailable?(): string | null;
}
export type ShortcutOverrides = Record<string, string | null>;
const STORAGE_KEY = 'mdiff.shortcuts.v1';

/** Window-owned services. No store/transport imports, singleton listeners or eager review work. */
export class CommandRegistry {
  private revision = 0;
  private suspended = 0;
  suspendReview(): () => void {
    this.suspended++;
    this.refresh();
    return () => {
      this.suspended--;
      this.refresh();
    };
  }
  private sourceRevision = 0;
  private listeners = new Set<() => void>();
  private handlers = new Map<string, CommandHandler[]>();
  private sources = new Map<string, SearchSource>();
  private overrides: ShortcutOverrides = {};
  constructor(
    readonly platform: Platform = 'other',
    private storage?: Pick<Storage, 'getItem' | 'setItem'>,
  ) {
    try {
      const saved = JSON.parse(storage?.getItem(STORAGE_KEY) ?? '{}');
      if (saved && typeof saved === 'object' && !Array.isArray(saved)) {
        for (const [id, value] of Object.entries(saved)) {
          if (COMMAND_BY_ID.has(id) && (value === null || typeof value === 'string')) {
            const normalized = value === null ? null : normalizeShortcut(value);
            if (value !== null && (!normalized || isReserved(normalized, platform))) continue;
            this.overrides[id] = normalized;
          }
        }
        // Validate after loading all overrides so swaps and intentionally cleared defaults survive.
        const used = new Set<string>();
        for (const command of COMMANDS) {
          const binding = this.shortcut(command.id);
          if (!binding) continue;
          const physical = physicalShortcut(binding, platform);
          if (used.has(physical)) this.overrides[command.id] = null;
          else used.add(physical);
        }
      }
    } catch {
      /* Corrupt/unavailable storage must not stop keyboard navigation. */
    }
  }
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  version = () => this.revision;
  sourceVersion = () => this.sourceRevision;
  invalidateSources = () => {
    this.sourceRevision++;
    this.refresh();
  };
  refresh = () => {
    this.revision++;
    for (const listener of this.listeners) listener();
  };
  definitions(): readonly CommandDefinition[] {
    return COMMANDS;
  }
  shortcut(id: string): string | null {
    return Object.hasOwn(this.overrides, id) ? (this.overrides[id] ?? null) : this.defaultShortcut(id);
  }
  private defaultShortcut(id: string): string | null {
    // Ctrl+O is the existing Vim jump-back binding on Linux.
    if (id === 'app.openRepository' && this.platform !== 'mac') return 'Mod+Shift+O';
    return COMMAND_BY_ID.get(id)?.shortcut ?? null;
  }
  handler(id: string): CommandHandler | undefined {
    return this.handlers.get(id)?.at(-1);
  }
  reason(id: string): string | null {
    if (this.suspended && COMMAND_BY_ID.get(id)?.scope === 'review') return translate(messages.reviewSuspended);
    const handler = this.handler(id);
    return handler ? (handler.unavailable?.() ?? null) : translate(messages.unavailable);
  }
  execute(id: string): unknown {
    if (this.reason(id)) return false;
    return this.handler(id)!.run();
  }
  register(handlers: Record<string, CommandHandler>): () => void {
    for (const [id, handler] of Object.entries(handlers)) {
      if (!COMMAND_BY_ID.has(id)) throw new Error(`Unknown command: ${id}`);
      const stack = this.handlers.get(id) ?? [];
      stack.push(handler);
      this.handlers.set(id, stack);
    }
    this.refresh();
    return () => {
      for (const [id, handler] of Object.entries(handlers)) {
        const stack = this.handlers.get(id);
        if (stack)
          this.handlers.set(
            id,
            stack.filter((item) => item !== handler),
          );
      }
      this.refresh();
    };
  }
  registerSource(source: SearchSource): () => void {
    this.sources.set(source.id, source);
    this.invalidateSources();
    return () => {
      if (this.sources.get(source.id) === source) {
        this.sources.delete(source.id);
        this.invalidateSources();
      }
    };
  }
  hasSourceGroup(group: SearchGroup): boolean {
    return [...this.sources.values()].some((source) => source.group === group);
  }
  openSources() {
    return [...this.sources.values()].map((source) => ({ group: source.group, ...source.open() }));
  }
  entries(): PaletteEntry[] {
    return COMMANDS.filter((command) => this.handler(command.id) || command.scope === 'review').map((command) => ({
      id: command.id,
      label: translate(command.label),
      group: 'commands',
      reason: this.reason(command.id),
      shortcut: this.shortcut(command.id) ?? command.key,
      run: () => this.execute(command.id),
    }));
  }
  match(event: KeyboardEvent): CommandDefinition | undefined {
    const binding = eventBinding(event, this.platform);
    if (!binding) return;
    return COMMANDS.find((command) => {
      const shortcut = this.shortcut(command.id);
      return shortcut && physicalShortcut(shortcut, this.platform) === binding && this.handler(command.id);
    });
  }
  /** Suppress the old modifier handler even when its default has been rebound/disabled. */
  manages(event: KeyboardEvent): boolean {
    const binding = eventBinding(event, this.platform);
    return (
      !!binding &&
      COMMANDS.some((command) => {
        const shortcut = this.defaultShortcut(command.id);
        return (
          shortcut &&
          (physicalShortcut(shortcut, this.platform) === binding ||
            // The legacy keymap accepted both Ctrl and Meta for these defaults.
            // Retire those aliases too, so clearing/rebinding cannot fall through.
            (shortcut.startsWith('Mod+') &&
              physicalShortcut(shortcut, this.platform === 'mac' ? 'other' : 'mac') === binding))
        );
      })
    );
  }
  setShortcut(id: string, binding: string | null): string | null {
    if (!COMMAND_BY_ID.has(id)) return translate(messages.invalid);
    const normalized = binding === null ? null : normalizeShortcut(binding);
    if (binding !== null && (!normalized || isReserved(normalized, this.platform))) return translate(messages.invalid);
    const collision =
      normalized &&
      COMMANDS.find(
        (other) =>
          other.id !== id &&
          this.shortcut(other.id) &&
          physicalShortcut(this.shortcut(other.id)!, this.platform) === physicalShortcut(normalized, this.platform),
      );
    if (collision) return translate(messages.conflict, { command: translate(collision.label) });
    this.overrides = { ...this.overrides, [id]: normalized };
    this.save();
    return null;
  }
  shortcutOverrides(): ShortcutOverrides {
    return { ...this.overrides };
  }
  replaceShortcuts(value: unknown): void {
    const normalized = new CommandRegistry(this.platform, {
      getItem: () => JSON.stringify(value),
      setItem: () => {},
    });
    this.overrides = normalized.overrides;
    this.save();
  }
  resetShortcuts(): void {
    this.overrides = {};
    this.save();
  }
  private save() {
    try {
      this.storage?.setItem(STORAGE_KEY, JSON.stringify(this.overrides));
    } catch {
      /* Session changes still apply. */
    }
    this.refresh();
  }
}

function eventBinding(event: KeyboardEvent, platform: Platform): string | null {
  const binding = shortcutFromEvent(event, platform);
  return binding ? physicalShortcut(binding, platform) : null;
}
