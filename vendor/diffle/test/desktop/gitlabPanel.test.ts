// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Desktop component lives outside the engine alias.
import { GitlabPanel } from '../../../../src/GitlabPanel';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
let root: Root;
let host: HTMLDivElement;
const data = (name: string) => ({
  mergeRequest: { title: name, iid: 1, web_url: `https://gitlab.com/group/${name}/-/merge_requests/1` },
  discussions: [],
  drafts: [],
});
const render = (id: string) => act(() => root.render(createElement(GitlabPanel, { id, onClose: vi.fn() })));
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.mocked(invoke).mockReset();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
});

it('removes the previous MR and its actions while the next review is loading', async () => {
  vi.mocked(invoke).mockResolvedValueOnce(data('first'));
  await render('a');
  expect(host.textContent).toContain('first');
  let finish!: (value: unknown) => void;
  vi.mocked(invoke).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await render('b');
  expect(host.textContent).not.toContain('first');
  expect(host.textContent).toContain('Loading merge request');
  expect([...host.querySelectorAll('button')].some((b) => b.textContent === 'Save drafts')).toBe(false);
  await act(() => finish(data('second')));
  expect(host.textContent).toContain('second');
});

it('ignores a late export and refresh from the previous review', async () => {
  let finish!: (value: unknown) => void;
  vi.mocked(invoke).mockImplementation(async (_command, args) => {
    const { id, action } = args as { id: string; action: string };
    if (action === 'export')
      return new Promise((resolve) => {
        finish = resolve;
      });
    return data(id === 'a' ? 'first' : 'second');
  });
  await render('a');
  await act(() => [...host.querySelectorAll('button')].find((b) => b.textContent === 'Save drafts')!.click());
  await render('b');
  expect(host.textContent).toContain('second');
  await act(() => finish({ exported: 1, skipped: 0 }));
  expect(host.textContent).toContain('second');
  expect(host.textContent).not.toContain('first');
  expect(host.textContent).not.toContain('drafts saved');
});
