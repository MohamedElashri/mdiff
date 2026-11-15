// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { beforeEach, afterEach, expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- The desktop host lives outside the engine's @ alias.
import { GithubPanel } from '../../../../src/GithubPanel';
import { useStore } from '@/client/store';
import { api } from '@/client/api';
import { request } from '@/client/transport';
import type { CommentThread, Snapshot } from '@/shared/protocol';

vi.mock('@/client/transport', () => ({ request: vi.fn(), reviewTransport: vi.fn() }));
vi.mock('@/client/api', () => ({ api: { exportToGithub: vi.fn() } }));
let root: Root;
let host: HTMLDivElement;
const close = vi.fn();
const connections = vi.fn();
const refresh = vi.fn(async () => {});
const enter = vi.fn(async () => 'applied' as const);
const context = {
  connected: true,
  host: 'github.com',
  repository: 'user/project',
  headRepository: 'user/project',
  branch: 'feature',
  head: 'abc123',
  base: 'main',
  title: 'A useful change',
  reason: null,
};
const metadata = {
  version: 1,
  repository: 'user/project',
  repositoryUrl: 'https://github.com/user/project',
  reason: null,
  pullRequest: {
    number: 7,
    url: 'https://github.com/user/project/pull/7',
    title: 'A useful change',
    state: 'OPEN' as const,
    isDraft: true,
    repository: 'user/project',
  },
};
const result = {
  posted: 1,
  updated: 0,
  skipped: [{ id: 'stale', reason: 'stale' }],
  url: metadata.pullRequest.url,
  review: 'created' as const,
};
const button = (name: string) => [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === name)!;
async function mount() {
  await act(() => root.render(createElement(GithubPanel, { onClose: close, onConnections: connections })));
}
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  vi.mocked(request).mockResolvedValue(new Response(JSON.stringify(context)));
  vi.mocked(api.exportToGithub).mockResolvedValue(result);
  useStore.setState({
    github: { status: 'ready', data: metadata },
    refreshGithub: refresh,
    enter,
    snapshot: { version: 1 } as Snapshot,
    threads: [
      { id: 'open', resolved: false, stale: false, anchor: { path: 'src/main.ts' } },
      { id: 'stale', resolved: false, stale: true, anchor: { path: 'src/old.ts' } },
      { id: 'resolved', resolved: true, stale: false, anchor: { path: 'src/done.ts' } },
    ] as CommentThread[],
  });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
});

it('transfers open threads with visible results and a GitHub submission link', async () => {
  await mount();
  expect(host.textContent).toContain('1 open thread ready · 1 stale skipped');
  await act(() => button('Save comments to GitHub').click());
  expect(api.exportToGithub).toHaveBeenCalledExactlyOnceWith({});
  expect(host.textContent).toContain('1 added · 0 updated · 1 skipped');
  expect(host.textContent).toContain('src/old.ts: stale');
  expect(host.querySelector('a[href="https://github.com/user/project/pull/7"]')).not.toBeNull();
  expect(useStore.getState().threads).toHaveLength(3);
});

it('keeps errors visible and does not claim that a failed export succeeded', async () => {
  vi.mocked(api.exportToGithub).mockRejectedValue(new Error('The PR head changed'));
  await mount();
  await act(() => button('Save comments to GitHub').click());
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('The PR head changed');
  expect(host.querySelector('[role="status"]')).toBeNull();
  expect(button('Save comments to GitHub').disabled).toBe(false);
});

it('disables comment transfer for an incompatible comparison', async () => {
  useStore.setState({
    github: { status: 'ready', data: { ...metadata, reason: 'GitHub cannot anchor comments to the worktree' } },
  });
  await mount();
  expect(button('Save comments to GitHub').disabled).toBe(true);
  expect(host.textContent).toContain('GitHub cannot anchor comments to the worktree');
  expect(api.exportToGithub).not.toHaveBeenCalled();
});

it('creates a draft using the verified branch and offers to review the result', async () => {
  await mount();
  await act(() => button('Create a pull request').click());
  expect(host.querySelector<HTMLInputElement>('input[type="checkbox"]')?.checked).toBe(true);
  vi.mocked(request).mockResolvedValue(
    new Response(JSON.stringify({ url: metadata.pullRequest.url, number: 7, existing: false })),
  );
  await act(() => button('Create draft PR').click());
  const [, init] = vi.mocked(request).mock.calls.at(-1)!;
  expect(JSON.parse(String(init?.body))).toEqual({
    repository: 'user/project',
    headRepository: 'user/project',
    branch: 'feature',
    head: 'abc123',
    base: 'main',
    title: 'A useful change',
    body: '',
    draft: true,
  });
  expect(host.textContent).toContain('Pull request created: #7');
  expect(button('Create draft PR').closest('fieldset')?.disabled).toBe(true);
  await act(() => button('Review this PR in mdiff').click());
  expect(enter).toHaveBeenCalledExactlyOnceWith({ kind: 'pr', pr: metadata.pullRequest.url });
});

it('explains missing credentials and offers connection settings', async () => {
  vi.mocked(request).mockResolvedValue(
    new Response(
      JSON.stringify({
        ...context,
        connected: false,
        reason: 'Connect GitHub in Preferences → Connections, then reopen this review.',
      }),
    ),
  );
  await mount();
  await act(() => button('Create a pull request').click());
  expect(button('Create draft PR').closest('fieldset')?.disabled).toBe(true);
  expect(host.textContent).toContain('Connect GitHub in Preferences');
  await act(() => button('Connection settings').click());
  expect(connections).toHaveBeenCalledOnce();
});

it('ignores an export result after the comparison changes', async () => {
  let resolve!: (value: typeof result) => void;
  vi.mocked(api.exportToGithub).mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  await mount();
  await act(() => button('Save comments to GitHub').click());
  await act(() => useStore.setState({ snapshot: { version: 2 } as Snapshot }));
  await act(() => resolve(result));
  expect(host.querySelector('[role="status"]')).toBeNull();
});

it('aborts the context read when the panel closes', async () => {
  await mount();
  const signal = vi.mocked(request).mock.calls[0]![1]?.signal;
  await act(() => root.render(null));
  expect(signal?.aborted).toBe(true);
});
