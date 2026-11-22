// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import { CommandRegistry } from '@/client/commands/registry.js';
import { COMMANDS } from '@/client/commands/catalogue.js';
import { normalizeShortcut, shortcutFromEvent, keyboardTarget, physicalShortcut } from '@/client/commands/shortcuts.js';
import { rankMatches, fuzzyScore } from '@/client/commands/search.js';
import { message, messages, translate } from '@/client/i18n/messages.js';
import type { ReviewState } from '@/client/store.js';
import type { Snapshot } from '@/shared/protocol.js';
const refs = vi.fn();
vi.mock('@/client/api.js', () => ({ api: { refs } }));
const { reviewSources } = await import('@/client/commands/reviewSources.js');

const snapshot = (): Snapshot => ({
  root: '/repo',
  review: { kind: 'range', old: 'main', new: 'HEAD', base: 'direct', view: { kind: 'whole' } },
  follows: 'none',
  commentKey: 'key',
  version: 1,
  oldSha: 'a',
  newSha: 'b',
  headSha: 'b',
  context: 5,
  changed: [
    {
      path: 'deleted.ts',
      status: 'D',
      additions: 0,
      deletions: 1,
      binary: false,
      blob: '',
      oldBlob: 'a',
      generated: false,
    },
  ],
  tree: ['src/main.ts'],
  commit: null,
  commits: {
    list: [
      { sha: 'abc', short: 'abc', message: 'Fix palette\nDetails', author: 'Ada', date: 1, email: 'ada@example.com' },
    ],
    total: 1,
    oldSha: 'a',
    newSha: 'b',
  },
  iterations: [],
});
const key = (value: string, init: KeyboardEventInit = {}) =>
  new KeyboardEvent('keydown', { key: value, ctrlKey: true, ...init });

describe('command registry', () => {
  it.each(['mac', 'other'] as const)('has collision-free defaults on %s and leaves Vim aliases fixed', (platform) => {
    const registry = new CommandRegistry(platform);
    const physical = COMMANDS.flatMap((command) =>
      registry.shortcut(command.id) ? [physicalShortcut(registry.shortcut(command.id)!, platform)] : [],
    );
    expect(new Set(physical).size).toBe(physical.length);
    expect(registry.shortcut('app.openRepository')).toBe(platform === 'mac' ? 'Mod+O' : 'Mod+Shift+O');
    expect(COMMANDS.find((command) => command.id === 'review.viewed')?.key).toBe('v');
    expect(registry.manages(key('b'))).toBe(true);
    expect(registry.manages(key('b', { ctrlKey: false, metaKey: true }))).toBe(true);
  });
  it('shares availability and execution, and disposal cannot remove a newer review handler', () => {
    const registry = new CommandRegistry();
    const old = vi.fn(),
      next = vi.fn();
    const removeOld = registry.register({ 'review.viewed': { run: old } });
    const removeNext = registry.register({ 'review.viewed': { run: next, unavailable: () => 'No file' } });
    removeOld();
    expect(registry.execute('review.viewed')).toBe(false);
    expect(registry.entries().find((entry) => entry.id === 'review.viewed')?.reason).toBe('No file');
    removeNext();
    expect(registry.reason('review.viewed')).toContain('Open a review');
    expect(old).not.toHaveBeenCalled();
    expect(next).not.toHaveBeenCalled();
  });
  it('rebinds, persists, disables old defaults, detects physical collisions, and resets', () => {
    const data = new Map<string, string>();
    const storage = {
      getItem: (id: string) => data.get(id) ?? null,
      setItem: (id: string, value: string) => {
        data.set(id, value);
      },
    };
    const registry = new CommandRegistry('other', storage);
    const run = vi.fn();
    registry.register({ 'review.tree': { run } });
    expect(registry.setShortcut('review.tree', 'Mod+Shift+U')).toBeNull();
    expect(registry.match(key('b'))).toBeUndefined();
    expect(registry.manages(key('b'))).toBe(true);
    expect(registry.match(key('U', { shiftKey: true }))?.id).toBe('review.tree');
    expect(registry.setShortcut('review.panel', 'Ctrl+Shift+U')).toContain('Toggle file tree');
    expect(registry.setShortcut('review.tree', 'Mod+C')).toContain('reserved');
    expect(new CommandRegistry('other', storage).shortcut('review.tree')).toBe('Mod+Shift+U');
    registry.setShortcut('review.tree', null);
    expect(registry.shortcut('review.tree')).toBeNull();
    registry.resetShortcuts();
    expect(registry.shortcut('review.tree')).toBe('Mod+B');
  });
  it('ignores unknown, corrupt and reserved saved bindings and normalizes valid ones', () => {
    const storage = {
      getItem: () =>
        JSON.stringify({
          'bad.id': 'Mod+U',
          'review.tree': 'Shift+Mod+U',
          'review.panel': 'Mod+C',
          'review.find': 'Mod+K',
        }),
      setItem: vi.fn(),
    };
    const registry = new CommandRegistry('other', storage);
    expect(registry.shortcut('review.tree')).toBe('Mod+Shift+U');
    expect(registry.shortcut('review.panel')).toBe('Mod+Shift+B');
    expect(registry.shortcut('review.find')).toBeNull();
    expect(new CommandRegistry('other', { ...storage, getItem: () => '{' }).shortcut('review.tree')).toBe('Mod+B');
  });
  it('does not reopen source sessions for ordinary availability changes', () => {
    const registry = new CommandRegistry();
    const initial = registry.sourceVersion();
    registry.refresh();
    expect(registry.sourceVersion()).toBe(initial);
    const remove = registry.registerSource({ id: 'test', group: 'files', open: () => ({ search: () => [] }) });
    expect(registry.sourceVersion()).toBeGreaterThan(initial);
    remove();
    expect(registry.openSources()).toEqual([]);
  });
});

describe('shortcut input boundaries', () => {
  it('normalizes shifted punctuation and macOS modifiers, rejecting plain keys, IME and AltGr', () => {
    expect(normalizeShortcut('Shift+Mod+u')).toBe('Mod+Shift+U');
    expect(normalizeShortcut('v')).toBeNull();
    expect(normalizeShortcut('Shift+V')).toBeNull();
    expect(shortcutFromEvent(key('<', { code: 'Comma', shiftKey: true }), 'other')).toBe('Mod+Shift+,');
    expect(shortcutFromEvent(key('k', { ctrlKey: false, metaKey: true }), 'mac')).toBe('Mod+K');
    expect(shortcutFromEvent(key('k', { isComposing: true }), 'other')).toBeNull();
    expect(shortcutFromEvent(key('@', { altKey: true }), 'other')).toBeNull();
  });
  it('looks through shadow inputs, and blocks terminals and unfocused modal dialogs', () => {
    const host = document.createElement('div');
    const input = document.createElement('input');
    host.attachShadow({ mode: 'open' }).append(input);
    document.body.append(host);
    let result: ReturnType<typeof keyboardTarget> | undefined;
    const listen = (event: Event) => {
      result = keyboardTarget(event);
    };
    window.addEventListener('keydown', listen);
    input.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, composed: true }));
    expect(result).toEqual({ input: true, blocked: false });
    host.setAttribute('data-terminal', '');
    input.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, composed: true }));
    expect(result?.blocked).toBe(true);
    const dialog = document.createElement('div');
    dialog.setAttribute('role', 'dialog');
    dialog.setAttribute('aria-modal', 'true');
    document.body.append(dialog);
    document.body.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true }));
    expect(result?.blocked).toBe(true);
    window.removeEventListener('keydown', listen);
    host.remove();
    dialog.remove();
  });
});

describe('bounded search', () => {
  it('ranks exact and subsequence matches before spelling corrections, with stable caps', () => {
    const items = ['unrelated', 'command palette', 'palette', 'palet', 'path/lower/entry.ts'];
    expect(rankMatches(items, 'palette', (s) => s)[0]).toBe('palette');
    expect(fuzzyScore('paletet', 'palette')).toBeGreaterThan(-Infinity);
    expect(rankMatches(items, 'ple', (s) => s, 1)).toHaveLength(1);
    expect(rankMatches(items, '', (s) => s, 0)).toEqual([]);
    expect(rankMatches(items, '', (s) => s, 2)).toEqual(items.slice(0, 2));
  });
  it('bounds 20k file searches without a large result allocation', () => {
    const files = Array.from({ length: 20_000 }, (_, i) => `packages/project-${i}/src/command-palette.ts`);
    const start = performance.now();
    for (const query of ['command', 'cmdplt', 'paletet', 'absent'])
      expect(rankMatches(files, query, (s) => s).length).toBeLessThanOrEqual(20);
    expect(performance.now() - start).toBeLessThan(1000);
  });
});

describe('review search sources', () => {
  it('includes deleted files and comparison commits, and refuses stale review actions', async () => {
    const state = { snapshot: snapshot(), openFile: vi.fn(), focusCommit: vi.fn() } as unknown as ReviewState;
    const navigate = vi.fn();
    const sources = reviewSources(() => state, navigate);
    const files = sources[0]!.open();
    const commits = sources[1]!.open();
    const entries = await files.search('deleted', new AbortController().signal);
    expect(entries[0]?.label).toBe('deleted.ts');
    await entries[0]!.run();
    expect(state.openFile).toHaveBeenCalledWith('deleted.ts');
    const commitEntries = await commits.search('Ada', new AbortController().signal);
    commitEntries[0]!.run();
    expect(state.focusCommit).toHaveBeenCalledWith('abc');
    expect(navigate).toHaveBeenCalledTimes(2);
    state.snapshot = snapshot();
    expect(entries[0]!.run()).toBe(false);
    expect(await files.search('', new AbortController().signal)).toEqual([]);
    expect(state.openFile).toHaveBeenCalledTimes(1);
  });
  it('loads refs once per session, ignores superseded queries and cancels transport on close', async () => {
    refs.mockReset();
    const state = { snapshot: snapshot(), enter: vi.fn() } as unknown as ReviewState;
    let finish!: (value: unknown) => void;
    refs.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const source = reviewSources(() => state)[2]!.open();
    const first = new AbortController(),
      second = new AbortController();
    const stale = source.search('ma', first.signal);
    first.abort();
    const latest = source.search('main', second.signal);
    expect(refs).toHaveBeenCalledTimes(1);
    const transport = refs.mock.calls[0]![0] as AbortSignal;
    expect(transport.aborted).toBe(false);
    finish({ branches: ['main'], remoteBranches: [], tags: [] });
    expect(await stale).toEqual([]);
    const entries = await latest;
    expect(entries[0]?.id).toBe('ref:refs/heads/main');
    entries[0]!.run();
    expect(state.enter).toHaveBeenCalledWith({ kind: 'revspec', args: ['refs/heads/main'] });
    await source.search('main', second.signal);
    expect(refs).toHaveBeenCalledTimes(1);
    source.dispose?.();
    expect(transport.aborted).toBe(true);
  });
});

describe('message foundation', () => {
  it('provides unique stable IDs, English fallback, interpolation and locale-aware plurals', () => {
    const definitions = [...Object.values(messages), ...COMMANDS.map((command) => command.label)];
    expect(new Set(definitions.map((entry) => entry.id)).size).toBe(definitions.length);
    expect(translate(messages.results, { count: 1 })).toBe('1 result');
    expect(translate(messages.results, { count: 1200 })).toBe('1,200 results');
    expect(translate(messages.results, { count: 2 }, 'en', {})).toBe('2 results');
    expect(translate(messages.results, { count: 0 }, 'fr', {})).toBe('0 results');
    expect(translate(message('hello', 'Hello {name}'), { name: '<script>' })).toBe('Hello <script>');
    expect(translate(messages.record, { command: 'Tree' })).toBe('Record shortcut for Tree');
  });
});

it('does not return focus from an old asynchronous file load into a replacement review', async () => {
  let finish!: () => void;
  const navigate = vi.fn();
  const state = {
    snapshot: snapshot(),
    openFile: vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    ),
  } as unknown as ReviewState;
  const source = reviewSources(() => state, navigate)[0]!.open();
  const entries = await source.search('main', new AbortController().signal);
  const pending = entries[0]!.run();
  state.snapshot = snapshot();
  finish();
  await pending;
  expect(navigate).not.toHaveBeenCalled();
});

it('suspends review commands while history owns focus, without blocking app commands or leaking after cleanup', () => {
  const registry = new CommandRegistry();
  const stop = registry.suspendReview();
  const remove = registry.register({ 'review.tree': { run: vi.fn() }, 'app.history': { run: vi.fn() } });
  expect(registry.reason('review.tree')).toContain('Return to Changes');
  expect(registry.reason('app.history')).toBeNull();
  stop();
  expect(registry.reason('review.tree')).toBeNull();
  remove();
});
