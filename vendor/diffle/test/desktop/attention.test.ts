// @vitest-environment jsdom
import { act, createElement, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { beforeEach, afterEach, it, expect, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Actual desktop inbox flow.
import AttentionHub from '../../../../src/attention/AttentionHub';
// oxlint-disable-next-line no-restricted-imports -- Native inbox contract and reducer.
import {
  inbox,
  mergeDelta,
  mergeSnapshot,
  type View,
  type Item,
  type Delta,
  type Target,
} from '../../../../src/attention/api';
// oxlint-disable-next-line no-restricted-imports -- Pinned hosting checks adapter.
import { hosting } from '../../../../src/hosting/api';
const events = vi.hoisted(() => new Map<string, (e: { payload: unknown }) => void>());
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, cb: (e: { payload: unknown }) => void) => {
    events.set(name, cb);
    return () => events.delete(name);
  },
}));
vi.mock('../../../../src/attention/api', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  inbox: { reload: vi.fn(), view: vi.fn(), save: vi.fn(), update: vi.fn(), open: vi.fn(async () => {}) },
}));
vi.mock('../../../../src/hosting/api', () => ({ hosting: { read: vi.fn() } }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('@/client/ui/dialog', () => ({
  Dialog: ({ children }: { children: ReactNode }) => children,
  DialogContent: ({ children, ...p }: { children: ReactNode }) => createElement('div', p, children),
}));
const target: Target = {
  kind: 'conflicts',
  tab: 'tab',
  workspace: 'workspace',
  review: null,
  head: null,
  url: null,
  owner: null,
  context: null,
  terminal: null,
};
const item: Item = {
  id: 'item-one',
  scope: 'repo',
  source: 'conflicts',
  title: '2 unresolved conflicts',
  detail: 'checkout',
  created: 1,
  updated: 1,
  resolved: null,
  read: false,
  dismissed: false,
  stale: false,
  paused: false,
  target,
};
const base: View = {
  checking: false,
  lastChecked: null,
  revision: 0,
  preferences: {
    revision: 0,
    enabled: false,
    requested: true,
    checks: true,
    conflicts: true,
    agents: true,
    notifications: false,
    notifyRequested: true,
    notifyChecks: true,
    notifyConflicts: true,
    notifyAgents: true,
    tray: false,
    keepInTray: false,
    localTabs: [],
    repositories: [],
  },
  items: [],
  running: [],
  repositories: [],
  warnings: [],
  error: null,
  trayError: null,
  unread: 0,
  metrics: { cycles: 0, localReads: 0, remoteJobs: 0, notifications: 0 },
};
let host: HTMLDivElement, root: Root;
const button = (label: string) =>
  [...host.querySelectorAll('button')].find(
    (b) => b.textContent?.trim() === label || b.getAttribute('aria-label') === label,
  )!;
async function mount(view: View = base, initialTarget: Target | null = null) {
  vi.mocked(inbox.view).mockResolvedValue(view);
  await act(() => root.render(createElement(AttentionHub, { onClose: () => {}, initialTarget })));
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(inbox.reload).mockImplementation(() => inbox.view());
  events.clear();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
});
it('opens paused without enabling monitoring, discovering credentials or writing preferences', async () => {
  await mount();
  expect(host.textContent).toContain('Monitoring is paused');
  expect(inbox.save).not.toHaveBeenCalled();
  expect(hosting.read).not.toHaveBeenCalled();
  expect(inbox.open).not.toHaveBeenCalled();
});
it('applies ordered deltas once and does not regress from an older snapshot', () => {
  const v = { ...base, revision: 1, items: [item] };
  const d: Delta = { revision: 2, upsert: [{ ...item, read: true }], remove: [], unread: 0, running: [] };
  const merged = mergeDelta(v, d);
  expect(merged.items).toHaveLength(1);
  expect(merged.items[0]!.read).toBe(true);
  expect(mergeDelta(merged, { ...d, revision: 1, upsert: [] })).toBe(merged);
  expect(mergeDelta(merged, { ...d, revision: 3, upsert: [], remove: [item.id] }).items).toHaveLength(0);
});
it('routes an item using native identity only and keeps read and dismiss explicit', async () => {
  const view = { ...base, preferences: { ...base.preferences, enabled: true }, items: [item], unread: 1 };
  await mount(view);
  await act(() => button('Open destination').click());
  expect(inbox.open).toHaveBeenCalledWith(item.id);
  vi.mocked(inbox.update).mockResolvedValue({ ...view, revision: 1, items: [{ ...item, read: true }] });
  await act(() => button('Mark read').click());
  expect(inbox.update).toHaveBeenCalledWith('read', item.id);
  vi.mocked(inbox.update).mockResolvedValue({ ...view, revision: 2, items: [{ ...item, dismissed: true }] });
  await act(() => button('Dismiss').click());
  expect(inbox.update).toHaveBeenCalledWith('dismiss', item.id);
  expect(host.textContent).not.toContain(item.title);
});
it('keeps unavailable observations unconfirmed and cannot open a paused repository', async () => {
  await mount({
    ...base,
    preferences: { ...base.preferences, enabled: true },
    items: [{ ...item, stale: true, paused: true }],
    warnings: ['Source unavailable'],
  });
  await act(() => button('Activity history').click());
  expect(host.textContent).toContain('Unconfirmed');
  expect(host.textContent).toContain('Paused');
  expect(button('Open destination').disabled).toBe(true);
  expect(host.textContent).not.toContain('Resolved');
});
it('preserves unsaved settings across other-window preference events and reloads explicitly', async () => {
  await mount();
  const checkbox = host.querySelector<HTMLInputElement>('input[type=checkbox]')!;
  await act(() => checkbox.click());
  vi.mocked(inbox.view).mockResolvedValue({ ...base, revision: 1 });
  await act(() => events.get('attention-settings')?.({ payload: null }));
  expect(checkbox.checked).toBe(true);
  await act(() => button('Reload inbox preferences').click());
  expect(checkbox.checked).toBe(false);
});
it('fetches checks for the recorded immutable head without opening a review runtime', async () => {
  const review = { site: { provider: 'github' as const, host: 'provider.test' }, repository: 'team/repo', number: 7 };
  const head = 'a'.repeat(40);
  vi.mocked(hosting.read).mockResolvedValue({
    items: [
      { id: 'check', name: 'tests', status: 'failure', sha: head, url: null, kind: 'check', run: null, attempt: null },
    ],
    page: 1,
    next: null,
  });
  await mount(base, { ...target, kind: 'checks', review, head });
  expect(hosting.read).toHaveBeenCalledWith(
    review.site,
    { kind: 'checks', review, sha: head, page: 1 },
    expect.any(AbortSignal),
  );
  expect(host.textContent).toContain(head);
  expect(host.textContent).toContain('failure');
  expect(inbox.open).not.toHaveBeenCalled();
});
it('shows live waiting updates without fetching a full inbox snapshot per event', async () => {
  await mount({ ...base, preferences: { ...base.preferences, enabled: true } });
  const calls = vi.mocked(inbox.view).mock.calls.length;
  await act(() =>
    events.get('attention-changed')?.({
      payload: {
        revision: 1,
        upsert: [{ ...item, source: 'agents', title: 'codex waiting' }],
        remove: [],
        unread: 1,
        running: [],
      },
    }),
  );
  expect(host.textContent).toContain('codex waiting');
  expect(inbox.view).toHaveBeenCalledTimes(calls);
});

it('keeps a delta that arrives while the first native snapshot is still pending', async () => {
  let complete!: (v: View) => void;
  const pending = new Promise<View>((resolve) => {
    complete = resolve;
  });
  vi.mocked(inbox.view).mockReturnValue(pending);
  await act(() => root.render(createElement(AttentionHub, { onClose: () => {} })));
  await act(() =>
    events.get('attention-changed')?.({ payload: { revision: 2, upsert: [item], remove: [], unread: 1, running: [] } }),
  );
  await act(() => complete({ ...base, preferences: { ...base.preferences, enabled: true } }));
  expect(host.textContent).toContain(item.title);
  expect(inbox.view).toHaveBeenCalledTimes(1);
});

it('accepts newer preferences even when a newer item delta overtook the snapshot', () => {
  const current = {
    ...base,
    revision: 2,
    preferences: { ...base.preferences, enabled: true },
    items: [{ ...item, read: true }],
  };
  const snapshot = { ...base, revision: 1, preferences: { ...base.preferences, revision: 1, enabled: false } };
  const merged = mergeSnapshot(current, snapshot);
  expect(merged.revision).toBe(2);
  expect(merged.items[0]!.read).toBe(true);
  expect(merged.preferences.enabled).toBe(false);
  expect(mergeSnapshot(merged, base).preferences.revision).toBe(1);
});

it('clears the dirty guard when monitoring choices are reverted and preserves edits on failed disk reload', async () => {
  const dirty = vi.fn();
  vi.mocked(inbox.view).mockResolvedValue(base);
  await act(() =>
    root.render(createElement(AttentionHub, { onClose: () => {}, preferencesOnly: true, onDirty: dirty })),
  );
  const enabled = host.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  await act(() => enabled.click());
  expect(dirty).toHaveBeenLastCalledWith(true);
  await act(() => enabled.click());
  expect(dirty).toHaveBeenLastCalledWith(false);
  expect(button('Save changes').disabled).toBe(true);
  await act(() => enabled.click());
  vi.mocked(inbox.reload).mockRejectedValueOnce(Error('Invalid saved monitoring document'));
  await act(() => button('Discard and reload').click());
  expect(enabled.checked).toBe(true);
  expect(dirty).toHaveBeenLastCalledWith(true);
  expect(host.textContent).toContain('Invalid saved monitoring document');
  await act(() => button('Discard changes').click());
  expect(enabled.checked).toBe(false);
  expect(dirty).toHaveBeenLastCalledWith(false);
});

it('shows one save acknowledgement and clears it when editing again', async () => {
  vi.mocked(inbox.view).mockResolvedValue(base);
  vi.mocked(inbox.save).mockImplementation(async (preferences) => ({
    ...base,
    preferences: { ...preferences, revision: 1 },
  }));
  await act(() => root.render(createElement(AttentionHub, { onClose: () => {}, preferencesOnly: true })));
  const enabled = host.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  await act(() => enabled.click());
  expect(button('Discard and reload')).toBeDefined();
  await act(() => button('Save changes').click());
  expect(
    [...host.querySelectorAll('[role="status"]')].filter((el) => el.textContent === 'Monitoring preferences saved.'),
  ).toHaveLength(1);
  await act(() => enabled.click());
  expect(host.textContent).not.toContain('Monitoring preferences saved.');
});
