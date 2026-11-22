// @vitest-environment jsdom
import { act, createElement, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Exercise the actual desktop write UI.
import GitOperations from '../../../../src/git-operations/GitOperations';
// oxlint-disable-next-line no-restricted-imports -- Native desktop contract.
import { gitApi, type Catalog, type Receipt } from '../../../../src/git-operations/api';
vi.mock('../../../../src/git-operations/api', () => ({
  gitApi: {
    catalog: vi.fn(),
    conflict: vi.fn(),
    rebaseCommits: vi.fn(),
    upstream: vi.fn(async () => ({ upstream: 'origin/main', ahead: 2, behind: 1 })),
    patch: vi.fn(),
    preview: vi.fn(),
    execute: vi.fn(),
    jobs: vi.fn(),
    cancel: vi.fn(),
    inspect: vi.fn(),
    forget: vi.fn(async () => {}),
    draft: vi.fn(),
    saveDraft: vi.fn(),
  },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn(async () => {}) }));
vi.mock('@/client/ui/dialog', () => ({
  Dialog: ({ children }: { children: ReactNode }) => children,
  DialogContent: ({ children, ...props }: { children: ReactNode }) => createElement('div', props, children),
}));
const catalog: Catalog = {
  state: {
    identity: { root: '/repo/main', git: '/repo/main/.git', common: '/repo/main/.git' },
    head: 'a'.repeat(40),
    branch: 'refs/heads/main',
    conflicts: false,
    operations: [],
  },
  files: [{ path: "file 'quotes'\n.ts", index: ' ', worktree: 'M', untracked: false, oldPath: null }],
  branches: ['main', 'feature'],
  tags: [],
  remotes: ['origin'],
  stashes: [],
  locks: [],
  worktrees: [{ path: '/repo/main', branch: 'main', locked: null, prunable: null, bare: false, canModify: true }],
};
const receipt: Receipt = {
  operation: 'native-ticket',
  root: '/repo/main',
  label: 'Stage file',
  status: 'pending',
  message: 'Accepted native job',
  before: catalog.state,
  after: null,
};
let root: Root, host: HTMLDivElement, flushRef: { current: () => Promise<unknown> };
const closed = vi.fn(),
  choose = vi.fn(async () => {}),
  opened = vi.fn(async () => {}),
  toolsOpened = vi.fn(async () => {});
const button = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>('button')].find(
    (b) => b.textContent?.trim() === label || b.getAttribute('aria-label') === label,
  )!;
async function mount(context: string | null = 'context-1', initialSection?: 'Advanced and recovery') {
  await act(() =>
    root.render(
      createElement(GitOperations, {
        context,
        initialSection,
        onClose: closed,
        onChoose: choose,
        onOpen: opened,
        onTools: toolsOpened,
        flushRef,
      }),
    ),
  );
}
function change(input: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto = input.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(input, value);
  input.dispatchEvent(new Event('input', { bubbles: true }));
}
async function selectFile() {
  await act(() => host.querySelector<HTMLButtonElement>('.git-files > button')!.click());
}
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  vi.mocked(gitApi.catalog).mockResolvedValue(catalog);
  vi.mocked(gitApi.jobs).mockResolvedValue([]);
  vi.mocked(gitApi.draft).mockResolvedValue({ revision: 7, message: 'Saved commit draft' });
  vi.mocked(gitApi.saveDraft).mockImplementation(async (_context, revision, message) => ({
    revision: revision + 1,
    message,
  }));
  vi.mocked(gitApi.patch).mockResolvedValue({
    text: 'canonical native diff',
    partial: true,
    hunks: ['@@ -1 +1 @@\n-old\n+new\n'],
  });
  vi.mocked(gitApi.preview).mockResolvedValue({
    id: 'native-ticket',
    label: 'Stage file',
    text: 'Native exact file preview',
    before: catalog.state,
  });
  vi.mocked(gitApi.execute).mockResolvedValue(receipt);
  vi.mocked(gitApi.cancel).mockResolvedValue();
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  flushRef = { current: async () => {} };
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
});
it('browsing only reads; a write needs a native preview and explicit execution', async () => {
  await mount();
  expect(gitApi.preview).not.toHaveBeenCalled();
  expect(gitApi.execute).not.toHaveBeenCalled();
  await selectFile();
  expect(gitApi.patch).toHaveBeenCalledWith('context-1', catalog.files[0]!.path, false, expect.any(AbortSignal));
  await act(() => button('Preview stage file').click());
  expect(gitApi.preview).toHaveBeenCalledWith(
    'context-1',
    { kind: 'stage', path: catalog.files[0]!.path },
    expect.any(AbortSignal),
  );
  expect(gitApi.execute).not.toHaveBeenCalled();
  expect(host.textContent).toContain('Native exact file preview');
  await act(() => button('Stage file').click());
  expect(gitApi.execute).toHaveBeenCalledExactlyOnceWith('native-ticket', 'context-1');
  expect(host.textContent).toContain('Accepted native job');
  expect(gitApi.cancel).not.toHaveBeenCalled();
});
it('hunk actions send a native ordinal and side rather than a rendered patch', async () => {
  await mount();
  await selectFile();
  await act(() => button('Preview stage hunk 1').click());
  expect(gitApi.preview).toHaveBeenCalledWith(
    'context-1',
    { kind: 'hunk', path: catalog.files[0]!.path, staged: false, hunk: 0 },
    expect.any(AbortSignal),
  );
});
it('drafts survive category navigation and flush before dialog close and application exit', async () => {
  await mount();
  await act(() => change(host.querySelector<HTMLTextAreaElement>('textarea')!, 'New checkout-owned draft'));
  await act(() => button('Refs and remotes').click());
  await act(() => button('Changes').click());
  expect(host.querySelector<HTMLTextAreaElement>('textarea')!.value).toBe('New checkout-owned draft');
  await act(() => flushRef.current());
  expect(gitApi.saveDraft).toHaveBeenCalledWith('context-1', 7, 'New checkout-owned draft');
  await act(() => change(host.querySelector<HTMLTextAreaElement>('textarea')!, 'Saved on close'));
  await act(() => button('Close Git operations').click());
  expect(gitApi.saveDraft).toHaveBeenLastCalledWith('context-1', 8, 'Saved on close');
  expect(closed).toHaveBeenCalledOnce();
});
it('save failure keeps the draft and dialog without submitting a commit', async () => {
  await mount();
  await act(() => change(host.querySelector<HTMLTextAreaElement>('textarea')!, 'Keep after CAS failure'));
  vi.mocked(gitApi.saveDraft).mockRejectedValue(Error('Changed in another window'));
  await act(() => button('Close Git operations').click());
  expect(closed).not.toHaveBeenCalled();
  expect(host.querySelector<HTMLTextAreaElement>('textarea')!.value).toBe('Keep after CAS failure');
  expect(host.textContent).toContain('dialog stayed open');
  expect(gitApi.execute).not.toHaveBeenCalled();
});
it('a rejected stale preview remains visible and is never retried automatically', async () => {
  await mount();
  await selectFile();
  await act(() => button('Preview stage file').click());
  vi.mocked(gitApi.execute).mockRejectedValue(Error('Index changed; preview again'));
  await act(() => button('Stage file').click());
  expect(gitApi.execute).toHaveBeenCalledTimes(1);
  expect(host.textContent).toContain('Index changed');
  expect(host.textContent).toContain('Back to action');
});
it('unknown operations remain observable without auto-resuming or reposting', async () => {
  vi.mocked(gitApi.jobs).mockResolvedValue([
    { receipt: { ...receipt, status: 'needsInspection' }, active: false, canCancel: false },
  ]);
  await mount();
  await act(() => button('Operations').click());
  expect(host.textContent).toContain('Inspect result');
  expect(gitApi.execute).not.toHaveBeenCalled();
  expect(gitApi.inspect).not.toHaveBeenCalled();
  vi.mocked(gitApi.inspect).mockResolvedValue({
    receipt: { ...receipt, status: 'observed', message: 'Attribution remains unknown' },
    current: catalog,
    remoteHead: 'b'.repeat(40),
  });
  await act(() => button('Inspect current state').click());
  expect(host.textContent).toContain('Attribution remains unknown');
  expect(host.textContent).toContain('Current remote head');
  expect(gitApi.execute).not.toHaveBeenCalled();
});
it('init needs no existing review and rejects folder-name traversal before preview', async () => {
  await mount(null);
  expect(gitApi.catalog).not.toHaveBeenCalled();
  expect(gitApi.draft).not.toHaveBeenCalled();
  const input = (label: string) =>
    [...host.querySelectorAll<HTMLInputElement>('input')].find((input) =>
      input.parentElement?.textContent?.startsWith(label),
    )!;
  await act(() => {
    change(input('Parent folder'), '/tmp/parent');
    change(input('New folder name'), '../escape');
  });
  await act(() => button('Preview initialize').click());
  expect(gitApi.preview).not.toHaveBeenCalled();
  await act(() => change(input('New folder name'), "repo 'quotes'"));
  await act(() => button('Preview initialize').click());
  expect(gitApi.preview).toHaveBeenCalledWith(
    null,
    { kind: 'init', branch: 'main', path: "/tmp/parent/repo 'quotes'" },
    expect.any(AbortSignal),
  );
  expect(gitApi.execute).not.toHaveBeenCalled();
});
it('binary diffs retain whole-file actions without fabricated partial staging', async () => {
  vi.mocked(gitApi.patch).mockResolvedValue({ text: 'Binary file', partial: false, hunks: [] });
  await mount();
  await selectFile();
  expect(button('Preview stage file')).toBeDefined();
  expect(button('Preview stage hunk 1')).toBeUndefined();
  expect(host.textContent).toContain('Partial staging is unavailable');
});
it('worktree lease errors appear before removal can execute', async () => {
  await mount();
  await act(() => button('Worktrees').click());
  vi.mocked(gitApi.preview).mockRejectedValue(Error('Worktree is used by a review or external tool'));
  await act(() => button('Preview remove clean worktree').click());
  expect(host.textContent).toContain('used by a review');
  expect(gitApi.execute).not.toHaveBeenCalled();
});
it('changing native contexts discards the old preview before it can be confirmed', async () => {
  await mount();
  await selectFile();
  await act(() => button('Preview stage file').click());
  vi.mocked(gitApi.catalog).mockResolvedValue({
    ...catalog,
    state: { ...catalog.state, identity: { ...catalog.state.identity, root: '/other' } },
  });
  await act(() =>
    root.render(
      createElement(GitOperations, {
        context: 'context-2',
        onClose: closed,
        onChoose: choose,
        onOpen: opened,
        flushRef,
      }),
    ),
  );
  expect(host.textContent).toContain('/other');
  expect(host.textContent).not.toContain('Native exact file preview');
  expect(gitApi.forget).toHaveBeenCalledWith('native-ticket');
  expect(gitApi.execute).not.toHaveBeenCalled();
});

const recoveryCatalog = (): Catalog => ({
  ...catalog,
  state: { ...catalog.state, conflicts: true, operations: ['MERGE_HEAD'] },
  recovery: {
    operation: 'merge',
    commands: ['abort'],
    conflicts: [{ path: catalog.files[0]!.path, stages: [] }],
    currentCommit: 'b'.repeat(40),
    onto: null,
    branch: null,
    explanation: 'Current checkout is stage 2; incoming merge head is stage 3.',
  },
});
const conflictView = {
  path: catalog.files[0]!.path,
  draftKey: 'd'.repeat(64),
  expected: 'e'.repeat(64),
  supported: true,
  reason: '',
  labels: ['Index base (stage 1)', 'Current checkout (stage 2)', 'Incoming merge head (stage 3)'],
  contents: ['base\n', 'ours\n', 'theirs\n'],
  result: 'before\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> incoming\nafter\n',
};
async function openConflict() {
  vi.mocked(gitApi.catalog).mockResolvedValue(recoveryCatalog());
  vi.mocked(gitApi.conflict).mockResolvedValue(conflictView);
  vi.mocked(gitApi.draft).mockImplementation(async (_ctx, purpose) =>
    purpose ? { revision: 0, message: '' } : { revision: 7, message: 'Commit draft kept' },
  );
  await mount();
  await act(() => button('Advanced and recovery').click());
  await act(() => button(catalog.files[0]!.path).click());
}
it('disk recovery offers only native-valid commands and requires an explicit preview', async () => {
  vi.mocked(gitApi.catalog).mockResolvedValue(recoveryCatalog());
  await mount();
  await act(() => button('Advanced and recovery').click());
  expect(button('Preview continue')).toBeUndefined();
  expect(button('Preview skip')).toBeUndefined();
  await act(() => button('Preview abort').click());
  expect(gitApi.preview).toHaveBeenCalledWith(
    'context-1',
    { kind: 'recover', operation: 'merge', command: 'abort' },
    expect.any(AbortSignal),
  );
  expect(gitApi.execute).not.toHaveBeenCalled();
});
it('per-change conflict choices preserve surrounding text and stage only native-bound edited content', async () => {
  await openConflict();
  const text = host.querySelector<HTMLTextAreaElement>('[aria-label="Conflict result"]')!;
  expect(text.value).toContain('<<<<<<<');
  await act(() => button('Use Current checkout').click());
  expect(text.value).toBe('before\nours\nafter\n');
  await act(() => button('Preview save and stage resolution').click());
  expect(gitApi.saveDraft).toHaveBeenCalledWith(
    'context-1',
    0,
    'before\nours\nafter\n',
    expect.stringContaining('resolution/'),
  );
  expect(gitApi.preview).toHaveBeenCalledWith(
    'context-1',
    { kind: 'resolveText', path: conflictView.path, expected: conflictView.expected, content: 'before\nours\nafter\n' },
    expect.any(AbortSignal),
  );
  expect(gitApi.execute).not.toHaveBeenCalled();
});
it('unsupported conflicts show the external fallback without an editable result or resolve action', async () => {
  await openConflict();
  await act(() => button('Changes').click());
  vi.mocked(gitApi.conflict).mockResolvedValue({
    ...conflictView,
    supported: false,
    reason: 'Binary conflict requires external Git',
  });
  await act(() => button('Advanced and recovery').click());
  await act(() => button(catalog.files[0]!.path).click());
  expect(host.textContent).toContain('Binary conflict requires external Git');
  expect(host.querySelector('[aria-label="Conflict result"]')).toBeNull();
  expect(button('Preview save and stage resolution')).toBeUndefined();
});
it('resolution draft failures retain the editor and block category navigation and app flush', async () => {
  await openConflict();
  await act(() => button('Use both').click());
  vi.mocked(gitApi.saveDraft).mockRejectedValue(Error('Draft changed in another window'));
  await act(() => button('Changes').click());
  expect(host.querySelector('[aria-label="Conflict result"]')).not.toBeNull();
  expect(host.textContent).toContain('Draft changed in another window');
  await expect(flushRef.current()).rejects.toThrow('Draft changed');
  expect(gitApi.preview).not.toHaveBeenCalled();
});
it('interactive rebase submits exact ordered native commit IDs and constrained commands', async () => {
  vi.mocked(gitApi.rebaseCommits).mockResolvedValue([
    { oid: 'a'.repeat(40), subject: 'First' },
    { oid: 'b'.repeat(40), subject: 'Second' },
  ]);
  await mount();
  await act(() => button('Advanced and recovery').click());
  const inputs = [...host.querySelectorAll<HTMLInputElement>('input')];
  await act(() =>
    change(
      inputs.find((i) => i.placeholder === 'main')!,
      'main',
    ),
  );
  await act(() => button('Load interactive plan').click());
  await act(() => button('Move commit 2 earlier').click());
  await act(() => button('Preview interactive rebase').click());
  expect(gitApi.preview).toHaveBeenCalledWith(
    'context-1',
    {
      kind: 'rebaseInteractive',
      upstream: 'main',
      todo: [
        { oid: 'b'.repeat(40), command: 'pick' },
        { oid: 'a'.repeat(40), command: 'pick' },
      ],
    },
    expect.any(AbortSignal),
  );
  expect(gitApi.execute).not.toHaveBeenCalled();
});
it('diff3 marker choices use real side text and preserve manual edits outside each block', async () => {
  // oxlint-disable-next-line no-restricted-imports -- Exercise the desktop conflict marker parser.
  const { conflictChunks } = await import('../../../../src/git-operations/AdvancedOperations');
  const text = 'manual edit\n<<<<<<< ours\nleft\n||||||| base\nold\n=======\nright\n>>>>>>> theirs\ntail';
  const chunks = conflictChunks(text);
  expect(chunks).toHaveLength(1);
  expect(chunks[0]!.current).toBe('left\n');
  expect(chunks[0]!.incoming).toBe('right\n');
  expect(text.slice(0, chunks[0]!.start)).toBe('manual edit\n');
  expect(conflictChunks('<<<<<<< broken\nx\n')).toEqual([]);
});

it('a stale resolution rejection keeps edited text and never retries or stages implicitly', async () => {
  await openConflict();
  await act(() => button('Use Current checkout').click());
  vi.mocked(gitApi.preview).mockRejectedValue(Error('Conflict stages changed; reload'));
  await act(() => button('Preview save and stage resolution').click());
  expect(host.querySelector<HTMLTextAreaElement>('[aria-label="Conflict result"]')!.value).toBe(
    'before\nours\nafter\n',
  );
  expect(host.textContent).toContain('Conflict stages changed; reload');
  expect(gitApi.preview).toHaveBeenCalledTimes(1);
  expect(gitApi.execute).not.toHaveBeenCalled();
});
it('large unmerged inventories render bounded pages and filter without new Git reads', async () => {
  const next = recoveryCatalog();
  next.recovery!.conflicts = Array.from({ length: 150 }, (_, i) => ({ path: `conflict-${i}.ts`, stages: [] }));
  vi.mocked(gitApi.catalog).mockResolvedValue(next);
  await mount();
  await act(() => button('Advanced and recovery').click());
  const rows = () => [...host.querySelectorAll('button')].filter((b) => b.textContent?.startsWith('conflict-'));
  expect(rows()).toHaveLength(100);
  await act(() => button('Next conflicts').click());
  expect(rows()).toHaveLength(50);
  await act(() =>
    change(host.querySelector<HTMLInputElement>('[aria-label="Filter unmerged paths"]')!, 'conflict-130.ts'),
  );
  expect(rows()).toHaveLength(1);
  expect(gitApi.catalog).toHaveBeenCalledTimes(1);
  expect(gitApi.conflict).not.toHaveBeenCalled();
});

it('external recovery tools flush both commit and resolution drafts before handoff', async () => {
  await mount();
  await act(() => change(host.querySelector<HTMLTextAreaElement>('textarea')!, 'Unsent commit draft'));
  await act(() => button('Advanced and recovery').click());
  vi.mocked(gitApi.saveDraft).mockRejectedValue(Error('Commit draft save failed'));
  await act(() => button('Open external recovery tools').click());
  expect(toolsOpened).not.toHaveBeenCalled();
  expect(host.textContent).toContain('Commit draft save failed');
  vi.mocked(gitApi.saveDraft).mockResolvedValue({ revision: 8, message: 'Unsent commit draft' });
  await act(() => button('Open external recovery tools').click());
  expect(gitApi.saveDraft).toHaveBeenLastCalledWith('context-1', 7, 'Unsent commit draft');
  expect(toolsOpened).toHaveBeenCalledOnce();
});

it('opens attention recovery destinations directly without executing a Git action', async () => {
  await mount('context-1', 'Advanced and recovery');
  expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe('Advanced and recovery');
  expect(gitApi.execute).not.toHaveBeenCalled();
});
