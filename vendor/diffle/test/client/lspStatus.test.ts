// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { LspStatus } from '@/shared/protocol.js';
const { state } = vi.hoisted(() => ({
  state: {
    lspLogs: {
      server: {
        entries: [{ id: 1, time: 0, source: 'stderr', level: 'warning', message: 'Fixture diagnostic', count: 1 }],
      },
    },
    config: { lineWrap: true },
    watchLspLog: vi.fn(),
  },
}));
vi.mock('@/client/store.js', () => ({ useStore: (select: (s: typeof state) => unknown) => select(state) }));
vi.mock('@/client/clipboard.js', () => ({ copyText: vi.fn() }));
import { copyText } from '@/client/clipboard.js';
import { LspIndicator } from '@/client/header/LspStatus.js';
let host: HTMLDivElement;
let root: Root;
const status: LspStatus = {
  enabled: true,
  servers: [
    {
      name: 'Fixture server',
      command: 'server',
      languages: ['python'],
      joins: [],
      supports: [],
      state: 'ready',
      log: { seq: 1, errors: 0, warnings: 1 },
    },
  ],
  missing: [{ language: 'rust', tried: ['rust-analyzer'] }],
};
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  state.watchLspLog.mockClear();
  host = document.createElement('div');
  host.style.overflow = 'auto';
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
});
async function render(lsp = status) {
  await act(() => root.render(createElement(LspIndicator, { lsp })));
}
async function open() {
  await act(() => {
    const details = host.querySelector('details')!;
    details.open = true;
    details.dispatchEvent(new Event('toggle'));
  });
}
it('opens outside the scrolling toolbar and dismisses with Escape, outside click, and Close', async () => {
  await render();
  expect(document.querySelector('[aria-label="Language server status"]')).toBeNull();
  for (const action of ['inside', 'escape', 'outside', 'close']) {
    await open();
    const panel = document.querySelector<HTMLElement>('[aria-label="Language server status"]')!;
    expect(panel.parentElement).toBe(document.body);
    expect(panel.style.position).toBe('fixed');
    expect(panel.textContent).toContain('Not on PATH');
    await act(() => {
      if (action === 'inside') panel.dispatchEvent(new Event('pointerdown', { bubbles: true }));
      if (action === 'escape') document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      if (action === 'outside') document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }));
      if (action === 'close') panel.querySelector<HTMLButtonElement>('button')!.click();
    });
    expect(host.querySelector('details')!.open).toBe(action === 'inside');
  }
});
it.each(['starting', 'unavailable', 'ready'] as const)('labels the %s state', async (state) => {
  await render({ ...status, servers: [{ ...status.servers[0]!, state }] });
  expect(host.querySelector('summary')!.getAttribute('aria-label')).toBe(
    `Language servers: ${state === 'ready' ? 'logs' : state}`,
  );
});
it.each([true, false])('subscribes only while expanded and reports clipboard success=%s', async (success) => {
  vi.mocked(copyText).mockResolvedValue(success);
  await render();
  expect(state.watchLspLog).not.toHaveBeenCalled();
  await open();
  const details = document.querySelector<HTMLDetailsElement>('[aria-label="Language server status"] details')!;
  await act(() => {
    details.open = true;
    details.dispatchEvent(new Event('toggle'));
  });
  expect(state.watchLspLog).toHaveBeenCalledWith('server', true);
  expect(document.querySelector('[aria-label="server log"]')!.textContent).toContain('Fixture diagnostic');
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="Copy log"]')!.click());
  expect(copyText).toHaveBeenCalledWith(expect.stringContaining('Fixture diagnostic'));
  expect(document.body.textContent).toContain(success ? 'Copied' : 'Copy failed');
  await act(() => document.querySelector<HTMLButtonElement>('[aria-label="Close language server status"]')!.click());
  await act(() => host.querySelector('details')!.dispatchEvent(new Event('toggle')));
  expect(state.watchLspLog).toHaveBeenCalledWith('server', false);
});

it.each([
  [{ notice: { severity: 'warning' as const, message: 'Fixture warning' } }, 'warning'],
  [{ notice: { severity: 'error' as const, message: 'Fixture error' } }, 'error'],
  [{ activity: ['Indexing fixture'] }, 'busy'],
] as const)('prioritizes server notices and activity', async (extra, label) => {
  await render({
    ...status,
    servers: [{ ...status.servers[0]!, ...extra, activity: 'activity' in extra ? [...extra.activity] : undefined }],
  });
  expect(host.querySelector('summary')!.getAttribute('aria-label')).toBe(`Language servers: ${label}`);
  await open();
  expect(document.querySelector('[aria-label="Language server status"]')!.textContent).toContain(
    'notice' in extra ? extra.notice.message : extra.activity[0],
  );
});
