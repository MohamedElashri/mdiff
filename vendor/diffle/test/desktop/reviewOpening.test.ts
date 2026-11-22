// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import type { Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Desktop shell lives outside the engine alias.
import '../../../../src/main';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

import { workspaceFixture } from './workspaceFixture.js';
let native: ReturnType<typeof workspaceFixture>;
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
const events = vi.hoisted(() => [] as string[]);
const entry = vi.hoisted(() => ({ node: null as unknown }));
vi.mock('react-dom/client', () => ({
  createRoot: () => ({
    render: (node: unknown) => {
      entry.node = node;
    },
  }),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: () => true }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ label: 'main', onDragDropEvent: async () => vi.fn() }),
}));
vi.mock('../../../../src/transport', () => ({ bindReview: (id: string) => events.push(`bind:${id}`) }));
vi.mock('../../../../src/ReviewWorkspace', async () => {
  const React = await import('react');
  return {
    closeWorkspace: vi.fn(),
    acknowledgeWorkspaceSaved: () => {},
    initializeWorkspace: vi.fn(),
    captureWorkspace: () => null,
    subscribeWorkspace: () => () => {},
    ReviewChrome: () => null,
    ReviewEnvironment: ({ children }: { children: ReactNode }) => children,
    ReviewWorkspace: () => {
      React.useEffect(() => {
        events.push('mount');
        return () => {
          events.push('unmount');
        };
      }, []);
      return React.createElement('textarea', { 'aria-label': 'Unsent draft', defaultValue: 'unsent comment' });
    },
  };
});
vi.mock('../../../../src/OpenInEditor', () => ({ OpenInEditor: () => null }));
vi.mock('../../../../src/CodeIntelligence', () => ({ CodeIntelligence: () => null }));
let root: Root;
let host: HTMLDivElement;
const choose = () => [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Open repository')!;
beforeEach(async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  events.length = 0;
  localStorage.clear();
  native = workspaceFixture();
  vi.mocked(invoke)
    .mockReset()
    .mockImplementation((command, args) => native.invoke(command, args as Record<string, unknown>));
  vi.mocked(open).mockResolvedValue('/first');
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {} });
  host = document.createElement('div');
  document.body.appendChild(host);
  const client = await vi.importActual<typeof import('react-dom/client')>('react-dom/client');
  root = client.createRoot(host);
  await act(() => root.render(entry.node as ReactNode));
  await act(() => choose().click());
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
  localStorage.clear();
});

it('preserves the mounted workspace and its unsent draft after a failed open', async () => {
  const draft = host.querySelector<HTMLTextAreaElement>('[aria-label="Unsent draft"]')!;
  expect(draft).not.toBeNull();
  let fail!: (error: Error) => void;
  native.setActivate(
    async () =>
      new Promise((_resolve, reject) => {
        fail = reject;
      }),
  );
  vi.mocked(open).mockResolvedValue('/invalid');
  await act(() => choose().click());
  expect(host.querySelector('[aria-label="Unsent draft"]')).toBe(draft);
  await act(() => fail(new Error('not a repository')));
  expect(host.querySelector('[aria-label="Unsent draft"]')).toBe(draft);
  expect(draft.value).toBe('unsent comment');
  expect(events.filter((e) => e === 'unmount')).toHaveLength(0);
  expect(host.textContent).toContain('not a repository');
});

it('unmounts the previous workspace before mounting a different scoped review', async () => {
  let finish!: (value: ReturnType<typeof native.activated>) => void;
  native.setActivate(
    async () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  vi.mocked(open).mockResolvedValue('/second');
  await act(() => choose().click());
  expect(host.querySelector('[aria-label="Unsent draft"]')).not.toBeNull();
  await act(() => finish(native.activated(native.document.workspaces[0]!.active!)));
  expect(events).toEqual(['mount', 'unmount', 'mount']);
});
