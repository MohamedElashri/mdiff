// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { confirm } from '@tauri-apps/plugin-dialog';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
// oxlint-disable-next-line no-restricted-imports -- Exercise desktop PTY credit and lifecycle.
import { Session, decodeFrame, INPUT_LIMIT, SCROLLBACK, terminalStore } from '../../../../src/terminal/controller';
// oxlint-disable-next-line no-restricted-imports -- Native-only terminal contract.
import { terminalApi, type Info } from '../../../../src/terminal/api';
// oxlint-disable-next-line no-restricted-imports -- Actual optional desktop terminal panel.
import TerminalPanel from '../../../../src/terminal/TerminalPanel';
vi.mock('../../../../src/terminal/api', () => ({
  terminalApi: {
    start: vi.fn(),
    list: vi.fn(async () => []),
    preferences: vi.fn(async () => ({
      version: 1,
      revision: 0,
      enabled: true,
      fontSize: 13,
      height: 280,
    })),
    save: vi.fn(async (p: object) => p),
    ack: vi.fn(async () => {}),
    input: vi.fn(async () => {}),
    resize: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
  },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ confirm: vi.fn() }));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }));
vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    options: Record<string, unknown>;
    element?: HTMLElement;
    callbacks: (() => void)[] = [];
    writes: Uint8Array[] = [];
    parser = { registerOscHandler: vi.fn() };
    constructor(options: Record<string, unknown>) {
      this.options = options;
    }
    loadAddon() {}
    onData() {}
    onBinary() {}
    attachCustomKeyEventHandler() {}
    write(data: Uint8Array, callback: () => void) {
      this.writes.push(data);
      this.callbacks.push(callback);
    }
    resize = vi.fn();
    focus = vi.fn();
    dispose = vi.fn();
    hasSelection() {
      return false;
    }
    getSelection() {
      return '';
    }
    open(host: HTMLElement) {
      this.element = document.createElement('div');
      host.append(this.element);
    }
  },
}));
vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class {
    proposeDimensions() {
      return { cols: 91, rows: 27 };
    }
  },
}));
const info: Info = {
  id: 'owned',
  root: "/repo/linked 'quotes'\n 世界",
  shell: '/bin/zsh',
  pid: 123,
  running: true,
  exit: null,
};
function frame(seq: number, text: string, kind = 0) {
  const data = new TextEncoder().encode(text),
    bytes = new Uint8Array(16 + data.length),
    view = new DataView(bytes.buffer);
  view.setBigUint64(0, BigInt(seq), true);
  view.setUint32(8, kind, true);
  bytes.set(data, 16);
  return bytes.buffer;
}
type MockTerm = { callbacks: (() => void)[]; writes: Uint8Array[] };
let root: Root | null = null,
  host: HTMLDivElement | null = null;
const sessions: Session[] = [];
function session() {
  const s = new Session(vi.fn());
  sessions.push(s);
  return s;
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(async () => {
  if (root) await act(() => root!.unmount());
  root = null;
  host?.remove();
  host = null;
  for (const s of sessions.splice(0)) s.dispose();
  for (const s of terminalStore.sessions()) await terminalStore.close(s.info!.id);
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
it('buffers only startup credit, preserves binary Unicode chunks and ACKs only parsed output', async () => {
  const s = session(),
    term = s.term as unknown as MockTerm;
  s.receive(frame(1, 'héllo 世界'));
  expect(term.writes).toHaveLength(0);
  s.finishConnect(info);
  expect(term.writes).toHaveLength(1);
  expect(terminalApi.ack).not.toHaveBeenCalled();
  expect(new TextDecoder().decode(term.writes[0])).toBe('héllo 世界');
  term.callbacks.shift()!();
  await vi.advanceTimersByTimeAsync(16);
  expect(terminalApi.ack).toHaveBeenCalledWith('owned', 1);
  expect(s.term.options.scrollback).toBe(SCROLLBACK);
  expect(s.term.options.minimumContrastRatio).toBe(4.5);
});
it('coalesces parse acknowledgements and never publishes output through React notifications', async () => {
  const changed = vi.fn(),
    s = new Session(changed);
  sessions.push(s);
  s.finishConnect(info);
  changed.mockClear();
  for (let n = 1; n <= 4; n++) s.receive(frame(n, `${n}`));
  const term = s.term as unknown as MockTerm;
  term.callbacks.splice(0).forEach((f) => f());
  await vi.advanceTimersByTimeAsync(16);
  expect(terminalApi.ack).toHaveBeenCalledTimes(1);
  expect(terminalApi.ack).toHaveBeenCalledWith('owned', 4);
  expect(changed).not.toHaveBeenCalled();
});
it('rejects corrupt or excessive startup output and stops even when native startup replies late', () => {
  expect(() => decodeFrame(new ArrayBuffer(15))).toThrow();
  const s = session();
  s.receive(frame(2, 'bad'));
  s.finishConnect(info);
  expect(terminalApi.close).toHaveBeenCalledWith(info.id);
  expect(s.info!.running).toBe(false);
  const flood = session();
  for (let n = 1; n <= 5; n++) flood.receive(frame(n, 'x'));
  expect(flood.error).toContain('startup credit');
});
it('handles an exit arriving before startup metadata and does not send more input', async () => {
  const s = session();
  s.receive(frame(0, JSON.stringify({ code: 0, message: 'Shell exited.' }), 1));
  s.finishConnect(info);
  s.send(new Uint8Array([13]));
  await Promise.resolve();
  expect(s.info!.running).toBe(false);
  expect(terminalApi.input).not.toHaveBeenCalled();
});
it('rejects an oversized paste atomically and serializes accepted input without replay', async () => {
  const s = session();
  s.finishConnect(info);
  s.send(new Uint8Array(INPUT_LIMIT + 1));
  await Promise.resolve();
  expect(terminalApi.input).not.toHaveBeenCalled();
  expect(s.error).toContain('not sent');
  vi.mocked(terminalApi.input).mockRejectedValueOnce(Error('busy'));
  s.send(new Uint8Array([1]));
  s.send(new Uint8Array([2]));
  await vi.runAllTimersAsync();
  expect(terminalApi.input).toHaveBeenCalledTimes(2);
  expect(s.error).toContain('not queued');
});
it('consumes privileged OSC clipboard/notification sequences without registering app actions', () => {
  const s = session(),
    register = vi.mocked(s.term.parser.registerOscHandler);
  expect(register.mock.calls.map((c) => c[0])).toEqual([52, 9]);
  for (const call of register.mock.calls) expect(call[1]('untrusted')).toBe(true);
  expect(s.term.options.linkHandler).toBeDefined();
});
it('coalesces layout changes and never resizes a hidden or disposed surface', async () => {
  const s = session();
  s.finishConnect(info);
  const surface = document.createElement('div');
  document.body.append(surface);
  Object.defineProperties(surface, {
    clientWidth: { value: 700 },
    clientHeight: { value: 280 },
  });
  s.attach(surface);
  s.resize();
  s.resize();
  await vi.advanceTimersByTimeAsync(80);
  expect(terminalApi.resize).toHaveBeenCalledTimes(1);
  expect(terminalApi.resize).toHaveBeenCalledWith(info.id, 91, 27);
  s.dispose();
  s.resize();
  await vi.advanceTimersByTimeAsync(80);
  expect(terminalApi.resize).toHaveBeenCalledTimes(1);
  surface.remove();
});
it('opens no shell on display, starts explicitly in the context, and allows cancelling tab close', async () => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(() =>
    root!.render(
      createElement(TerminalPanel, {
        hidden: false,
        context: 'review-one',
        onHide: vi.fn(),
        onExternal: vi.fn(),
      }),
    ),
  );
  expect(terminalApi.start).not.toHaveBeenCalled();
  vi.mocked(terminalApi.start).mockResolvedValue(info);
  const button = (text: string) => [...host!.querySelectorAll('button')].find((b) => b.textContent?.trim() === text)!;
  await act(() => button('Start shell').click());
  expect(terminalApi.start).toHaveBeenCalledWith('review-one', null, expect.any(Function));
  vi.mocked(confirm).mockResolvedValue(false);
  await act(() => host!.querySelector<HTMLButtonElement>('[aria-label^="Close terminal"]')!.click());
  expect(terminalApi.close).not.toHaveBeenCalled();
  vi.mocked(confirm).mockResolvedValue(true);
  await act(() => host!.querySelector<HTMLButtonElement>('[aria-label^="Close terminal"]')!.click());
  expect(terminalApi.close).toHaveBeenCalledWith(info.id);
});

it('writes the clipboard only after an explicit copy action with a selection', async () => {
  const s = session();
  await s.copy();
  expect(writeText).not.toHaveBeenCalled();
  vi.spyOn(s.term, 'getSelection').mockReturnValue('selected 世界');
  await s.copy();
  expect(writeText).toHaveBeenCalledWith('selected 世界');
});
it('keeps external tools available with embedded startup disabled', async () => {
  const external = vi.fn(async () => {});
  await terminalStore.init();
  await terminalStore.save({ enabled: false });
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(() =>
    root!.render(
      createElement(TerminalPanel, { hidden: false, context: 'review-one', onHide: vi.fn(), onExternal: external }),
    ),
  );
  const buttons = [...host.querySelectorAll('button')];
  expect(buttons.find((b) => b.textContent === 'Start shell')!.disabled).toBe(true);
  await act(() => buttons.find((b) => b.textContent === 'Open external terminal')!.click());
  expect(external).toHaveBeenCalledOnce();
  expect(terminalApi.start).not.toHaveBeenCalled();
  await act(() => terminalStore.save({ enabled: true }));
});
it('selects a new worktree shell and keeps sessions independent across selection changes', async () => {
  vi.mocked(terminalApi.start)
    .mockResolvedValueOnce(info)
    .mockResolvedValueOnce({ ...info, id: 'second', root: '/repo/another' });
  await terminalStore.start('review-one');
  await terminalStore.start('review-one', 'opaque-worktree');
  expect(terminalStore.selected()).toBe('second');
  terminalStore.select(info.id);
  expect(terminalStore.sessions().map((s) => s.info!.root)).toEqual([info.root, '/repo/another']);
  await expect(terminalStore.start('foreign-context')).rejects.toThrow('two tabs');
});
