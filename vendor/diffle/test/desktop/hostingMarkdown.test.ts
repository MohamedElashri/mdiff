// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Exercise provider text independently of a review runtime.
import HostingMarkdown from '../../../../src/hosting/HostingMarkdown';
import { openUrl } from '@tauri-apps/plugin-opener';
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
it('renders provider tables without fetching images or enabling embedded HTML and unsafe links', async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement('div');
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(() =>
    root.render(
      createElement(HostingMarkdown, {
        text: '| Name | State |\n| --- | --- |\n| Build | Passed |\n\n![tracker](https://unrelated.invalid/image)\n\n<script>steal()</script>\n\n[Bad](javascript:alert(1)) [Good](https://github.com/team/repo)',
      }),
    ),
  );
  expect(host.querySelector('table')).not.toBeNull();
  expect(host.querySelector('img')).toBeNull();
  expect(host.querySelector('script')).toBeNull();
  expect(host.querySelectorAll('a')).toHaveLength(1);
  expect(openUrl).not.toHaveBeenCalled();
  await act(() => host.querySelector<HTMLAnchorElement>('a')!.click());
  expect(openUrl).toHaveBeenCalledWith('https://github.com/team/repo');
  await act(() => root.unmount());
  host.remove();
});
