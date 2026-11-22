// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import type { Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Exercise the actual desktop shell.
import '../../../../src/main';
import { invoke } from '@tauri-apps/api/core';
import { confirm } from '@tauri-apps/plugin-dialog';
import { workspaceFixture } from './workspaceFixture.js';
const entry = vi.hoisted(() => ({ node: null as unknown }));
const runtime = vi.hoisted(() => ({
  text: new Map<string, string>(),
  listeners: new Map<string, () => void>(),
  mounted: 0,
}));
const listeners = vi.hoisted(() => new Map<string, (e: { payload: string }) => void>());
const eventTargets = vi.hoisted(() => new Map<string, unknown>());
vi.mock('react-dom/client', () => ({
  createRoot: () => ({
    render: (node: unknown) => {
      entry.node = node;
    },
  }),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  isTauri: () => true,
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (event: string, callback: (e: { payload: string }) => void, options?: unknown) => {
    eventTargets.set(event, options);
    listeners.set(event, callback);
    return () => listeners.delete(event);
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
  confirm: vi.fn(),
}));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({
    label: 'main',
    onDragDropEvent: async () => () => {},
  }),
}));
vi.mock('../../../../src/ReviewWorkspace', async () => {
  const React = await import('react');
  return {
    acknowledgeWorkspaceSaved: () => {},
    initializeWorkspace: (
      id: string,
      _repository: string,
      _owner: string,
      view: { unsent: Record<string, string> },
    ) => {
      if (!runtime.text.has(id)) runtime.text.set(id, view.unsent.draft ?? '');
    },
    captureWorkspace: (id: string) => ({
      unsent: runtime.text.get(id) ? { draft: runtime.text.get(id)! } : {},
      navigation: null,
    }),
    subscribeWorkspace: (id: string, changed: () => void) => {
      runtime.listeners.set(id, changed);
      return () => runtime.listeners.delete(id);
    },
    closeWorkspace: (id: string) => runtime.text.delete(id),
    clearWorkspaceDrafts: (id: string) => runtime.text.set(id, ''),
    ReviewEnvironment: ({ children }: { children: ReactNode }) => children,
    ReviewChrome: () => null,
    ReviewWorkspace: ({ id }: { id: string }) => {
      const [text, setText] = React.useState(runtime.text.get(id) ?? '');
      React.useEffect(() => {
        runtime.mounted++;
        return () => {
          runtime.mounted--;
        };
      }, []);
      return React.createElement('textarea', {
        'aria-label': 'Draft',
        value: text,
        onChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => {
          runtime.text.set(id, e.target.value);
          setText(e.target.value);
          runtime.listeners.get(id)?.();
        },
      });
    },
  };
});
let native: ReturnType<typeof workspaceFixture>, root: Root, host: HTMLDivElement;
const tabs = () => Array.from(host.querySelectorAll<HTMLButtonElement>('[role=tab]'));
const button = (label: string) =>
  Array.from(host.querySelectorAll<HTMLButtonElement>('button')).find(
    (b) => b.getAttribute('aria-label') === label || b.textContent?.trim() === label,
  )!;
async function mount() {
  const client = await vi.importActual<typeof import('react-dom/client')>('react-dom/client');
  root = client.createRoot(host);
  await act(async () => {
    // Resolve the lazy tab rail before waiting for React's Suspense commit.
    // oxlint-disable-next-line no-restricted-imports -- Resolve the actual lazy desktop component.
    await import('../../../../src/workspaces/WorkspaceBar');
    root.render(entry.node as ReactNode);
  });
  expect(tabs()).toHaveLength(20);
}
beforeEach(async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  sessionStorage.clear();
  runtime.text.clear();
  runtime.listeners.clear();
  runtime.mounted = 0;
  listeners.clear();
  native = workspaceFixture();
  for (let i = 0; i < 20; i++)
    native.add({
      path: `/repo-${i}`,
      request: { kind: 'working' },
      label: `Review ${i}`,
    });
  vi.mocked(invoke)
    .mockReset()
    .mockImplementation((command, args) => native.invoke(command, args as Record<string, unknown>));
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: {},
  });
  host = document.createElement('div');
  document.body.appendChild(host);
  await mount();
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
});
it('restores twenty cold tabs without opening any runtime, then mounts only the selected review', async () => {
  expect(runtime.mounted).toBe(0);
  expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'workspace_activate' || c === 'enable_review_lsp')).toBe(
    false,
  );
  await act(() => tabs()[4]!.click());
  expect(runtime.mounted).toBe(1);
  expect(host.querySelectorAll('[aria-label=Draft]')).toHaveLength(1);
  await act(() => tabs()[5]!.click());
  expect(runtime.mounted).toBe(1);
  expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === 'workspace_activate')).toHaveLength(2);
});
it('keeps tab-owned drafts through switching, close/reopen, and metadata-only shell restart', async () => {
  await act(() => tabs()[0]!.click());
  const first = native.document.workspaces[0]!.tabs[0]!.id;
  runtime.text.set(first, 'Repository-specific unsent text');
  runtime.listeners.get(first)?.();
  await act(() => tabs()[1]!.click());
  expect(native.views.get(first)?.unsent.draft).toBe('Repository-specific unsent text');
  await act(() => tabs()[0]!.click());
  expect(host.querySelector<HTMLTextAreaElement>('[aria-label=Draft]')!.value).toBe('Repository-specific unsent text');
  await act(() => button('Close Review 0').click());
  expect(native.document.closed[0]?.tab.id).toBe(first);
  await act(() => button('Reopen closed tab').click());
  expect(host.querySelector<HTMLTextAreaElement>('[aria-label=Draft]')!.value).toBe('Repository-specific unsent text');
  await act(() => button('Recent reviews').click());
  await act(() => root.unmount());
  runtime.text.clear();
  await mount();
  expect(runtime.mounted).toBe(0);
  const index = native.document.workspaces[0]!.tabs.findIndex((t) => t.id === first);
  await act(() => tabs()[index]!.click());
  expect(host.querySelector<HTMLTextAreaElement>('[aria-label=Draft]')!.value).toBe('Repository-specific unsent text');
});
it('keeps the review open when drafts cannot be saved and flushes before allowing quit', async () => {
  await act(() => tabs()[0]!.click());
  const id = native.document.workspaces[0]!.tabs[0]!.id;
  runtime.text.set(id, 'must not disappear');
  vi.mocked(invoke).mockImplementation((command, args) =>
    command === 'workspace_save_view'
      ? Promise.reject(Error('disk full'))
      : native.invoke(command, args as Record<string, unknown>),
  );
  await act(() => button('Close Review 0').click());
  expect(runtime.mounted).toBe(1);
  expect(native.document.closed).toHaveLength(0);
  await act(() => listeners.get('workspace-flush')?.({ payload: 'quit' }));
  expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'workspace_finish_exit')).toBe(false);
  expect(host.textContent).toContain('window stayed open');
  vi.mocked(invoke).mockImplementation((command, args) => native.invoke(command, args as Record<string, unknown>));
  await act(() => listeners.get('workspace-flush')?.({ payload: 'quit' }));
  expect(native.views.get(id)?.unsent.draft).toBe('must not disappear');
  const commands = vi.mocked(invoke).mock.calls.map(([c]) => c);
  expect(commands.lastIndexOf('workspace_finish_exit')).toBeGreaterThan(commands.lastIndexOf('workspace_save_view'));
});

it('saves and unmounts a review moved to another workspace, then restores it on demand', async () => {
  const second = 'b'.repeat(64);
  native.document.workspaces.push({
    id: second,
    name: 'Other',
    tabs: [],
    groups: [],
    active: null,
    recent: [],
  });
  await act(() => listeners.get('workspace-state')?.({ payload: '' }));
  await act(() => tabs()[0]!.click());
  const first = native.document.workspaces[0]!.tabs[0]!.id;
  runtime.text.set(first, 'moved draft');
  await act(() => button('Options for Review 0').click());
  const target = Array.from(document.querySelectorAll('label'))
    .find((l) => l.textContent?.includes('Move to workspace'))!
    .querySelector('select')!;
  await act(() => {
    target.value = second;
    target.dispatchEvent(new Event('change', { bubbles: true }));
  });
  expect(runtime.mounted).toBe(0);
  expect(native.views.get(first)?.unsent.draft).toBe('moved draft');
  const chooser = host.querySelector<HTMLSelectElement>('select[aria-label=Workspace]')!;
  await act(() => {
    chooser.value = second;
    chooser.dispatchEvent(new Event('change', { bubbles: true }));
  });
  expect(runtime.mounted).toBe(0);
  expect(tabs()).toHaveLength(1);
  await act(() => tabs()[0]!.click());
  expect(host.querySelector<HTMLTextAreaElement>('[aria-label=Draft]')!.value).toBe('moved draft');
});

it('reconciles a committed view after a metadata failure without overwriting another revision', async () => {
  await act(() => tabs()[0]!.click());
  const id = native.document.workspaces[0]!.tabs[0]!.id;
  runtime.text.set(id, 'durable before metadata error');
  let fail = true;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    const result = await native.invoke(command, args as Record<string, unknown>);
    if (command === 'workspace_save_view' && fail) {
      fail = false;
      throw Error('metadata write failed after view commit');
    }
    return result;
  });
  await act(() => button('Close Review 0').click());
  expect(native.document.closed).toHaveLength(0);
  expect(native.views.get(id)?.revision).toBe(1);
  await act(() => button('Close Review 0').click());
  expect(native.document.closed[0]?.tab.id).toBe(id);
  expect(native.views.get(id)?.unsent.draft).toBe('durable before metadata error');
});

it('acknowledges only the current exit generation and reports a failed flush to the native barrier', async () => {
  await act(() => tabs()[0]!.click());
  vi.mocked(invoke).mockImplementation((command, args) =>
    command === 'workspace_save_view'
      ? Promise.reject(Error('disk full'))
      : native.invoke(command, args as Record<string, unknown>),
  );
  // The native event owns the generation; a cancelled request cannot approve a later quit.
  await act(() =>
    listeners.get('workspace-flush')?.({
      payload: { action: 'quit', generation: 7 } as unknown as string,
    }),
  );
  expect(vi.mocked(invoke).mock.calls).toContainEqual(['workspace_cancel_exit', { generation: 7 }]);
  await act(() => listeners.get('workspace-exit-cancelled')?.({ payload: '' }));
  vi.mocked(invoke).mockImplementation((command, args) => native.invoke(command, args as Record<string, unknown>));
  await act(() =>
    listeners.get('workspace-flush')?.({
      payload: { action: 'quit', generation: 8 } as unknown as string,
    }),
  );
  expect(vi.mocked(invoke).mock.calls).toContainEqual(['workspace_finish_exit', { action: 'quit', generation: 8 }]);
});

it('ignores a targeted close flush owned by another window', async () => {
  expect(eventTargets.get('workspace-flush')).toEqual({
    target: { kind: 'WebviewWindow', label: 'main' },
  });
  await act(() => tabs()[0]!.click());
  const before = vi.mocked(invoke).mock.calls.length;
  await act(() =>
    listeners.get('workspace-flush')?.({
      payload: {
        owner: 'workspace-peer',
        action: 'close',
        generation: 19,
      } as unknown as string,
    }),
  );
  expect(
    vi
      .mocked(invoke)
      .mock.calls.slice(before)
      .some(([command]) => command === 'workspace_finish_exit' || command === 'workspace_save_view'),
  ).toBe(false);
  expect(runtime.mounted).toBe(1);
  await act(() =>
    listeners.get('workspace-flush')?.({
      payload: {
        owner: 'main',
        action: 'close',
        generation: 20,
      } as unknown as string,
    }),
  );
  expect(vi.mocked(invoke).mock.calls).toContainEqual(['workspace_finish_exit', { action: 'close', generation: 20 }]);
});

it('opens a revalidated linked worktree as a distinct tab while preserving the previous tab draft and fresh trust', async () => {
  const selected = "/repo/linked 'quotes'\n folder";
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    const query = (args as { query?: { kind: string } } | undefined)?.query;
    if (command === 'worktree_read') {
      if (query?.kind === 'open') return { path: selected };
      if (query?.kind === 'status')
        return {
          head: null,
          branch: 'feature',
          upstream: null,
          ahead: null,
          behind: null,
          staged: 0,
          unstaged: 1,
          untracked: 0,
          conflicts: 0,
          shallow: false,
        };
      return {
        root: '/repo-0',
        managed: false,
        items: [
          {
            id: 'native-entry',
            path: '/renderer-cache-is-not-authority',
            head: null,
            branch: 'feature',
            detached: false,
            bare: false,
            locked: 'Keep for editor',
            prunable: null,
            current: false,
            canOpen: true,
            canInspect: true,
            reason: null,
            reviews: [],
          },
        ],
      };
    }
    return native.invoke(command, args as Record<string, unknown>);
  });
  await act(() => tabs()[0]!.click());
  const original = native.document.workspaces[0]!.tabs[0]!.id;
  runtime.text.set(original, 'Retain main-checkout draft');
  runtime.listeners.get(original)?.();
  await act(async () => {
    // oxlint-disable-next-line no-restricted-imports -- Resolve the actual lazy desktop browser.
    await import('../../../../src/worktrees/WorktreesHub');
    button('Repository tools').click();
  });
  await vi.waitFor(async () => {
    await act(async () => {});
    expect(document.querySelector('[role="menuitem"]')).not.toBeNull();
  });
  await act(async () => {
    const item = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((el) =>
      el.textContent?.includes('Worktrees and external tools'),
    )!;
    item.click();
  });
  const open = document.querySelector<HTMLButtonElement>('.worktree-actions .primary')!;
  expect(open).not.toBeNull();
  await act(() => open.click());
  const added = native.document.workspaces[0]!.tabs.at(-1)!;
  expect(added.source.path).toBe(selected);
  expect(added.source.request).toEqual({ kind: 'working' });
  expect(added.id).not.toBe(original);
  expect(native.views.get(original)?.unsent.draft).toBe('Retain main-checkout draft');
  expect(runtime.mounted).toBe(1);
  expect(document.querySelector('.worktree-hub')).toBeNull();
  expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'enable_review_lsp' || c === 'open_external_tool')).toBe(
    false,
  );
});

it('keeps a closing window and its Git commit draft when native draft persistence fails', async () => {
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'git_operation_read')
      return {
        state: {
          identity: {
            root: '/repo-0',
            git: '/repo-0/.git',
            common: '/repo-0/.git',
          },
          head: 'a'.repeat(40),
          branch: 'refs/heads/main',
          operations: [],
          conflicts: false,
        },
        files: [],
        branches: ['main'],
        tags: [],
        remotes: [],
        stashes: [],
        locks: [],
        worktrees: [],
      };
    if (command === 'git_operation_receipts') return [];
    if (command === 'git_operation_draft') {
      if ((args as { message?: string | null }).message != null) throw Error('Git commit draft could not be saved');
      return { revision: 0, message: '' };
    }
    return native.invoke(command, args as Record<string, unknown>);
  });
  await act(() => tabs()[0]!.click());
  await act(async () => {
    // oxlint-disable-next-line no-restricted-imports -- Resolve actual lazy desktop workflow.
    await import('../../../../src/git-operations/GitOperations');
    button('Git operations').click();
  });
  const textarea = document.querySelector<HTMLTextAreaElement>('[aria-label="Commit message"]')!;
  expect(textarea).not.toBeNull();
  await act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(
      textarea,
      'Keep this Git draft during quit',
    );
    textarea.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await act(() =>
    listeners.get('workspace-flush')?.({
      payload: {
        owner: 'main',
        action: 'quit',
        generation: 31,
      } as unknown as string,
    }),
  );
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'workspace_finish_exit')).toBe(false);
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'workspace_cancel_exit')).toBe(true);
  expect(textarea.value).toBe('Keep this Git draft during quit');
  expect(host.textContent).toContain('Git commit draft could not be saved');
  expect(runtime.mounted).toBe(1);
});

it('preserves live shells when terminal close is cancelled and defers termination to the native exit barrier', async () => {
  vi.mocked(invoke).mockImplementation((command, args) =>
    command === 'terminal_list'
      ? (Promise.resolve([{ running: true }]) as never)
      : (native.invoke(command, args as Record<string, unknown>) as never),
  );
  vi.mocked(confirm).mockResolvedValue(false);
  await act(() =>
    listeners.get('workspace-flush')?.({
      payload: { action: 'quit', generation: 101 } as unknown as string,
    }),
  );
  await act(async () => {});
  expect(vi.mocked(invoke).mock.calls).toContainEqual(['workspace_cancel_exit', { generation: 101 }]);
  expect(
    vi.mocked(invoke).mock.calls.some(([c]) => c === 'terminal_close_owner' || c === 'workspace_finish_exit'),
  ).toBe(false);
  vi.mocked(confirm).mockResolvedValue(true);
  await act(() =>
    listeners.get('workspace-flush')?.({
      payload: { action: 'quit', generation: 102 } as unknown as string,
    }),
  );
  await act(async () => {});
  expect(vi.mocked(invoke).mock.calls).toContainEqual(['workspace_finish_exit', { action: 'quit', generation: 102 }]);
  expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'terminal_close_owner')).toBe(false);
});

it('saves Connections independently of invalid unsaved Code intelligence edits', async () => {
  await mount();
  await act(() => button('Preferences').click());
  const tab = (name: string) =>
    [...document.querySelectorAll<HTMLButtonElement>('.preferences-window [role="tab"]')].find((el) =>
      el.textContent?.includes(name),
    )!;
  await act(() => tab('Code intelligence').click());
  const editor = document.querySelector<HTMLTextAreaElement>('.preferences-window textarea')!;
  await act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(editor, 'invalid JSON');
    editor.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await act(() => tab('Connections').click());
  const pane = () => document.querySelector<HTMLElement>('.preferences-window [role="tabpanel"]:not([hidden])')!;
  await act(() => pane().querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
  await act(() => pane().querySelector<HTMLButtonElement>('button.primary')!.click());
  const save = vi.mocked(invoke).mock.calls.find(([command]) => command === 'save_desktop_settings');
  expect(save?.[1]).toMatchObject({ lspCommands: null, githubHosts: null, githubEnabled: true });
  await act(() => tab('Code intelligence').click());
  expect(editor.value).toBe('invalid JSON');
  await act(() => pane().querySelector<HTMLButtonElement>('button.primary')!.click());
  expect(document.querySelector('.preferences-window [role="alert"]')?.textContent).toContain('valid JSON');
  expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'save_desktop_settings')).toHaveLength(1);
});
it('keeps unsaved Preferences through the native all-window quit barrier', async () => {
  await mount();
  await act(() => button('Preferences').click());
  const connections = [...document.querySelectorAll<HTMLButtonElement>('.preferences-window [role="tab"]')].find(
    (el) => el.textContent === 'Connections',
  )!;
  await act(() => connections.click());
  await act(() =>
    document
      .querySelector<HTMLInputElement>('.preferences-window [role="tabpanel"]:not([hidden]) input[type="checkbox"]')!
      .click(),
  );
  await act(() =>
    listeners.get('workspace-flush')?.({ payload: { owner: 'main', action: 'quit', generation: 92 } } as unknown as {
      payload: string;
    }),
  );
  expect(invoke).toHaveBeenCalledWith('workspace_cancel_exit', { generation: 92 });
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'workspace_finish_exit')).toBe(false);
  expect(document.querySelector('.preferences-window [role="alert"]')?.textContent).toContain('Save or discard');
});
