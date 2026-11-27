// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Exercise the desktop update page.
import { Updates, updates } from '../../../../src/Updates';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (command: string) =>
    command === 'updater_info' ? { version: '0.3.3', supported: true, reason: null } : undefined,
}));
vi.mock('@tauri-apps/plugin-updater', () => ({
  check: async () => ({
    version: '0.3.4',
    body: '**A faster review.**',
    close: async () => {},
  }),
}));

it('renders deferred Markdown release notes after an update becomes available', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(Updates)));
    expect(host.querySelector('.update-release-notes')).toBeNull();

    await act(async () => updates.check());
    await vi.waitFor(async () => {
      await act(async () => {});
      expect(host.querySelector('.update-release-notes strong')?.textContent).toBe('A faster review.');
    });
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});
