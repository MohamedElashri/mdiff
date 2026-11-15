// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Desktop component lives outside the engine alias.
import { OpenInEditor } from '../../../../src/OpenInEditor';
import { invoke } from '@tauri-apps/api/core';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { api } from '@/client/api';
import { useStore } from '@/client/store';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }));
vi.mock('@/client/api', () => ({ api: { exportComments: vi.fn() } }));
let root: Root;
let host: HTMLDivElement;
const button = (name: string) =>
  [...document.querySelectorAll('button')].find(
    (b) => b.textContent?.trim() === name || b.getAttribute('aria-label') === name,
  )!;
beforeEach(async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  vi.mocked(invoke).mockImplementation(async (command) =>
    command === 'discover_editors' ? { editors: [{ id: 'code', name: 'VS Code' }], temporary: false } : undefined,
  );
  vi.mocked(writeText).mockResolvedValue();
  useStore.setState({ threads: [{ resolved: false }] as never });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(() => root.render(createElement(OpenInEditor, { id: 'a' })));
  await act(() => button('Open in IDE').click());
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
  localStorage.clear();
});

it('does not overwrite the clipboard after the picker is closed', async () => {
  let finish!: (value: string) => void;
  vi.mocked(api.exportComments).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await act(() => button('Copy agent prompt').click());
  await act(() => button('Close editor picker').click());
  await act(() => finish('old review prompt'));
  expect(writeText).not.toHaveBeenCalled();
});

it('does not launch the previous review editor after its component unmounts', async () => {
  let finish!: (value: string) => void;
  vi.mocked(api.exportComments).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await act(() => document.querySelector<HTMLInputElement>('input[type=checkbox]')!.click());
  await act(() => button('VS Code').click());
  await act(() => root.render(null));
  await act(() => finish('old review prompt'));
  expect(writeText).not.toHaveBeenCalled();
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'open_in_editor')).toBe(false);
});

it('allows a new action after reopening and ignores the previous completion', async () => {
  let finish!: (value: string) => void;
  vi.mocked(api.exportComments)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    )
    .mockResolvedValue('new prompt');
  await act(() => button('Copy agent prompt').click());
  await act(() => button('Close editor picker').click());
  await act(() => button('Open in IDE').click());
  expect(button('Copy agent prompt').disabled).toBe(false);
  await act(() => button('Copy agent prompt').click());
  await act(() => finish('old prompt'));
  expect(writeText).toHaveBeenCalledExactlyOnceWith('new prompt');
});

it('still copies and opens the selected editor while the picker is current', async () => {
  vi.mocked(api.exportComments).mockResolvedValue('current review prompt');
  await act(() => document.querySelector<HTMLInputElement>('input[type=checkbox]')!.click());
  await act(() => button('VS Code').click());
  expect(writeText).toHaveBeenCalledExactlyOnceWith('current review prompt');
  expect(invoke).toHaveBeenCalledWith('open_in_editor', { id: 'a', editorId: 'code' });
  expect(document.body.textContent).toContain('VS Code opened.');
  expect(localStorage.getItem('mdiff.editor')).toBe('code');
});
