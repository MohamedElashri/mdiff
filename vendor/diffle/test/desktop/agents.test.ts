// @vitest-environment jsdom
import { act, createElement, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
// oxlint-disable-next-line no-restricted-imports -- Actual desktop agent workflow.
import AgentPanel from '../../../../src/agents/AgentPanel';
// oxlint-disable-next-line no-restricted-imports -- Native agent DTOs.
import { agentApi, type View, type Preview } from '../../../../src/agents/api';
const events = vi.hoisted(() => new Map<string, () => void>());
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, cb: () => void) => {
    events.set(name, cb);
    return () => events.delete(name);
  },
}));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }));
vi.mock('../../../../src/agents/api', () => ({
  agentApi: {
    view: vi.fn(),
    save: vi.fn(),
    unwatch: vi.fn(async () => {}),
    hooks: vi.fn(),
    preview: vi.fn(),
    import: vi.fn(),
    external: vi.fn(),
  },
}));
vi.mock('@/client/ui/dialog', () => ({
  Dialog: ({ children }: { children: ReactNode }) => children,
  DialogContent: ({ children, ...p }: { children: ReactNode }) => createElement('div', p, children),
}));
const base: View = {
  preferences: { revision: 0, enabled: false, hooks: false, codex: null, claude: null, focus: '' },
  activities: [],
  tools: [{ id: 'codex', path: '/tools/codex' }],
  terminals: [],
  hooks: null,
  error: null,
};
const preview: Preview = {
  id: 'preview',
  executable: '/tools/codex',
  revision: 0,
  root: '/source',
  checkout: '/isolated',
  base: 'a'.repeat(40),
  head: 'b'.repeat(40),
  tool: 'codex',
  profile: 'general',
  prompt: 'Review pinned a..b',
  created: 1,
};
let root: Root, host: HTMLDivElement;
const launched = vi.fn(async () => {}),
  activity = vi.fn(),
  external = vi.fn(async () => {}),
  draft = vi.fn();
function button(label: string) {
  return [...host.querySelectorAll('button')].find(
    (b) => b.textContent?.trim() === label || b.getAttribute('aria-label') === label,
  )!;
}
async function mount(context: string | null = 'review-one') {
  await act(() =>
    root.render(
      createElement(AgentPanel, {
        context,
        open: true,
        onClose: vi.fn(),
        onActivity: activity,
        onLaunch: launched,
        onTerminal: vi.fn(),
        onExternal: external,
        onDraft: draft,
      }),
    ),
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  events.clear();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(agentApi.view).mockResolvedValue(structuredClone(base));
  vi.mocked(agentApi.preview).mockResolvedValue(preview);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
});
it('is opt-in and never launches, installs hooks or saves preferences merely by opening', async () => {
  await mount();
  expect(host.textContent).toContain('Detection is off');
  expect(agentApi.preview).not.toHaveBeenCalled();
  expect(launched).not.toHaveBeenCalled();
  expect(agentApi.hooks).not.toHaveBeenCalled();
  expect(agentApi.save).not.toHaveBeenCalled();
});
it('requires a review context for launch while external fallback remains an explicit action', async () => {
  await mount(null);
  expect(button('Prepare agent review').disabled).toBe(true);
  expect(launched).not.toHaveBeenCalled();
  await mount();
  await act(() => button('Open external terminal').click());
  expect(external).toHaveBeenCalledOnce();
});
it('shows pinned checkout and range before the separate explicit launch action', async () => {
  await mount();
  await act(() => button('Prepare agent review').click());
  expect(agentApi.preview).toHaveBeenCalledWith('review-one', 'codex', 'general');
  expect(host.textContent).toContain('/isolated');
  expect(host.textContent).toContain('a'.repeat(40));
  expect(launched).not.toHaveBeenCalled();
  await act(() => button('Launch codex in embedded terminal').click());
  expect(launched).toHaveBeenCalledWith('preview');
});
it('keeps detected activity distinct from hook state and does not invent commit associations', async () => {
  vi.mocked(agentApi.view).mockResolvedValue({
    ...base,
    preferences: { ...base.preferences, enabled: true },
    activities: [
      {
        id: 'a',
        tool: 'codex',
        session: null,
        parent: null,
        root: '/source',
        context: 'review-one',
        state: 'detected',
        provenance: 'detected',
        started: 1,
        seen: 2,
        pid: 5,
        start: 'unique-start',
        commit: null,
        terminal: null,
      },
    ],
  });
  await mount();
  expect(host.textContent).toContain('Process detected; state is unknown');
  expect(host.textContent).not.toContain('Local hook report');
  expect(activity).toHaveBeenCalledWith([expect.objectContaining({ commit: null, terminal: null })]);
});
it('discards a late preview after changing the review context', async () => {
  let resolve!: (p: Preview) => void;
  vi.mocked(agentApi.preview).mockImplementation(() => new Promise((r) => (resolve = r)));
  await mount();
  await act(() => button('Prepare agent review').click());
  await mount('review-two');
  await act(() => resolve(preview));
  expect(host.textContent).not.toContain('/isolated');
  expect(agentApi.unwatch).toHaveBeenCalledWith('review-one');
});
it('imports only native-validated drafts with pinned attribution and never posts comments', async () => {
  vi.mocked(agentApi.import).mockResolvedValue({
    base: preview.base,
    head: preview.head,
    tool: 'codex',
    findings: [{ path: 'file.ts', start: 2, end: 3, body: 'Draft only' }],
  });
  await mount();
  const textarea = host.querySelector<HTMLTextAreaElement>('[aria-label="Agent findings JSON"]')!;
  await act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(textarea, '{"findings":[]}');
    textarea.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await act(() => button('Validate draft findings').click());
  expect(host.textContent).toContain('Imported draft');
  expect(host.textContent).toContain('Draft only');
  expect(writeText).not.toHaveBeenCalled();
  expect(launched).not.toHaveBeenCalled();
});
it('surfaces missing tools without substituting or launching another agent', async () => {
  vi.mocked(agentApi.preview).mockRejectedValue(Error('Selected agent executable is unavailable'));
  await mount();
  await act(() => button('Prepare agent review').click());
  expect(host.querySelector('[role=alert]')?.textContent).toContain('unavailable');
  expect(launched).not.toHaveBeenCalled();
});
