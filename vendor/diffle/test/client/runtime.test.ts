// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { createReviewScope, ReviewScopeProvider } from '@/client/runtime.js';
import { ReviewScopePool } from '@/client/scopePool.js';
import { createApi, useApi } from '@/client/api.js';
import { createReviewStore, useReviewStore, useStore } from '@/client/store.js';
import { useKeymap } from '@/client/keyboard/useKeymap.js';
import { CommandProvider, useCommands, useCommandVersion } from '@/client/commands/CommandProvider.js';
import {
  DEFAULT_USER_CONFIG,
  type Snapshot,
  type ServerMessage,
  type GraphResponse,
  type ChangedFile,
} from '@/shared/protocol.js';
import type { ReviewTransport } from '@/client/transport.js';
import { composerKey } from '@/client/model.js';
import { legacyDrafts, assignLegacyDrafts } from '@/client/legacyDrafts.js';
import { defineDiffStub } from './views.js';

defineDiffStub();
const scopes: ReturnType<typeof createReviewScope>[] = [];
afterEach(() => {
  for (const scope of scopes.splice(0)) scope.dispose();
  sessionStorage.clear();
});
function snapshot(root: string): Snapshot {
  return {
    root,
    version: 1,
    review: { kind: 'range', old: 'HEAD', new: 'worktree', base: 'direct', view: { kind: 'whole' } },
    oldSha: 'a'.repeat(40),
    newSha: 'worktree',
    headSha: 'a'.repeat(40),
    follows: 'worktree',
    commentKey: 'working',
    context: 5,
    changed: [],
    tree: ['f.py'],
    commit: null,
    commits: { list: [], total: 0, oldSha: 'a'.repeat(40), newSha: 'worktree' },
    iterations: [],
  };
}
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function fixture(name: string) {
  let listener: ((msg: ServerMessage) => void) | null = null;
  const file = deferred<string>();
  const graph = deferred<GraphResponse>();
  const calls: string[] = [];
  const transport: ReviewTransport = {
    async request(path, init) {
      calls.push(path);
      const route = path.split('?')[0];
      let data: unknown;
      switch (route) {
        case 'api/snapshot':
          data = snapshot(name);
          break;
        case 'api/threads':
        case 'api/viewed':
          data = [];
          break;
        case 'api/config':
          data = DEFAULT_USER_CONFIG;
          break;
        case 'api/lsp/status':
          data = { enabled: false, servers: [], missing: [] };
          break;
        case 'api/moved':
          data = null;
          break;
        case 'api/github':
          data = { version: 1, repository: null, repositoryUrl: null, pullRequest: null, reason: 'offline' };
          break;
        case 'api/file':
          data = { path: 'f.py', binary: false, contents: await file.promise };
          break;
        case 'api/lsp/graph':
          data = await graph.promise;
          break;
        default:
          throw new Error(`Unexpected ${init?.method ?? 'GET'} ${path}`);
      }
      // Intentionally ignore cancellation here: the scope must reject a late transport reply.
      return new Response(JSON.stringify(data), { headers: { 'content-type': 'application/json' } });
    },
    subscribe(message, ready) {
      listener = message;
      queueMicrotask(ready);
      return () => {
        listener = null;
      };
    },
  };
  return {
    transport,
    file,
    graph,
    calls,
    push: (msg: ServerMessage) => listener?.(msg),
    subscribed: () => listener !== null,
  };
}
function scope(f: ReturnType<typeof fixture>, repository: string, owner = 'tab') {
  const value = createReviewScope(f.transport, { repository, composerOwner: owner });
  scopes.push(value);
  return value;
}

it('mounts a scoped command UI inside a shell subscribed to registry changes without a render loop', async () => {
  const a = scope(fixture('/a'), 'repo-a');
  await a.store.getState().boot();
  let renders = 0;
  function ReviewKeys() {
    useKeymap();
    return createElement('div', null, 'Review ready');
  }
  function Shell() {
    useCommandVersion(useCommands());
    renders++;
    return createElement(ReviewScopeProvider, { scope: a }, createElement(ReviewKeys));
  }
  const host = document.createElement('div');
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    await act(() => root.render(createElement(CommandProvider, null, createElement(Shell))));
    expect(host.textContent).toBe('Review ready');
    expect(renders).toBeLessThan(10);
  } finally {
    await act(() => root.unmount());
    host.remove();
  }
});

it('namespaces standalone composers by canonical worktree root before restoring them', async () => {
  const a = fixture('/repo-a');
  const b = fixture('/repo-b');
  let current = a;
  const store = createReviewStore(
    createApi({
      request: (path, init) => current.transport.request(path, init),
      subscribe: (message, ready, lost) => current.transport.subscribe(message, ready, lost),
    }),
  );
  sessionStorage.setItem('diffle:unsent', JSON.stringify({ 'working\0draft\0f.py\0file': 'ambiguous old draft' }));
  await store.getState().boot();
  expect(store.getState().unsent).toEqual({});
  store.getState().setUnsent('working\0draft\0f.py\0file', 'Repository A');
  current = b;
  await store.getState().boot();
  expect(store.getState().unsent).toEqual({});
  current = a;
  await store.getState().boot();
  expect(store.getState().unsent['working\0draft\0f.py\0file']).toBe('Repository A');
  store.dispose();
});

it('rejects authenticated metadata replies that predate credential invalidation', async () => {
  const f = fixture('/a');
  const a = scope(f, 'git-dir-a');
  await a.store.getState().boot();
  const reply = deferred<Response>();
  const original = f.transport.request;
  f.transport.request = (path, init) => (path === 'api/github' ? reply.promise : original(path, init));
  const request = a.store.getState().refreshGithub();
  a.store.getState().invalidateGithub();
  reply.resolve(
    new Response(JSON.stringify({ version: 1, repository: 'secret', repositoryUrl: null, pullRequest: null })),
  );
  await request;
  expect(a.store.getState().github).toEqual({ status: 'idle' });
  expect(a.store.getState().authGeneration).toBe(1);
});

it('isolates interleaved metadata, graph results, events and composer owners at identical versions/paths', async () => {
  const fa = fixture('/a'),
    fb = fixture('/b');
  const a = scope(fa, 'git-dir-a'),
    b = scope(fb, 'git-dir-b');
  await Promise.all([a.store.getState().boot(), b.store.getState().boot()]);
  for (const current of [a, b]) current.store.getState().openFileDraft('f.py');
  const keyA = composerKey(a.store.getState(), { kind: 'draft' })!,
    keyB = composerKey(b.store.getState(), { kind: 'draft' })!;
  expect(keyA).toBe(keyB);
  a.store.getState().setUnsent(keyA, 'Only A');
  b.store.getState().setUnsent(keyB, 'Only B');
  const ga = a.graph.store.getState().reload(),
    gb = b.graph.store.getState().reload();
  fb.graph.resolve({ nodes: [], edges: [], cut: [], inconclusive: 'B' });
  await gb;
  fa.graph.resolve({ nodes: [], edges: [], cut: [], inconclusive: 'A' });
  await ga;
  const stateA = a.graph.store.getState().load,
    stateB = b.graph.store.getState().load;
  expect(stateA.state === 'ready' && stateA.graph.inconclusive).toBe('A');
  expect(stateB.state === 'ready' && stateB.graph.inconclusive).toBe('B');
  const seenA: ServerMessage[] = [],
    seenB: ServerMessage[] = [];
  a.api.connectWs(
    (msg) => seenA.push(msg),
    () => {},
    () => {},
  );
  b.api.connectWs(
    (msg) => seenB.push(msg),
    () => {},
    () => {},
  );
  fa.push({ type: 'snapshot', version: 2 });
  fb.push({ type: 'threads' });
  expect(seenA).toEqual([{ type: 'snapshot', version: 2 }]);
  expect(seenB).toEqual([{ type: 'threads' }]);
  a.dispose();
  expect(fa.subscribed()).toBe(false);
  expect(fb.subscribed()).toBe(true);
  expect(b.store.getState().snapshot?.root).toBe('/b');
  expect(b.store.getState().unsent[keyB]).toBe('Only B');
  const reopened = scope(fixture('/a'), 'git-dir-a');
  await reopened.store.getState().boot();
  reopened.store.getState().openFileDraft('f.py');
  expect(reopened.store.getState().unsent[keyA]).toBe('Only A');
  const duplicate = scope(fixture('/a'), 'git-dir-a', 'another-tab');
  expect(duplicate.store.getState().unsent[keyA]).toBeUndefined();
});

it('cancels a closed owner even if its transport ignores abort, without retrying forever or altering another review', async () => {
  const fa = fixture('/a'),
    fb = fixture('/b'),
    a = scope(fa, 'a'),
    b = scope(fb, 'b');
  const one = a.store.getState().loadFile('f.py', 'new'),
    two = b.store.getState().loadFile('f.py', 'new');
  const rejected = expect(one).rejects.toMatchObject({ name: 'AbortError' });
  a.dispose();
  fa.file.resolve('stale A');
  fb.file.resolve('current B');
  await rejected;
  expect((await two).contents).toBe('current B');
  expect(fa.calls.filter((p) => p.startsWith('api/file'))).toHaveLength(1);
  expect(a.diagnostics()).toEqual({ subscriptions: 0, disposed: true });
  expect(b.diagnostics().disposed).toBe(false);
});

it('binds hooks, buttons and document shortcuts to their explicit review, including an inactive mounted subtree', async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const fa = fixture('/a'),
    fb = fixture('/b'),
    a = scope(fa, 'a'),
    b = scope(fb, 'b');
  const moveA = vi.fn(),
    moveB = vi.fn();
  a.store.setState({ moveCursor: moveA });
  b.store.setState({ moveCursor: moveB });
  function Harness({ name }: { name: string }) {
    const api = useApi(),
      store = useReviewStore();
    const path = useStore((s) => s.activePath);
    useKeymap();
    return createElement(
      'button',
      {
        'data-owner': name,
        onClick: async () => {
          const file = await api.file('f.py', 'new');
          store.getState().setActivePath(file.contents);
        },
      },
      path ?? name,
    );
  }
  const host = document.createElement('div');
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(() =>
    root.render(
      createElement(
        'div',
        null,
        createElement(ReviewScopeProvider, { scope: a, active: true }, createElement(Harness, { name: 'a' })),
        createElement(ReviewScopeProvider, { scope: b, active: false }, createElement(Harness, { name: 'b' })),
      ),
    ),
  );
  await act(() => {
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'j', bubbles: true, cancelable: true }));
  });
  expect(moveA).toHaveBeenCalledOnce();
  expect(moveB).not.toHaveBeenCalled();
  await act(() => {
    host.querySelector<HTMLButtonElement>('[data-owner="b"]')!.click();
    fb.file.resolve('B only');
  });
  expect(b.store.getState().activePath).toBe('B only');
  expect(a.store.getState().activePath).toBeNull();
  await act(() => root.unmount());
  host.remove();
});

it('evicts warm payloads under a shared byte budget while retaining drafts and bounded cold navigation', async () => {
  const pool = new ReviewScopePool(8000);
  const fa = fixture('/a'),
    fb = fixture('/b');
  const a = pool.acquire('a', () => fa.transport, { repository: 'a', composerOwner: 'owner' });
  a.store.getState().setUnsent('working\0draft\0f.py\0file', 'keep me');
  a.store.setState({ contents: { 'f.py': { new: 'a'.repeat(2000) } } });
  const b = pool.acquire('b', () => fb.transport, { repository: 'b', composerOwner: 'owner' });
  b.store.setState({ contents: { 'f.py': { new: 'b'.repeat(3000) } } });
  expect(pool.diagnostics().live).toBe(1);
  expect(a.diagnostics().disposed).toBe(true);
  expect(pool.diagnostics().payloadBytes).toBeLessThanOrEqual(8000);
  const restored = pool.acquire('a2', () => fixture('/a').transport, { repository: 'a', composerOwner: 'owner' });
  expect(restored.store.getState().unsent['working\0draft\0f.py\0file']).toBe('keep me');
  for (let i = 0; i < 75; i++)
    pool.acquire(String(i), () => fixture(String(i)).transport, { repository: String(i), composerOwner: 'owner' });
  expect(pool.diagnostics().live).toBe(2);
  expect(pool.diagnostics().cold).toBeLessThanOrEqual(64);
  pool.dispose();
  expect(pool.diagnostics()).toEqual({ live: 0, cold: 0, payloadBytes: 0 });
});

it('rejects an oversized active payload without recursive graph resets or unbounded cold descriptors', () => {
  const pool = new ReviewScopePool(1000);
  for (let i = 0; i < 75; i++) {
    const current = pool.acquire(String(i), () => fixture(String(i)).transport, {
      repository: String(i),
      composerOwner: 'owner',
    });
    current.store.getState().setUnsent('draft', 'keep this');
    current.graph.store.setState({ focusPath: 'f.py' });
    expect(() => current.store.setState({ contents: { 'f.py': { new: 'x'.repeat(1000) } } })).not.toThrow();
    expect(current.store.getState().error).toContain('payload budget');
    expect(current.store.getState().unsent.draft).toBe('keep this');
    expect(pool.diagnostics().payloadBytes).toBeLessThanOrEqual(1000);
    expect(pool.diagnostics().cold).toBeLessThanOrEqual(64);
  }
  pool.dispose();
});

it('preserves ambiguous legacy drafts until explicitly assigned, and keeps the source for recovery', () => {
  const key = 'working\0draft\0f.py\0file',
    legacy = JSON.stringify({ [key]: 'older draft' });
  sessionStorage.setItem('diffle:unsent', legacy);
  const a = scope(fixture('/a'), 'a');
  expect(a.store.getState().unsent[key]).toBeUndefined();
  const found = legacyDrafts('working');
  expect(found[key]).toBe('older draft');
  assignLegacyDrafts(found, 'a', a.store.getState().setUnsent);
  expect(a.store.getState().unsent[key]).toBe('older draft');
  expect(sessionStorage.getItem('diffle:unsent')).toBe(legacy);
  expect(legacyDrafts('working')).toEqual({});
});

it('preserves composers across payload disposal when WebView storage refuses a write', () => {
  const pool = new ReviewScopePool();
  const identity = { repository: 'storage-failure-review', composerOwner: 'owner' };
  const a = pool.acquire('first', () => fixture('/a').transport, identity);
  const storage = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
    throw new DOMException('Storage quota reached', 'QuotaExceededError');
  });
  try {
    a.store.getState().setUnsent('draft', 'keep despite quota');
    pool.close('first');
    const restored = pool.acquire('second', () => fixture('/a').transport, identity);
    expect(restored.store.getState().unsent.draft).toBe('keep despite quota');
    storage.mockRestore();
    restored.store.getState().setUnsent('draft', '');
  } finally {
    storage.mockRestore();
    pool.dispose();
  }
});

it('restores bounded full-file navigation only against matching contents and never reloads external files', async () => {
  const pool = new ReviewScopePool();
  const fa = fixture('/a');
  const changed: ChangedFile = {
    path: 'f.py',
    status: 'M',
    additions: 1,
    deletions: 1,
    binary: false,
    blob: 'one',
    oldBlob: 'base',
    generated: false,
  };
  const a = pool.acquire('a', () => fa.transport, { repository: 'a', composerOwner: 'tab' });
  await a.store.getState().boot();
  a.store.setState({ snapshot: { ...a.store.getState().snapshot!, changed: [changed] } });
  fa.file.resolve('current file\n');
  await a.store.getState().openFullFile('f.py');
  a.store.setState({
    activePath: 'f.py',
    selection: { id: 'file:f.py@99', range: { start: 1, end: 1 } },
    draft: { path: 'f.py', selection: null },
    paneScroll: 543,
  });
  const saved = pool.capture('a')!;
  expect(saved.navigation.fileView?.path).toBe('f.py');
  expect(JSON.stringify(saved)).not.toContain('current file');
  pool.close('a');
  const fb = fixture('/a');
  fb.file.resolve('current file\n');
  const original = fb.transport.request;
  fb.transport.request = (path, init) =>
    path.split('?')[0] === 'api/snapshot'
      ? Promise.resolve(new Response(JSON.stringify({ ...snapshot('/a'), changed: [changed] })))
      : original(path, init);
  const b = pool.acquire('b', () => fb.transport, { repository: 'a', composerOwner: 'tab' }, saved.navigation);
  await b.store.getState().boot();
  await vi.waitFor(() => expect(b.store.getState().paneScroll).toBe(543));
  expect(b.store.getState().selection?.id).toBe(`file:f.py@${b.store.getState().gens['f.py'] ?? 0}`);
  expect(b.store.getState().selection?.id).not.toBe('file:f.py@99');
  expect(b.store.getState().fileView?.path).toBe('f.py');
  pool.close('b');
  const fc = fixture('/a');
  const request = fc.transport.request;
  fc.transport.request = (path, init) =>
    path.split('?')[0] === 'api/snapshot'
      ? Promise.resolve(new Response(JSON.stringify({ ...snapshot('/a'), changed: [{ ...changed, blob: 'changed' }] })))
      : request(path, init);
  const c = pool.acquire(
    'c',
    () => fc.transport,
    { repository: 'a', composerOwner: 'tab' },
    { ...saved.navigation, fileView: null },
  );
  c.store.getState().setUnsent('orphan line draft', 'keep despite content change');
  await c.store.getState().boot();
  await new Promise<void>((resolve) => queueMicrotask(resolve));
  expect(c.store.getState().selection).toBeNull();
  expect(c.store.getState().paneScroll).toBe(0);
  expect(c.store.getState().unsent['orphan line draft']).toBe('keep despite content change');
  c.store.setState({
    collapsed: Object.fromEntries(Array.from({ length: 10000 }, (_, i) => [`very/long/path/${i}`, true])),
  });
  expect(new TextEncoder().encode(JSON.stringify(pool.capture('c')!.navigation)).length).toBeLessThan(64 * 1024);
  pool.dispose();
});

it('admits one payload per desktop window within a divided aggregate budget', () => {
  const first = new ReviewScopePool(4000, 1),
    second = new ReviewScopePool(4000, 1);
  const a = first.acquire('a', () => fixture('/a').transport, { repository: 'a', composerOwner: 'a' });
  a.store.setState({ contents: { f: { new: 'x'.repeat(1200) } } });
  const b = second.acquire('b', () => fixture('/b').transport, { repository: 'b', composerOwner: 'b' });
  b.store.setState({ contents: { f: { new: 'y'.repeat(1200) } } });
  expect(first.diagnostics().payloadBytes + second.diagnostics().payloadBytes).toBeLessThanOrEqual(8000);
  first.acquire('c', () => fixture('/c').transport, { repository: 'c', composerOwner: 'c' });
  expect(a.diagnostics().disposed).toBe(true);
  expect(b.diagnostics().disposed).toBe(false);
  expect(first.diagnostics().live).toBe(1);
  expect(second.diagnostics().live).toBe(1);
  first.dispose();
  second.dispose();
});
