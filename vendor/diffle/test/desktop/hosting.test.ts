// @vitest-environment jsdom
import { act, createElement, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Exercise the desktop hosting surface.
import HostingHub from '../../../../src/hosting/HostingHub';
// oxlint-disable-next-line no-restricted-imports -- The native hosting DTOs live in the desktop adapter.
import { hosting, draftKey, type Context, type Details, type Receipt, type Write } from '../../../../src/hosting/api';
const events = vi.hoisted(() => new Map<string, () => void>());
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (event: string, cb: () => void) => {
    events.set(event, cb);
    return () => events.delete(event);
  },
}));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('../../../../src/hosting/api', async (original) => {
  const actual = await original<typeof import('../../../../src/hosting/api')>();
  return { ...actual, hosting: { context: vi.fn(), read: vi.fn(), write: vi.fn() } };
});
vi.mock('@/client/ui/dialog', () => ({
  Dialog: ({ children }: { children: ReactNode }) => children,
  DialogContent: ({ children, ...props }: { children: ReactNode }) => createElement('div', props, children),
}));
const SHA = 'a'.repeat(40),
  OTHER = 'b'.repeat(40);
const review = { site: { provider: 'github' as const, host: 'github.com' }, repository: 'team/repo', number: 7 };
const context: Context = {
  sites: [review.site, { provider: 'gitlab', host: 'gitlab.com' }],
  current: { review, base: OTHER, sha: SHA },
  epoch: 1,
};
const details: Details = {
  review,
  title: 'A useful change',
  body: 'Description',
  author: 'ada',
  state: 'open',
  head: SHA,
  base: OTHER,
  url: 'https://github.com/team/repo/pull/7',
  merged: false,
  draft: false,
  mergeRevision: OTHER,
  mergeStatus: 'clean',
  capabilities: {
    comment: true,
    reply: true,
    resolve: true,
    review: ['comment', 'approve', 'requestChanges'],
    mergeMethods: ['squash'],
  },
};
const receipt: Receipt = {
  operation: 'operation-1',
  status: 'succeeded',
  reviewedSha: SHA,
  url: details.url,
  message: 'Provider accepted',
  remoteId: '1',
};
let root: Root, host: HTMLDivElement;
let drafts: Map<string, string>;
const close = vi.fn(),
  connections = vi.fn(),
  open = vi.fn();
const button = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>('button')].find(
    (b) => b.textContent?.trim() === label || b.getAttribute('aria-label') === label,
  )!;
async function mount(id: string | undefined = 'owned-review') {
  await act(async () =>
    root.render(
      createElement(HostingHub, {
        id,
        getDraft: (k) => drafts.get(k) ?? '',
        setDraft: (k, v) => {
          drafts.set(k, v);
        },
        onClose: close,
        onConnections: connections,
        onOpen: open,
      }),
    ),
  );
}
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  events.clear();
  drafts = new Map([[draftKey(review), 'My saved feedback']]);
  vi.mocked(hosting.context).mockResolvedValue(context);
  vi.mocked(hosting.read).mockImplementation(async (_site, request) => {
    const r = request as { kind: string };
    if (r.kind === 'details') return details;
    if (r.kind === 'reconcile') return receipt;
    return { items: [], page: 1, next: null };
  });
  vi.mocked(hosting.write).mockResolvedValue(receipt);
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});
it('loads metadata without loading checks, jobs, logs or discussions until selected', async () => {
  await mount();
  expect(host.textContent).toContain('A useful change');
  expect((host.querySelector('textarea') as HTMLTextAreaElement).value).toBe('My saved feedback');
  expect(vi.mocked(hosting.read).mock.calls.map(([, r]) => (r as { kind: string }).kind)).toEqual(['details']);
  await act(() => button('Checks').click());
  expect(hosting.read).toHaveBeenLastCalledWith(
    review.site,
    { kind: 'checks', review, sha: SHA, page: 1 },
    expect.any(AbortSignal),
  );
  expect(host.textContent).toContain('not a combined mergeability verdict');
});
it('requires concrete confirmation and pins the owned recorded SHA without touching local feedback', async () => {
  drafts.set('local-thread-composer', 'Local draft');
  await mount();
  await act(() => button('Review submission…').click());
  expect(hosting.write).not.toHaveBeenCalled();
  expect(host.querySelector('[role=alertdialog]')?.textContent).toContain(SHA);
  await act(() => button('Confirm and submit').click());
  const request = vi.mocked(hosting.write).mock.calls[0]![1] as Write;
  expect(request).toMatchObject({
    review,
    expectedHead: SHA,
    action: { kind: 'submit', verdict: 'comment', body: 'My saved feedback' },
  });
  expect(request.operation).toBeTruthy();
  expect(drafts.get('local-thread-composer')).toBe('Local draft');
  expect(drafts.get(draftKey(review))).toBe('');
});
it('disables submission on provider drift and keeps the unsent composer', async () => {
  vi.mocked(hosting.read).mockResolvedValue({ ...details, head: OTHER });
  await mount();
  expect(host.textContent).toContain('provider head changed');
  expect(button('Review submission…').disabled).toBe(true);
  expect(drafts.get(draftKey(review))).toBe('My saved feedback');
});
it('retains uncertain operations across remounts and reconciles instead of reposting', async () => {
  vi.mocked(hosting.write).mockResolvedValue({ ...receipt, status: 'unknown', message: 'Connection lost' });
  await mount();
  await act(() => button('Review submission…').click());
  await act(() => button('Confirm and submit').click());
  const operation = drafts.get(draftKey(review, 'operation'));
  expect(operation).toBe(receipt.operation);
  expect(button('Review submission…').disabled).toBe(true);
  await act(() => root.unmount());
  root = createRoot(host);
  await mount();
  expect(host.textContent).toContain(operation!);
  await act(() => button('Reconcile operation').click());
  expect(hosting.write).toHaveBeenCalledTimes(1);
  expect(hosting.read).toHaveBeenCalledWith(review.site, { kind: 'reconcile', operation });
  expect(drafts.get(draftKey(review, 'operation'))).toBe('');
});
it('discovers requested reviews with bounded pages and opens the provider URL as a new tab', async () => {
  vi.mocked(hosting.context).mockResolvedValue({ ...context, current: null });
  vi.mocked(hosting.read).mockImplementation(async (_s, r) =>
    (r as { kind: string }).kind === 'details' ? details : { items: [details], page: 1, next: 2 },
  );
  await mount(undefined);
  expect(hosting.read).toHaveBeenCalledWith(
    review.site,
    { kind: 'discover', scope: 'requested', page: 1 },
    expect.any(AbortSignal),
  );
  await act(() => button('Next').click());
  expect(hosting.read).toHaveBeenCalledWith(
    review.site,
    { kind: 'discover', scope: 'requested', page: 2 },
    expect.any(AbortSignal),
  );
  await act(() => host.querySelector<HTMLButtonElement>('.hosting-review-row')!.click());
  await act(() => button('Open review in a tab').click());
  expect(open).toHaveBeenCalledWith(review, 'A useful change');
  expect(host.querySelector('textarea')).toBeNull();
});
it('clears authenticated context immediately when connection settings change', async () => {
  await mount();
  vi.mocked(hosting.context).mockResolvedValue({
    sites: [{ provider: 'gitlab', host: 'gitlab.com' }],
    current: null,
    epoch: 2,
  });
  await act(() => events.get('desktop-settings')!());
  expect(host.textContent).not.toContain('My saved feedback');
  expect(host.textContent).not.toContain('A useful change');
  expect(drafts.get(draftKey(review))).toBe('My saved feedback');
});
it('loads jobs and bounded log excerpts only on demand and preserves attempts', async () => {
  vi.mocked(hosting.read).mockImplementation(async (_s, r) => {
    const request = r as { kind: string };
    if (request.kind === 'details') return details;
    if (request.kind === 'checks')
      return {
        items: [
          {
            id: 'run:4:2',
            name: 'Build',
            status: 'failure',
            kind: 'workflow',
            sha: SHA,
            run: 4,
            attempt: 2,
            url: null,
          },
        ],
        page: 1,
        next: null,
      };
    if (request.kind === 'jobs')
      return {
        items: [
          { id: 8, name: 'Tests', status: 'failure', steps: [{ name: 'Run tests', status: 'failure' }], url: null },
        ],
        page: 1,
        next: null,
      };
    return { text: 'Failure details', truncated: true };
  });
  await mount();
  await act(() => button('Checks').click());
  expect(host.textContent).toContain('Attempt 2');
  await act(() => button('Load jobs').click());
  expect(hosting.read).toHaveBeenLastCalledWith(
    review.site,
    { kind: 'jobs', review, sha: SHA, run: 4, attempt: 2, page: 1 },
    expect.any(AbortSignal),
  );
  await act(() => button('Load log excerpt').click());
  expect(host.textContent).toContain('128 KiB · Truncated');
  expect(host.textContent).toContain('Failure details');
});
it('does not poll an unfocused window or discovery list', async () => {
  await mount();
  vi.useFakeTimers();
  vi.spyOn(document, 'hasFocus').mockReturnValue(false);
  const count = vi.mocked(hosting.read).mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(90000);
  });
  expect(hosting.read).toHaveBeenCalledTimes(count);
});
it('aborts a panel read when switching views and when closing the panel', async () => {
  await mount();
  const metadataSignal = vi.mocked(hosting.read).mock.calls[0]![2]!;
  expect(metadataSignal.aborted).toBe(false);
  await act(() => button('Checks').click());
  const checksSignal = vi.mocked(hosting.read).mock.calls.at(-1)![2]!;
  await act(() => button('Conversation').click());
  expect(checksSignal.aborted).toBe(true);
  await act(() => root.unmount());
  root = createRoot(host);
  expect(metadataSignal.aborted).toBe(true);
});
