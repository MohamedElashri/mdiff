// @vitest-environment jsdom
import { act, createElement, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Test the actual lazy desktop browser.
import WorktreesHub from '../../../../src/worktrees/WorktreesHub';
// oxlint-disable-next-line no-restricted-imports -- Native desktop-only DTOs.
import { worktreeApi, type Worktree, type Tools, type Status } from '../../../../src/worktrees/api';
// oxlint-disable-next-line no-restricted-imports -- Cached search remains independent of native Git reads.
import { worktreeEntries } from '../../../../src/worktrees/palette';
const events = vi.hoisted(() => new Map<string, (event: { payload: number }) => void>());
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, callback: (e: { payload: number }) => void) => {
    events.set(name, callback);
    return () => events.delete(name);
  },
}));
vi.mock('../../../../src/worktrees/api', () => ({
  worktreeApi: {
    read: vi.fn(),
    tools: vi.fn(),
    launch: vi.fn(),
    save: vi.fn(),
  },
}));
vi.mock('@/client/ui/dialog', () => ({
  Dialog: ({ children }: { children: ReactNode }) => children,
  DialogContent: ({ children, ...props }: { children: ReactNode }) => createElement('div', props, children),
}));
const row: Worktree = {
  id: 'first',
  path: '/repo/main',
  head: 'a'.repeat(40),
  branch: 'main',
  detached: false,
  bare: false,
  locked: null,
  prunable: null,
  current: true,
  canOpen: true,
  canInspect: true,
  reason: null,
  reviews: [
    {
      path: '/repo/main',
      tab: 'tab-1',
      workspace: 'w',
      workspaceName: 'Default',
      label: 'Working changes',
    },
  ],
};
const other: Worktree = {
  ...row,
  id: 'second',
  path: "/repo/linked 'quotes'\n folder",
  branch: 'feature',
  current: false,
  locked: 'Keep for editor',
  reviews: [],
};
const status: Status = {
  head: row.head,
  branch: 'main',
  upstream: 'origin/main',
  ahead: 2,
  behind: 1,
  staged: 1,
  unstaged: 3,
  untracked: 2,
  conflicts: 0,
  shallow: false,
};
const tools: Tools = {
  root: other.path,
  temporary: false,
  editors: [{ id: 'vscode', name: 'VS Code' }],
  terminals: [{ id: 'kitty', name: 'kitty' }],
  preferences: { revision: 2, terminal: null },
};
let root: Root, host: HTMLDivElement;
const opened = vi.fn(async () => {}),
  closed = vi.fn(),
  inventory = vi.fn(),
  embedded = vi.fn(async () => {});
const button = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>('button')].find(
    (b) => b.textContent?.trim() === label || b.getAttribute('aria-label') === label,
  )!;
async function mount(initial: 'worktrees' | 'terminal' | 'editor' = 'worktrees') {
  await act(() =>
    root.render(
      createElement(WorktreesHub, {
        id: 'context-1',
        initial,
        onClose: closed,
        onOpen: opened,
        onInventory: inventory,
        onEmbedded: embedded,
      }),
    ),
  );
}
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  events.clear();
  vi.mocked(worktreeApi.read).mockImplementation(async (_id, query) => {
    const q = query as { kind: string };
    if (q.kind === 'list') return { root: row.path, managed: false, items: [row, other] };
    if (q.kind === 'open') return { path: other.path };
    return status;
  });
  vi.mocked(worktreeApi.tools).mockResolvedValue(tools);
  vi.mocked(worktreeApi.launch).mockResolvedValue();
  vi.mocked(worktreeApi.save).mockResolvedValue({
    revision: 3,
    terminal: 'kitty',
  });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
});
it('reads status only for the selected worktree and retains path and review associations', async () => {
  await mount();
  expect(host.textContent).toContain('Known review tabs');
  expect(host.textContent).toContain('Default: Working changes');
  const statuses = () =>
    vi.mocked(worktreeApi.read).mock.calls.filter(([, q]) => (q as { kind: string }).kind === 'status');
  expect(statuses()).toHaveLength(1);
  expect(statuses()[0]![1]).toEqual({ kind: 'status', worktree: 'first' });
  expect(worktreeApi.tools).not.toHaveBeenCalled();
  expect(worktreeApi.launch).not.toHaveBeenCalled();
  await act(() => host.querySelectorAll<HTMLButtonElement>('.worktree-list button')[1]!.click());
  expect(statuses()).toHaveLength(2);
  expect(statuses()[1]![1]).toEqual({ kind: 'status', worktree: 'second' });
  expect(host.textContent).toContain('Locked: Keep for editor');
});
it('revalidates the native selection before opening a review instead of trusting a cached renderer path', async () => {
  await mount();
  await act(() => button('Open working changes').click());
  expect(worktreeApi.read).toHaveBeenCalledWith('context-1', {
    kind: 'open',
    worktree: 'first',
  });
  expect(opened).toHaveBeenCalledWith(other.path);
  expect(worktreeApi.launch).not.toHaveBeenCalled();
});
it('does not inspect unavailable worktrees or enable open and external-tool actions', async () => {
  vi.mocked(worktreeApi.read).mockResolvedValue({
    root: row.path,
    managed: false,
    items: [
      {
        ...row,
        canOpen: false,
        canInspect: false,
        prunable: 'folder missing',
        reason: 'Repair externally',
      },
    ],
  });
  await mount();
  expect(vi.mocked(worktreeApi.read).mock.calls).toHaveLength(1);
  expect(button('Open working changes').disabled).toBe(true);
  expect(button('Open in terminal').disabled).toBe(true);
  expect(host.textContent).toContain('Repair externally');
});
it('opening the terminal picker discovers tools without Git inventory scans or shell launch', async () => {
  await mount('terminal');
  expect(worktreeApi.read).not.toHaveBeenCalled();
  expect(worktreeApi.tools).toHaveBeenCalledWith('context-1', null);
  expect(worktreeApi.launch).not.toHaveBeenCalled();
  await act(() => button('Open terminal').click());
  expect(worktreeApi.launch).toHaveBeenCalledWith('context-1', null, 'terminal', 'kitty');
  expect(host.textContent).toContain('launch accepted');
});
it('launches the editor against the selected worktree and saves an explicit default without launching it', async () => {
  await mount();
  await act(() => host.querySelectorAll<HTMLButtonElement>('.worktree-list button')[1]!.click());
  await act(() => button('Open in editor').click());
  expect(worktreeApi.tools).toHaveBeenCalledWith('context-1', 'second');
  await act(() => button('Use as default').click());
  expect(worktreeApi.save).toHaveBeenCalledWith(2, 'kitty');
  expect(worktreeApi.launch).not.toHaveBeenCalled();
  await act(() => button('VS Code').click());
  expect(worktreeApi.launch).toHaveBeenCalledWith('context-1', 'second', 'editor', 'vscode');
});
it('shows stale preference failures and missing default terminals instead of silently launching another program', async () => {
  vi.mocked(worktreeApi.tools).mockResolvedValue({
    ...tools,
    preferences: { revision: 3, terminal: 'terminal' },
  });
  await mount('terminal');
  expect(host.textContent).toContain('Missing: terminal');
  expect(button('Open terminal').disabled).toBe(true);
  const select = host.querySelector<HTMLSelectElement>('select')!;
  await act(() => {
    select.value = 'kitty';
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
  vi.mocked(worktreeApi.save).mockRejectedValue(Error('Preferences changed in another window'));
  await act(() => button('Use as default').click());
  expect(host.querySelector('[role=alert]')?.textContent).toContain('another window');
  expect(worktreeApi.launch).not.toHaveBeenCalled();
});
it('aborts old status reads and rejects stale completions after selecting another worktree', async () => {
  let finish: ((value: unknown) => void) | undefined;
  vi.mocked(worktreeApi.read).mockImplementation(async (_id, query) => {
    const q = query as { kind: string; worktree?: string };
    if (q.kind === 'list') return { root: row.path, managed: false, items: [row, other] };
    if (q.worktree === 'first')
      return new Promise((resolve) => {
        finish = resolve;
      });
    return { ...status, branch: 'feature', staged: 99 };
  });
  await mount();
  const signal = vi.mocked(worktreeApi.read).mock.calls[1]![2]!;
  await act(() => host.querySelectorAll<HTMLButtonElement>('.worktree-list button')[1]!.click());
  expect(signal.aborted).toBe(true);
  await act(() => finish!({ ...status, branch: 'stale' }));
  expect(host.textContent).toContain('Observed branch: feature');
  expect(host.textContent).not.toContain('Observed branch: stale');
});
it('cached palette candidates remain bounded and unavailable entries cannot run', () => {
  const open = vi.fn();
  const rows = Array.from({ length: 300 }, (_, i) => ({
    ...row,
    id: String(i),
  }));
  expect(worktreeEntries(rows, '', false, open)).toHaveLength(20);
  const entry = worktreeEntries([{ ...row, canOpen: false, reason: 'Unavailable' }], '', false, open)[0]!;
  expect(entry.reason).toBe('Unavailable');
  expect(entry.run()).toBe(false);
  expect(open).not.toHaveBeenCalled();
});
it('does not mistake the native echo of its own preference save for another window changing settings', async () => {
  vi.mocked(worktreeApi.save).mockImplementation(async () => {
    events.get('external-tool-preferences')?.({ payload: 3 });
    return { revision: 3, terminal: 'kitty' };
  });
  await mount('terminal');
  await act(() => button('Use as default').click());
  expect(host.querySelector('[role=alert]')).toBeNull();
  expect(host.textContent).toContain('Default terminal saved');
});

it('starts an embedded shell using the selected opaque worktree identity', async () => {
  await mount();
  await act(() => host.querySelectorAll<HTMLButtonElement>('.worktree-list button')[1]!.click());
  await act(() => button('Start embedded terminal in this worktree').click());
  expect(embedded).toHaveBeenCalledWith(other.id);
  expect(worktreeApi.launch).not.toHaveBeenCalled();
});
