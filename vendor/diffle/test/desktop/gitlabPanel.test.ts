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
const position = {
  new_path: 'src/example.ts',
  position_type: 'text',
  head_sha: 'abc123',
  new_line: 19,
  line_range: { start: { type: 'new', new_line: 17 }, end: { type: 'new', new_line: 19 } },
};
const preview = {
  version: 1,
  drafts: [{ id: 'inline', path: 'src/example.ts', position }],
  skipped: [{ id: 'stale', path: 'old.ts', reason: 'Choose a new anchor.' }],
};
const button = (name: string) => [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === name)!;
const data = (name: string) => ({
  mergeRequest: {
    title: name,
    iid: 1,
    diff_refs: { head_sha: 'abc123' },
    web_url: `https://gitlab.com/group/${name}/-/merge_requests/1`,
  },
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
  expect([...host.querySelectorAll('button')].some((b) => b.textContent === 'Preview drafts')).toBe(false);
  await act(() => finish(data('second')));
  expect(host.textContent).toContain('second');
});

it('ignores a late export and refresh from the previous review', async () => {
  let finish!: (value: unknown) => void;
  vi.mocked(invoke).mockImplementation(async (_command, args) => {
    const { id, action } = args as { id: string; action: string };
    if (action === 'preview_export') return preview;
    if (action === 'export')
      return new Promise((resolve) => {
        finish = resolve;
      });
    return data(id === 'a' ? 'first' : 'second');
  });
  await render('a');
  await act(() => button('Preview drafts').click());
  expect(host.textContent).toContain('src/example.ts · new L17–new L19');
  expect(host.textContent).toContain('Choose a new anchor.');
  expect(invoke).not.toHaveBeenCalledWith('gitlab_action', expect.objectContaining({ action: 'export' }));
  await act(() => button('Save drafts').click());
  expect(invoke).toHaveBeenCalledWith('gitlab_action', { id: 'a', action: 'export', payload: preview });
  await render('b');
  expect(host.textContent).toContain('second');
  await act(() => finish({ exported: 1, skipped: 0 }));
  expect(host.textContent).toContain('second');
  expect(host.textContent).not.toContain('first');
  expect(host.textContent).not.toContain('drafts saved');
});

it('publishes the confirmed provider positions and leaves unanchored drafts alone', async () => {
  vi.mocked(invoke)
    .mockResolvedValueOnce({
      ...data('first'),
      drafts: [
        { id: 1, note: 'Inline draft', position, publishable: true },
        { id: 2, note: 'Unanchored draft', position: null, publishable: false },
      ],
    })
    .mockResolvedValueOnce({ published: 1 })
    .mockResolvedValueOnce(data('first'));
  await render('a');
  await act(() => button('Publish drafts').click());
  expect(host.textContent).toContain('Confirm publication');
  expect(host.textContent).toContain('no matching diff position');
  expect(invoke).toHaveBeenCalledTimes(1);
  await act(() => button('Confirm and publish drafts').click());
  expect(invoke).toHaveBeenNthCalledWith(2, 'gitlab_action', {
    id: 'a',
    action: 'publish',
    payload: [{ id: 1, position }],
  });
  expect(host.textContent).toContain('1 drafts published');
});
