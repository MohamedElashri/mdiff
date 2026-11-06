// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { beforeEach, afterEach, expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- The desktop host lives outside the engine's @ alias.
import { CodeIntelligence } from '../../../../src/CodeIntelligence';
import { useStore } from '@/client/store';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
let root: Root;
let host: HTMLDivElement;
const status = { enabled: true, servers: [], missing: [] };
const button = (text: string) =>
  [...document.querySelectorAll('button')].find(
    (b) => b.getAttribute('aria-label') === text || b.textContent === text,
  )!;

beforeEach(async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.mocked(invoke).mockReset().mockResolvedValue(status);
  useStore.setState({ lsp: { ...status, enabled: false } });
  localStorage.setItem('mdiff.lsp', 'true');
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(() => root.render(createElement(CodeIntelligence, { id: 'one', root: '/review' })));
});

afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
  localStorage.clear();
});

it('requires an explicit trust action even when the old global preference was enabled', async () => {
  expect(invoke).not.toHaveBeenCalled();
  await act(() => button('Enable code intelligence').click());
  expect(document.body.textContent).toContain('can run build scripts');
  expect(invoke).not.toHaveBeenCalled();
  await act(() => button('Keep disabled').click());
  expect(invoke).not.toHaveBeenCalled();
  await act(() => button('Enable code intelligence').click());
  await act(() => button('Trust and enable').click());
  expect(invoke).toHaveBeenCalledExactlyOnceWith('enable_review_lsp', { id: 'one' });
  expect(useStore.getState().lsp.enabled).toBe(true);
});

it('requires trust again when the review changes', async () => {
  await act(() => button('Enable code intelligence').click());
  await act(() => button('Trust and enable').click());
  await act(() => root.render(createElement(CodeIntelligence, { key: 'two', id: 'two', root: '/other' })));
  expect(button('Enable code intelligence').disabled).toBe(false);
  expect(invoke).toHaveBeenCalledTimes(1);
});

it('keeps a late result from changing the next review', async () => {
  let resolve!: (value: unknown) => void;
  vi.mocked(invoke).mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  await act(() => button('Enable code intelligence').click());
  await act(() => button('Trust and enable').click());
  await act(() => root.render(null));
  await act(() => resolve(status));
  expect(useStore.getState().lsp.enabled).toBe(false);
});
