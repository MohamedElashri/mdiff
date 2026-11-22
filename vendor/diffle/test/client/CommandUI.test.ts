// @vitest-environment jsdom
import { act, createElement, useEffect, useState, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CommandProvider, useCommands } from '@/client/commands/CommandProvider.js';
import type { CommandRegistry, PaletteEntry } from '@/client/commands/registry.js';
import CommandPalette from '@/client/commands/CommandPalette.js';
import { ShortcutSettings } from '@/client/commands/ShortcutSettings.js';
vi.mock('@/client/api.js', async (original) => {
  const actual = await original<typeof import('@/client/api.js')>();
  const api = { ...actual.api, refs: vi.fn() };
  return { ...actual, api, useApi: () => api };
});
const { useStore } = await import('@/client/store.js');
const { useKeymap } = await import('@/client/keyboard/useKeymap.js');
const { defineDiffStub } = await import('./views.js');
defineDiffStub();
let registry: CommandRegistry;
const toggle = vi.fn();
function Harness() {
  const current = useCommands()!;
  useEffect(() => {
    registry = current;
  }, [current]);
  useKeymap({ toggleTree: toggle });
  return createElement('button', { id: 'origin' }, 'Origin');
}
let root: Root, host: HTMLDivElement;
const initial = useStore.getState();
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  toggle.mockClear();
  useStore.setState({
    ...initial,
    snapshot: {
      root: '/repo',
      review: { kind: 'range', old: 'main', new: 'HEAD', base: 'direct', view: { kind: 'whole' } },
      follows: 'none',
      commentKey: 'key',
      version: 1,
      oldSha: 'a',
      newSha: 'b',
      headSha: 'b',
      context: 5,
      changed: [],
      tree: ['src/main.ts'],
      commit: null,
      commits: { list: [], total: 0, oldSha: 'a', newSha: 'b' },
      iterations: [],
    },
  });
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
  useStore.setState(initial);
});
const tick = () => new Promise((resolve) => setTimeout(resolve, 160));
const press = (target: Element, key: string, init: KeyboardEventInit = {}) => {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, composed: true, ...init });
  target.dispatchEvent(event);
  return event;
};
async function render(children: ReactNode = createElement(Harness)) {
  await act(() => root.render(createElement(CommandProvider, null, children)));
}

it('dispatches once in compact mode, preserves Vim, and suppresses a rebound default', async () => {
  await render();
  await act(() => {
    press(document.body, 'b', { ctrlKey: true });
  });
  expect(toggle).toHaveBeenCalledTimes(1);
  await act(() => {
    registry.setShortcut('review.tree', 'Mod+Shift+U');
  });
  await act(() => {
    press(document.body, 'b', { ctrlKey: true });
  });
  expect(toggle).toHaveBeenCalledTimes(1);
  await act(() => {
    press(document.body, 'U', { ctrlKey: true, shiftKey: true });
  });
  expect(toggle).toHaveBeenCalledTimes(2);
  const style = useStore.getState().diffStyle;
  await act(() => {
    press(document.body, 's');
  });
  expect(useStore.getState().diffStyle).not.toBe(style);
});

it('leaves text, dialogs, IME and terminal keys alone', async () => {
  await render();
  const input = document.createElement('input'),
    terminal = document.createElement('div'),
    dialog = document.createElement('div');
  terminal.className = 'xterm';
  dialog.setAttribute('role', 'dialog');
  document.body.append(input, terminal, dialog);
  try {
    for (const target of [input, terminal, dialog])
      await act(() => {
        press(target, 'b', { ctrlKey: true });
      });
    await act(() => {
      press(document.body, 'b', { ctrlKey: true, isComposing: true });
    });
    expect(toggle).not.toHaveBeenCalled();
  } finally {
    input.remove();
    terminal.remove();
    dialog.remove();
  }
});

it('cancels the palette and restores its trigger focus without the review Escape handler stealing it', async () => {
  await render();
  const origin = document.querySelector<HTMLButtonElement>('#origin')!;
  origin.focus();
  await act(async () => {
    press(origin, 'k', { ctrlKey: true });
    await tick();
  });
  await act(async () => {
    await tick();
  });
  const input = document.querySelector<HTMLInputElement>('[role="combobox"]')!;
  expect(input).not.toBeNull();
  expect(document.activeElement).toBe(input);
  await act(async () => {
    press(input, 'Escape');
    input.dispatchEvent(new KeyboardEvent('keyup', { key: 'Escape', bubbles: true }));
    await tick();
  });
  expect(document.querySelector('.command-palette')).toBeNull();
  expect(document.activeElement).toBe(origin);
});

it('records shortcuts inside preferences, displays collisions and lets Escape cancel recording', async () => {
  await render(createElement('div', { role: 'dialog' }, createElement(Harness), createElement(ShortcutSettings)));
  const button = document.querySelector<HTMLButtonElement>('[aria-label="Record shortcut for Toggle file tree"]')!;
  await act(() => button.click());
  let input = document.querySelector<HTMLInputElement>('[data-shortcut-recorder]')!;
  await act(() => {
    press(input, 'k', { ctrlKey: true });
  });
  expect(document.querySelector('.shortcut-settings [role="status"]')?.textContent).toContain('Already assigned');
  await act(() => {
    press(input, 'U', { ctrlKey: true, shiftKey: true });
  });
  expect(registry.shortcut('review.tree')).toBe('Mod+Shift+U');
  expect(toggle).not.toHaveBeenCalled();
  await act(() =>
    document.querySelector<HTMLButtonElement>('[aria-label="Record shortcut for Toggle file tree"]')!.click(),
  );
  input = document.querySelector<HTMLInputElement>('[data-shortcut-recorder]')!;
  await act(() => {
    press(input, 'Escape');
  });
  expect(document.querySelector('[data-shortcut-recorder]')).toBeNull();
  expect(document.querySelector('.shortcut-settings')).not.toBeNull();
});

it('ignores stale asynchronous results and disposes sources when the palette closes', async () => {
  let finish!: (entries: PaletteEntry[]) => void;
  const dispose = vi.fn();
  const signals: AbortSignal[] = [];
  let close!: () => void;
  function TestPalette() {
    const commands = useCommands()!;
    const [open, setOpen] = useState(true);
    useEffect(() => {
      close = () => setOpen(false);
    }, []);
    useEffect(
      () =>
        commands.registerSource({
          id: 'async',
          group: 'recent',
          open: () => ({
            dispose,
            search: (_query, signal) => {
              signals.push(signal);
              return new Promise((resolve) => {
                finish = resolve;
              });
            },
          }),
        }),
      [commands],
    );
    return createElement(CommandPalette, {
      registry: commands,
      open,
      close,
      previousFocus: { current: null },
      reportError: vi.fn(),
    });
  }
  await render(createElement(TestPalette));
  await act(async () => {
    await tick();
  });
  expect(signals.length).toBeGreaterThan(0);
  await act(async () => {
    close();
    await tick();
  });
  expect(signals.at(-1)?.aborted).toBe(true);
  expect(dispose).toHaveBeenCalled();
  await act(() => finish([{ id: 'stale', label: 'Stale entry', group: 'recent', run: vi.fn() }]));
  expect(document.body.textContent).not.toContain('Stale entry');
});

it('runs a selected command after releasing the focus trap, and allows recording Escape inside the real dialog', async () => {
  await render();
  const origin = document.querySelector<HTMLButtonElement>('#origin')!;
  origin.focus();
  await act(async () => {
    registry.execute('app.palette');
    await tick();
  });
  await act(async () => {
    await tick();
  });
  const input = document.querySelector<HTMLInputElement>('[role="combobox"]')!;
  const entry = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find((option) =>
    option.textContent?.includes('Customize keyboard shortcuts'),
  )!;
  expect(entry).toBeDefined();
  await act(async () => {
    entry.click();
    await tick();
  });
  await act(async () => {
    await tick();
  });
  expect(document.querySelector('.command-palette')).toBeNull();
  const record = document.querySelector<HTMLButtonElement>('[aria-label="Record shortcut for Toggle file tree"]')!;
  expect(record).not.toBeNull();
  await act(() => record.click());
  const recorder = document.querySelector<HTMLInputElement>('[data-shortcut-recorder]')!;
  await act(() => {
    press(recorder, 'Escape');
  });
  expect(document.querySelector('[data-shortcut-recorder]')).toBeNull();
  expect(document.querySelector('.shortcut-settings')).not.toBeNull();
  expect(input.isConnected).toBe(false);
});

it('keeps the latest query when an older source result arrives last', async () => {
  const completions = new Map<string, (entries: PaletteEntry[]) => void>();
  function TestPalette() {
    const commands = useCommands()!;
    useEffect(
      () =>
        commands.registerSource({
          id: 'query',
          group: 'recent',
          open: () => ({
            search: (query) =>
              new Promise((resolve) => {
                completions.set(query, resolve);
              }),
          }),
        }),
      [commands],
    );
    return createElement(CommandPalette, {
      registry: commands,
      open: true,
      close: () => {},
      previousFocus: { current: null },
      reportError: vi.fn(),
    });
  }
  await render(createElement(TestPalette));
  await act(async () => {
    await tick();
  });
  const input = document.querySelector<HTMLInputElement>('[role="combobox"]')!;
  await act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'new');
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await act(async () => {
    await tick();
  });
  await act(() => completions.get('new')!([{ id: 'new', label: 'new result', group: 'recent', run: vi.fn() }]));
  await act(() => completions.get('')!([{ id: 'old', label: 'old result', group: 'recent', run: vi.fn() }]));
  expect(document.body.textContent).toContain('new result');
  expect(document.body.textContent).not.toContain('old result');
});

it('preserves an action’s explicit focus target after closing the palette', async () => {
  const destination = document.createElement('button');
  destination.textContent = 'Destination';
  document.body.append(destination);
  function FocusHandler() {
    const commands = useCommands()!;
    useEffect(() => commands.register({ 'app.preferences': { run: () => destination.focus() } }), [commands]);
    return createElement(Harness);
  }
  try {
    await render(createElement(FocusHandler));
    document.querySelector<HTMLButtonElement>('#origin')!.focus();
    await act(async () => {
      registry.execute('app.palette');
      await tick();
    });
    await act(async () => {
      await tick();
    });
    const entry = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find((option) =>
      option.textContent?.includes('Open preferences'),
    )!;
    await act(async () => {
      entry.click();
      await tick();
    });
    await act(async () => {
      await tick();
    });
    expect(document.activeElement).toBe(destination);
  } finally {
    destination.remove();
  }
});

it('explains why a bound command is unavailable without starting its action', async () => {
  await render();
  const run = vi.fn();
  let remove!: () => void;
  await act(() => {
    remove = registry.register({ 'review.graph': { run, unavailable: () => 'Enable code intelligence first.' } });
    registry.setShortcut('review.graph', 'Alt+G');
  });
  try {
    await act(() => {
      press(document.body, 'g', { altKey: true });
    });
    expect(run).not.toHaveBeenCalled();
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('Enable code intelligence first.');
  } finally {
    await act(() => remove());
  }
});

it('restores a programmatically focused review pane after cancellation', async () => {
  await render(createElement('main', { tabIndex: -1 }, createElement(Harness)));
  const pane = host.querySelector('main')!;
  pane.focus();
  await act(async () => {
    registry.execute('app.palette');
    await tick();
  });
  await act(async () => {
    await tick();
  });
  const input = document.querySelector<HTMLInputElement>('[role="combobox"]')!;
  await act(async () => {
    press(input, 'Escape');
    await tick();
  });
  expect(document.activeElement).toBe(pane);
});

it('hides desktop worktree and hosting filters in the standalone palette', async () => {
  await render();
  await act(async () => {
    registry.execute('app.palette');
    await tick();
  });
  const groups = document.querySelector('.command-groups')!;
  expect(groups.textContent).not.toContain('Worktrees');
  expect(groups.textContent).not.toContain('Hosting');
  expect(groups.textContent).toContain('Files');
});
