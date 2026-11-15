// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import type { Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Test the actual desktop entrypoint.
import '../../../../src/main';
import { invoke } from '@tauri-apps/api/core';

const entry = vi.hoisted(() => ({ node: null as unknown }));
vi.mock('react-dom/client', () => ({
  createRoot: () => ({
    render: (node: unknown) => {
      entry.node = node;
    },
  }),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: () => true }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: async () => '/repository' }));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ onDragDropEvent: async () => vi.fn() }),
}));
vi.mock('../../../../src/transport', () => ({ bindReview: vi.fn() }));
vi.mock('../../../../src/ReviewWorkspace', () => ({ ReviewWorkspace: () => null }));
vi.mock('../../../../src/CodeIntelligence', async () => {
  const { createElement } = await import('react');
  return { CodeIntelligence: () => createElement('button', { 'data-review-action': 'code' }, 'Code intelligence') };
});
vi.mock('../../../../src/OpenInEditor', async () => {
  const { createElement } = await import('react');
  return { OpenInEditor: () => createElement('button', { 'data-review-action': 'editor' }, 'Open in IDE') };
});
let root: Root;
let host: HTMLDivElement;
const button = (name: string) =>
  [...host.querySelectorAll('button')].find(
    (b) => b.textContent?.trim() === name || b.getAttribute('aria-label') === name,
  )!;
beforeEach(async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  let next = 0;
  vi.mocked(invoke)
    .mockReset()
    .mockImplementation(async (command) =>
      command === 'open_review' ? { id: String(++next), root: '/repository' } : undefined,
    );
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {} });
  host = document.createElement('div');
  document.body.appendChild(host);
  const client = await vi.importActual<typeof import('react-dom/client')>('react-dom/client');
  root = client.createRoot(host);
  await act(() => root.render(entry.node as ReactNode));
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
  localStorage.clear();
});

it('removes both review-only actions on Home and never accumulates duplicates on reopen', async () => {
  for (let i = 0; i < 3; i++) {
    await act(() => button('Open repository').click());
    expect(host.querySelectorAll('[data-review-action=code]')).toHaveLength(1);
    expect(host.querySelectorAll('[data-review-action=editor]')).toHaveLength(1);
    await act(() => button('Recent reviews').click());
    expect(host.querySelectorAll('[data-review-action]')).toHaveLength(0);
  }
});
