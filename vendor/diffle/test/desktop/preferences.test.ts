// @vitest-environment jsdom
import { act, createElement, useEffect, useState, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ReviewPreferencesForm } from '@/client/header/SettingsDialog';
// oxlint-disable-next-line no-restricted-imports -- Exercise desktop preference ownership.
import { PreferencesDialog } from '../../../../src/preferences/PreferencesDialog';
// oxlint-disable-next-line no-restricted-imports -- Exercise native preference adapters.
import DesktopReviewPreferences from '../../../../src/preferences/ReviewPreferences';
// oxlint-disable-next-line no-restricted-imports -- Exercise terminal preference transactions.
import TerminalPreferences from '../../../../src/preferences/TerminalPreferences';
// oxlint-disable-next-line no-restricted-imports -- Typed native terminal DTO.
import { terminalApi } from '../../../../src/terminal/api';
// oxlint-disable-next-line no-restricted-imports -- Native external-tool preference authority.
import { worktreeApi } from '../../../../src/worktrees/api';
import { invoke } from '@tauri-apps/api/core';
import type { UserConfig } from '@/shared/protocol';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('../../../../src/terminal/api', () => ({
  terminalApi: { preferences: vi.fn(), save: vi.fn(), start: vi.fn() },
}));
vi.mock('../../../../src/worktrees/api', () => ({
  worktreeApi: { preferences: vi.fn(), save: vi.fn(), launch: vi.fn() },
}));
vi.mock('@/client/ui/dialog', () => ({
  Dialog: ({ children }: { children: ReactNode }) => children,
  DialogContent: ({ children, ...props }: { children: ReactNode }) => createElement('div', props, children),
}));
const config: UserConfig = {
  autoViewed: [],
  contextLines: 5,
  followRefs: 'auto',
  lineWrap: true,
  lspCommands: {},
  githubHosts: {},
};
let root: Root, host: HTMLDivElement;
const dirty = vi.fn();
function button(label: string) {
  return [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!;
}
function input(el: HTMLInputElement | HTMLTextAreaElement, text: string) {
  const prototype = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(el, text);
  el.dispatchEvent(new Event('input', { bubbles: true }));
}
beforeEach(() => {
  vi.clearAllMocks();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
});
it('retains review edits and reports failure instead of confirming a failed save', async () => {
  const save = vi.fn(async () => false),
    saved = vi.fn();
  await act(() =>
    root.render(createElement(ReviewPreferencesForm, { config, saveConfig: save, onDirty: dirty, onSaved: saved })),
  );
  const field = host.querySelector<HTMLInputElement>('[aria-label="Context lines"]')!;
  await act(() => input(field, '9'));
  await act(() => button('Save changes').click());
  expect(save).toHaveBeenCalledWith(expect.objectContaining({ contextLines: 9 }));
  expect(host.querySelector('[role=alert]')?.textContent).toContain('were not saved');
  expect(field.value).toBe('9');
  expect(saved).not.toHaveBeenCalled();
  expect(dirty).toHaveBeenLastCalledWith(true);
});
it('syncs untouched preferences but preserves edits through incoming configuration changes', async () => {
  const save = vi.fn(async () => true);
  const render = (value: UserConfig) =>
    root.render(createElement(ReviewPreferencesForm, { config: value, saveConfig: save, onDirty: dirty }));
  await act(() => render(config));
  await act(() => render({ ...config, contextLines: 11 }));
  const field = host.querySelector<HTMLInputElement>('[aria-label="Context lines"]')!;
  expect(field.value).toBe('11');
  expect(dirty).toHaveBeenLastCalledWith(false);
  await act(() => input(field, '7'));
  await act(() => render({ ...config, contextLines: 14 }));
  expect(field.value).toBe('7');
  expect(dirty).toHaveBeenLastCalledWith(true);
  await act(() => button('Discard changes').click());
  expect(field.value).toBe('14');
});
it('rejects invalid context values without sending or silently clamping them', async () => {
  const save = vi.fn(async () => true);
  await act(() => root.render(createElement(ReviewPreferencesForm, { config, saveConfig: save })));
  await act(() => input(host.querySelector<HTMLInputElement>('[aria-label="Context lines"]')!, '-1'));
  await act(() => button('Save changes').click());
  expect(save).not.toHaveBeenCalled();
  expect(host.textContent).toContain('whole number');
});
it('keeps a visited preference section mounted while switching sections', async () => {
  function Draft() {
    const [value, setValue] = useState('');
    return createElement('input', {
      'aria-label': 'Unsent setting',
      value,
      onChange: (e: React.ChangeEvent<HTMLInputElement>) => setValue(e.target.value),
    });
  }
  function Harness() {
    const [section, setSection] = useState('review');
    return createElement(PreferencesDialog, {
      section,
      onSection: setSection,
      dirty: {},
      error: '',
      onClose: vi.fn(),
      sections: [
        { id: 'review', label: 'Review', content: createElement(Draft) },
        { id: 'appearance', label: 'Appearance', content: createElement('p', null, 'Appearance form') },
      ],
    });
  }
  await act(() => root.render(createElement(Harness)));
  const field = host.querySelector<HTMLInputElement>('[aria-label="Unsent setting"]')!;
  await act(() => input(field, 'keep this edit'));
  await act(() => button('Appearance').click());
  await act(() => button('Review').click());
  expect(host.querySelector('[aria-label="Unsent setting"]')).toBe(field);
  expect(field.value).toBe('keep this edit');
});
it('loads and saves app-wide review preferences without creating a review runtime', async () => {
  vi.mocked(invoke).mockImplementation(async (command, args) =>
    command === 'review_preferences' ? { ...config, ...(args as { settings: object | null }).settings } : undefined,
  );
  await act(() => root.render(createElement(DesktopReviewPreferences, { onDirty: dirty })));
  await act(() => input(host.querySelector<HTMLInputElement>('[aria-label="Context lines"]')!, '12'));
  await act(() => button('Save changes').click());
  expect(invoke).toHaveBeenLastCalledWith('review_preferences', {
    settings: expect.objectContaining({ contextLines: 12 }),
  });
  expect(host.textContent).toContain('Review preferences saved.');
  expect(dirty).toHaveBeenLastCalledWith(false);
  expect(vi.mocked(invoke).mock.calls.every(([command]) => command === 'review_preferences')).toBe(true);
});
it('retains the remaining dirty section after a partial terminal/tool save failure', async () => {
  const prefs = { version: 1, revision: 0, enabled: true, fontSize: 13, height: 280 };
  vi.mocked(terminalApi.preferences).mockResolvedValue(prefs);
  vi.mocked(worktreeApi.preferences).mockResolvedValue({
    editors: [],
    terminals: [{ id: 'iterm', name: 'iTerm' }],
    preferences: { revision: 0, terminal: null },
  });
  vi.mocked(terminalApi.save).mockResolvedValue({ ...prefs, revision: 1, enabled: false });
  vi.mocked(worktreeApi.save)
    .mockRejectedValueOnce(Error('Tool preference conflict'))
    .mockResolvedValue({ revision: 1, terminal: 'iterm' });
  await act(() => root.render(createElement(TerminalPreferences, { onDirty: dirty })));
  await act(() => host.querySelector<HTMLInputElement>('[type=checkbox]')!.click());
  await act(() => {
    const select = host.querySelector<HTMLSelectElement>('[aria-label="Preferred terminal"]')!;
    select.value = 'iterm';
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await act(() => button('Save changes').click());
  expect(host.querySelector('[role=alert]')?.textContent).toContain('conflict');
  expect(dirty).toHaveBeenLastCalledWith(true);
  await act(() => button('Save changes').click());
  expect(terminalApi.save).toHaveBeenCalledOnce();
  expect(worktreeApi.save).toHaveBeenCalledTimes(2);
  expect(dirty).toHaveBeenLastCalledWith(false);
  expect(terminalApi.start).not.toHaveBeenCalled();
  expect(worktreeApi.launch).not.toHaveBeenCalled();
});

it('makes destructive reload explicit and reports reload progress without claiming to save', async () => {
  let fail!: (error: Error) => void;
  const reloading = new Promise<void>((_resolve, reject) => {
    fail = reject;
  });
  await act(() =>
    root.render(
      createElement(ReviewPreferencesForm, {
        config,
        saveConfig: vi.fn(async () => true),
        onReload: () => reloading,
      }),
    ),
  );
  const context = host.querySelector<HTMLInputElement>('[aria-label="Context lines"]')!;
  await act(() => input(context, '17'));
  await act(() => button('Discard and reload').click());
  expect(button('Reloading…')).toBeDefined();
  expect(button('Save changes').disabled).toBe(true);
  expect(host.textContent).not.toContain('Saving…');
  await act(() => fail(Error('Cannot read saved preferences')));
  expect(context.value).toBe('17');
  expect(button('Discard and reload')).toBeDefined();
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('Cannot read saved preferences');
});

it('applies a successful reload even if the saved config renders before the reload promise settles', async () => {
  let accept!: (next: UserConfig) => void;
  let finish!: () => void;
  const loaded = new Promise<void>((resolve) => {
    finish = resolve;
  });
  function Harness() {
    const [value, setValue] = useState(config);
    useEffect(() => {
      accept = setValue;
    }, []);
    return createElement(ReviewPreferencesForm, {
      config: value,
      saveConfig: vi.fn(async () => true),
      onReload: () => loaded,
      onDirty: dirty,
    });
  }
  await act(() => root.render(createElement(Harness)));
  const field = host.querySelector<HTMLInputElement>('[aria-label="Context lines"]')!;
  await act(() => input(field, '17'));
  await act(() => button('Discard and reload').click());
  await act(() => accept({ ...config, contextLines: 11 }));
  expect(field.value).toBe('17');
  await act(() => finish());
  expect(field.value).toBe('11');
  expect(dirty).toHaveBeenLastCalledWith(false);
});
