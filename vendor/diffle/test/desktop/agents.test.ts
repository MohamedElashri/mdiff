// @vitest-environment jsdom
import { act, createElement, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
// oxlint-disable-next-line no-restricted-imports -- Actual desktop agent workflow.
import AgentPanel from '../../../../src/agents/AgentPanel';
// oxlint-disable-next-line no-restricted-imports -- Verify central preference feedback.
import AgentPreferences from '../../../../src/preferences/AgentPreferences';
// oxlint-disable-next-line no-restricted-imports -- Exercise window-owned activity lifecycle.
import { useAgentActivity } from '../../../../src/agents/useActivity';
// oxlint-disable-next-line no-restricted-imports -- Native agent DTOs.
import { agentApi, type View, type Preview } from '../../../../src/agents/api';
const events = vi.hoisted(() => new Map<string, () => void>());
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, cb: () => void) => {
    events.set(name, cb);
    return () => events.delete(name);
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }));
vi.mock('../../../../src/agents/api', () => ({
  agentApi: {
    view: vi.fn(),
    settings: vi.fn(),
    scan: vi.fn(),
    reload: vi.fn(),
    tools: vi.fn(),
    hookConfig: vi.fn(),
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
  version: 1,
  scan: { running: false, completed: null, checked: 0, total: 0, readable: 0, limited: false, error: null },
  roots: ['/source'],
  hookReady: true,
  lastHook: null,
  hookError: null,
  preferences: { revision: 0, enabled: false, hooks: false, codex: null, claude: null, focus: '' },
  activities: [],
  tools: [
    { id: 'codex', path: '/tools/codex', source: 'automatic', error: null },
    { id: 'claude', path: null, source: 'automatic', error: 'Not found' },
  ],
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
  vi.mocked(agentApi.settings).mockResolvedValue(structuredClone(base));
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
  expect(agentApi.unwatch).toHaveBeenCalledWith('review-one', expect.any(String));
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
  await act(() => button('Copy draft finding').click());
  expect(writeText).toHaveBeenCalledWith(expect.stringContaining('User-attributed agent draft: codex @'));
  expect(host.textContent).toContain('Draft finding copied.');
});
it('surfaces missing tools without substituting or launching another agent', async () => {
  vi.mocked(agentApi.preview).mockRejectedValue(Error('Selected agent executable is unavailable'));
  await mount();
  await act(() => button('Prepare agent review').click());
  expect(host.querySelector('[role=alert]')?.textContent).toContain('unavailable');
  expect(launched).not.toHaveBeenCalled();
});

function inputValue(element: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(element, value);
  element.dispatchEvent(new Event('input', { bubbles: true }));
}
it('shows initial loading and offers a retry after a failed initial read', async () => {
  let reject!: (e: Error) => void;
  vi.mocked(agentApi.view).mockImplementationOnce(() => new Promise((_, no) => (reject = no)));
  await mount();
  expect(host.textContent).toContain('Loading agent activity');
  expect(host.textContent).not.toContain('No current agents found');
  await act(() => reject(Error('Could not open repository')));
  expect(host.querySelector('[role=alert]')?.textContent).toContain('Could not open repository');
  await act(() => button('Settings').click());
  expect(host.textContent).toContain('Agent settings could not be loaded');
  expect(host.textContent).not.toContain('Loading agent settings');
  await act(() => button('Retry').click());
  expect(host.querySelector('[role=alert]')).toBeNull();
});
it('Scan now awaits an actual scan and exposes progress and failure', async () => {
  vi.mocked(agentApi.view).mockResolvedValue({ ...base, preferences: { ...base.preferences, enabled: true } });
  let reject!: (e: Error) => void;
  vi.mocked(agentApi.scan).mockImplementationOnce(() => new Promise((_, no) => (reject = no)));
  await mount();
  await act(() => button('Scan now').click());
  expect(agentApi.scan).toHaveBeenCalledOnce();
  expect(host.textContent).toContain('Scanning local processes');
  expect(button('Scanning…').disabled).toBe(true);
  await act(() => reject(Error('Process inventory unavailable')));
  expect(host.querySelector('[role=alert]')?.textContent).toContain('Process inventory unavailable');
  expect(button('Scan now').disabled).toBe(false);
});
it('queues a scan event arriving during a pending read instead of losing the update', async () => {
  let resolve!: (value: View) => void;
  vi.mocked(agentApi.view).mockImplementationOnce(() => new Promise((yes) => (resolve = yes)));
  await mount();
  await act(() => events.get('agent-activity-changed')!());
  expect(agentApi.view).toHaveBeenCalledTimes(1);
  await act(() => resolve(base));
  expect(agentApi.view).toHaveBeenCalledTimes(2);
});
it('checks unsaved executable paths, reports missing tools and preserves edits through activity events', async () => {
  await mount();
  await act(() => button('Settings').click());
  const path = host.querySelectorAll<HTMLInputElement>('.agent-tool-path input')[0]!;
  await act(() => inputValue(path, '/custom/codex'));
  expect(host.textContent).toContain('Unsaved changes');
  await act(() => events.get('agent-activity-changed')!());
  expect(path.value).toBe('/custom/codex');
  vi.mocked(agentApi.tools).mockResolvedValue([
    { id: 'codex', path: '/custom/codex', source: 'configured', error: 'File not found or inaccessible' },
  ]);
  const check = [...host.querySelectorAll<HTMLButtonElement>('.agent-settings button')].find(
    (b) => b.textContent === 'Check installed tools',
  )!;
  await act(() => check.click());
  expect(agentApi.tools).toHaveBeenCalledWith(expect.objectContaining({ codex: '/custom/codex' }));
  expect(host.textContent).toContain('File not found or inaccessible');
  expect(agentApi.save).not.toHaveBeenCalled();
  vi.mocked(agentApi.save).mockResolvedValue({
    ...base,
    version: 2,
    preferences: { ...base.preferences, revision: 1, codex: '/custom/codex' },
  });
  await act(() => button('Save changes').click());
  expect(agentApi.save).toHaveBeenCalledWith(expect.objectContaining({ revision: 0, codex: '/custom/codex' }));
  expect(host.textContent).toContain('Agent preferences saved.');
  expect(button('Save changes').disabled).toBe(true);
});
it('keeps ended observations separate and never offers their terminal as a current session', async () => {
  const row = {
    id: 'live',
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
    start: 's',
    commit: null,
    terminal: null,
  };
  vi.mocked(agentApi.view).mockResolvedValue({
    ...base,
    preferences: { ...base.preferences, enabled: true },
    activities: [row, { ...row, id: 'old', state: 'unavailable', terminal: 'gone' }],
  });
  await mount();
  expect(host.querySelector('.agent-recent summary')?.textContent).toContain('Recent activity (1)');
  expect(host.querySelector('.agent-recent button')).toBeNull();
  expect(host.querySelectorAll('.agent-activity-view > .agent-activity > li')).toHaveLength(1);
});
it('does not replace a newer scan with an older background snapshot', async () => {
  const enabled = { ...base, preferences: { ...base.preferences, enabled: true } };
  vi.mocked(agentApi.view).mockResolvedValue(enabled);
  await mount();
  let resolve!: (v: View) => void;
  vi.mocked(agentApi.view).mockImplementationOnce(() => new Promise((yes) => (resolve = yes)));
  await act(() => events.get('agent-activity-changed')!());
  vi.mocked(agentApi.scan).mockResolvedValue({
    ...enabled,
    version: 5,
    scan: { ...base.scan, completed: 100, readable: 4, checked: 4, total: 4 },
  });
  await act(() => button('Scan now').click());
  await act(() => resolve({ ...enabled, version: 2, scan: { ...base.scan, running: true } }));
  expect(host.textContent).not.toContain('Scanning local processes');
  expect(button('Scan now').disabled).toBe(false);
});

it('keeps unsaved settings across tabs and failed reloads, and blocks accidental dialog close', async () => {
  await mount();
  await act(() => button('Settings').click());
  const path = host.querySelectorAll<HTMLInputElement>('.agent-tool-path input')[0]!;
  await act(() => inputValue(path, '/custom path/codex'));
  await act(() => button('Review').click());
  await act(() => button('Settings').click());
  expect(path.value).toBe('/custom path/codex');
  await act(() => button('Close agents and review').click());
  expect(host.querySelector('[role=alert]')?.textContent).toContain('Save or discard settings');
  expect(host.querySelector('[role=alert] button')).toBeNull();
  vi.mocked(agentApi.reload).mockRejectedValueOnce(Error('Settings unavailable'));
  await act(() => button('Discard and reload').click());
  expect(path.value).toBe('/custom path/codex');
  expect(button('Save changes').disabled).toBe(false);
});
it('copies generated hook configuration rather than a path to an inaccessible file', async () => {
  vi.mocked(agentApi.view).mockResolvedValue({
    ...base,
    hooks: {
      helper: '/hooks/helper',
      codex: '/hooks/codex.json',
      claude: '/hooks/claude.json',
      socket: '/hooks/socket',
    },
  });
  vi.mocked(agentApi.hookConfig).mockResolvedValue('{"hooks":{"SessionStart":[]}}');
  await mount();
  await act(() => button('Settings').click());
  expect(host.textContent).toContain('Local hook reports are disabled.');
  expect(host.textContent).not.toContain('Local hook listener is ready.');
  await act(() => button('Copy Codex configuration').click());
  expect(agentApi.hookConfig).toHaveBeenCalledWith('codex');
  expect(writeText).toHaveBeenCalledWith('{"hooks":{"SessionStart":[]}}');
});

it('starts window-owned observation when enabled without opening the Agents panel and releases it when disabled', async () => {
  function Observer({ context }: { context: string | null }) {
    useAgentActivity(context, true, activity);
    return null;
  }
  await act(() => root.render(createElement(Observer, { context: 'review-one' })));
  expect(agentApi.view).not.toHaveBeenCalled();
  const enabled = { ...base, preferences: { ...base.preferences, enabled: true } };
  vi.mocked(agentApi.settings).mockResolvedValue(enabled);
  vi.mocked(agentApi.view).mockResolvedValue(enabled);
  await act(() => events.get('agent-activity-changed')?.());
  expect(agentApi.view).toHaveBeenCalledWith('review-one', expect.any(String));
  const observer = vi.mocked(agentApi.view).mock.calls[0]![1];
  await act(() => events.get('agent-activity-changed')?.());
  expect(agentApi.view).toHaveBeenCalledOnce();
  vi.mocked(agentApi.settings).mockResolvedValue({ ...base, version: 2 });
  await act(() => events.get('agent-activity-changed')?.());
  expect(agentApi.unwatch).toHaveBeenCalledWith('review-one', observer);
  expect(activity).toHaveBeenLastCalledWith([]);
  await act(() => root.render(createElement(Observer, { context: null })));
  expect(agentApi.view).toHaveBeenCalledOnce();
});
it('keeps a managed Activity panel from replacing the window observer and prevents History from launching a review', async () => {
  await act(() =>
    root.render(
      createElement(AgentPanel, {
        context: 'history-one',
        reviewContext: null,
        activityManaged: true,
        open: true,
        onClose: vi.fn(),
        onActivity: activity,
        onLaunch: launched,
        onTerminal: vi.fn(),
        onExternal: external,
      }),
    ),
  );
  expect(agentApi.settings).toHaveBeenCalled();
  expect(agentApi.view).not.toHaveBeenCalled();
  expect(button('Prepare agent review').disabled).toBe(true);
  await act(() => root.unmount());
  expect(agentApi.unwatch).not.toHaveBeenCalled();
  root = createRoot(host);
});

it('clears the dirty guard when an agent preference is toggled back', async () => {
  await mount();
  await act(() => button('Settings').click());
  const enable = host.querySelector<HTMLInputElement>('.agent-settings input[type="checkbox"]')!;
  await act(() => enable.click());
  expect(button('Save changes').disabled).toBe(false);
  expect(button('Discard and reload')).toBeDefined();
  await act(() => enable.click());
  expect(button('Save changes').disabled).toBe(true);
  expect(button('Reload saved preferences')).toBeDefined();
});
it('keeps a failed preference save visible through background activity updates', async () => {
  await act(() => root.render(createElement(AgentPreferences, { onDirty: vi.fn() })));
  const enable = host.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  await act(() => enable.click());
  vi.mocked(agentApi.save).mockRejectedValueOnce(Error('Changed in another window'));
  await act(() => button('Save changes').click());
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('Changed in another window');
  await act(() => events.get('agent-activity-changed')?.());
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('Changed in another window');
  expect(enable.checked).toBe(true);
});

it('offers comment export without a doomed direct launch for a working-change comparison', async () => {
  await act(() =>
    root.render(
      createElement(AgentPanel, {
        context: 'working-review',
        reviewContext: 'working-review',
        activityManaged: true,
        commitComparison: false,
        pinnedComparison: false,
        initialTab: 'review',
        open: true,
        onClose: vi.fn(),
        onActivity: activity,
        onLaunch: launched,
        onTerminal: vi.fn(),
        onExternal: external,
      }),
    ),
  );
  expect(button('Prepare agent review').disabled).toBe(true);
  expect(button('Copy comment prompt').disabled).toBe(false);
  expect(host.textContent).toContain('Direct launch needs a commit comparison from History');
  const findings = host.querySelector<HTMLTextAreaElement>('[aria-label="Agent findings JSON"]')!;
  await act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(findings, '{"findings":[]}');
    findings.dispatchEvent(new Event('input', { bubbles: true }));
  });
  expect(button('Validate draft findings').disabled).toBe(true);
  expect(findings.value).toBe('{"findings":[]}');
  expect(host.textContent).toContain('Your pasted JSON is kept with this tab');
  expect(agentApi.import).not.toHaveBeenCalled();
  expect(agentApi.preview).not.toHaveBeenCalled();
});

it('keeps comment export available when the selected agent executable is missing', async () => {
  vi.mocked(agentApi.view).mockResolvedValue({
    ...base,
    tools: base.tools.map((tool) => ({ ...tool, path: null, error: 'Not found' })),
  });
  await mount();
  await act(() => button('Review').click());
  expect(button('Prepare agent review').disabled).toBe(true);
  expect(button('Copy comment prompt').disabled).toBe(false);
  expect(host.textContent).toContain('Choose an available agent');
  expect(agentApi.preview).not.toHaveBeenCalled();
});
