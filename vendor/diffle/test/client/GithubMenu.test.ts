// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { GithubMetadata, Snapshot } from '@/shared/protocol.js';

vi.mock('@/client/api.js', () => ({ api: { github: () => new Promise(() => {}) } }));
const { useStore } = await import('@/client/store.js');
const { GithubMenu } = await import('@/client/header/GithubMenu.js');
const { useKeymap } = await import('@/client/keyboard/useKeymap.js');
const { defineDiffStub } = await import('./views.js');
defineDiffStub();
function App() {
  useKeymap();
  return createElement(GithubMenu);
}
let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  useStore.setState({
    snapshot: {} as Snapshot,
    githubMenuOpen: false,
    github: { status: 'loading' },
    modeMenuOpen: false,
    helpOpen: false,
  });
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
});
const press = (key: string) =>
  document.body.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));

it('o opens repository information during lookup; Escape and outside clicks dismiss it', async () => {
  await act(() => root.render(createElement(App)));
  await act(() => press('o'));
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  expect(document.body.textContent).toContain('Loading…');
  expect(document.body.textContent).not.toContain('No GitHub origin');
  await act(() => press('Escape'));
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  await act(() => document.querySelector('button')!.click());
  await act(() => document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })));
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

it('shows PR metadata from another repository, then the origin when no PR matches', async () => {
  const metadata: GithubMetadata = {
    version: 1,
    repository: 'alice/fork',
    repositoryUrl: 'https://github.com/alice/fork',
    reason: 'The comparison does not match the pull request diff',
    pullRequest: {
      repository: 'upstream/project',
      number: 42,
      title: 'Improve parsing',
      url: 'https://github.com/upstream/project/pull/42',
      state: 'OPEN',
      isDraft: true,
    },
  };
  useStore.setState({ githubMenuOpen: true, github: { status: 'ready', data: metadata } });
  await act(() => root.render(createElement(App)));
  expect(document.body.textContent).toContain('#42 Improve parsing');
  expect(document.body.textContent).toContain('upstream/project');
  expect(document.body.textContent).toContain(metadata.reason);
  expect(document.querySelector(`a[href="${metadata.pullRequest!.url}"]`)).not.toBeNull();
  await act(() =>
    useStore.setState({
      github: {
        status: 'ready',
        data: { ...metadata, pullRequest: null, reason: 'Lookup failed: gh is not installed' },
      },
    }),
  );
  expect(document.body.textContent).toContain('Lookup failed: gh is not installed');
  expect(document.body.textContent).toContain('alice/fork');
  expect(document.querySelector('a')?.href).toBe('https://github.com/alice/fork');
});
