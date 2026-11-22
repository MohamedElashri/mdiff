import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { terminalApi, type Info, type Preferences, type Exit } from "./api";
export const INPUT_LIMIT = 65536;
export const SCROLLBACK = 1000;
export function decodeFrame(buffer: ArrayBuffer) {
  if (buffer.byteLength < 16 || buffer.byteLength > 8208)
    throw Error("Invalid terminal frame size");
  const view = new DataView(buffer);
  const seq = Number(view.getBigUint64(0, true)),
    kind = view.getUint32(8, true);
  if (
    !Number.isSafeInteger(seq) ||
    ![0, 1].includes(kind) ||
    (kind === 0 && seq === 0)
  )
    throw Error("Invalid terminal frame");
  return { seq, kind, data: new Uint8Array(buffer, 16) };
}
export class Session {
  info: Info | null = null;
  term: Terminal;
  fit: FitAddon;
  error = "";
  disposed = false;
  private failed = false;
  private ackBusy = false;
  private expected = 1;
  private waiting: { seq: number; data: Uint8Array }[] = [];
  private acked = 0;
  private desiredAck = 0;
  private ackTimer: ReturnType<typeof setTimeout> | null = null;
  private inputBytes = 0;
  private inputQueue: Promise<unknown> = Promise.resolve();
  private resizeTimer: ReturnType<typeof setTimeout> | null = null;
  private host: HTMLElement | null = null;
  constructor(private changed: () => void) {
    this.term = new Terminal({
      cols: 80,
      rows: 24,
      scrollback: SCROLLBACK,
      screenReaderMode: true,
      minimumContrastRatio: 4.5,
      allowProposedApi: false,
      allowTransparency: false,
      linkHandler: { activate: () => {}, hover: () => {}, leave: () => {} },
    });
    this.fit = new FitAddon();
    this.term.loadAddon(this.fit);
    this.term.parser.registerOscHandler(52, () => true);
    this.term.parser.registerOscHandler(9, () => true);
    this.term.onData((text) => this.send(new TextEncoder().encode(text)));
    this.term.onBinary((text) =>
      this.send(Uint8Array.from(text, (c) => c.charCodeAt(0))),
    );
    this.term.attachCustomKeyEventHandler((event) => {
      if (
        (event.metaKey || (event.ctrlKey && event.shiftKey)) &&
        event.key.toLowerCase() === "c" &&
        this.term.hasSelection()
      ) {
        void this.copy();
        return false;
      }
      return true;
    });
  }
  connect(info: Info) {
    this.info = info;
    for (const frame of this.waiting) this.write(frame.seq, frame.data);
    this.waiting = [];
    this.changed();
  }
  receive = (buffer: ArrayBuffer) => {
    if (this.disposed) return;
    try {
      const frame = decodeFrame(buffer);
      if (frame.kind === 1) {
        const exit = JSON.parse(new TextDecoder().decode(frame.data)) as Exit;
        if (this.info) this.info = { ...this.info, running: false, exit };
        else this.earlyExit = exit;
        this.changed();
        return;
      }
      if (frame.seq !== this.expected++)
        throw Error("Terminal stream sequence changed");
      if (this.info) this.write(frame.seq, frame.data);
      else {
        if (this.waiting.length >= 4)
          throw Error("Terminal output exceeded its startup credit");
        this.waiting.push(frame);
      }
    } catch (error) {
      this.failed = true;
      this.error = String(error);
      this.changed();
      if (this.info) void terminalApi.close(this.info.id).catch(() => {});
    }
  };
  private earlyExit: Exit | null = null;
  finishConnect(info: Info) {
    if (this.failed) {
      void terminalApi.close(info.id).catch(() => {});
      info = { ...info, running: false };
    }
    this.connect(
      this.earlyExit ? { ...info, running: false, exit: this.earlyExit } : info,
    );
  }
  private write(seq: number, data: Uint8Array) {
    this.term.write(data, () => {
      if (this.disposed) return;
      this.desiredAck = seq;
      this.scheduleAck();
    });
  }
  private scheduleAck() {
    if (this.ackTimer || this.ackBusy || !this.info || this.disposed) return;
    this.ackTimer = setTimeout(() => {
      this.ackTimer = null;
      const seq = this.desiredAck;
      if (seq <= this.acked || !this.info) return;
      this.ackBusy = true;
      void terminalApi
        .ack(this.info.id, seq)
        .then(() => {
          this.acked = Math.max(seq, this.acked);
        })
        .catch((error) => {
          if (!this.disposed) {
            this.error = String(error);
            this.changed();
          }
        })
        .finally(() => {
          this.ackBusy = false;
          if (this.desiredAck > this.acked && !this.error) this.scheduleAck();
        });
    }, 16);
  }
  send(data: Uint8Array) {
    if (this.disposed || !this.info?.running) return;
    if (
      data.length > INPUT_LIMIT ||
      this.inputBytes + data.length > INPUT_LIMIT
    ) {
      this.error =
        "Input is larger than the terminal queue. This input was not sent; paste a smaller part.";
      this.changed();
      return;
    }
    this.inputBytes += data.length;
    this.inputQueue = this.inputQueue
      .catch(() => {})
      .then(async () => {
        try {
          if (!this.disposed && this.info?.running)
            await terminalApi.input(this.info.id, data);
        } catch (error) {
          this.error = `Input was not queued: ${String(error)}`;
          this.changed();
        } finally {
          this.inputBytes -= data.length;
        }
      });
  }
  attach(host: HTMLElement) {
    this.host = host;
    if (!this.term.element) this.term.open(host);
    else host.append(this.term.element);
    this.resize();
  }
  resize() {
    if (this.resizeTimer) clearTimeout(this.resizeTimer);
    this.resizeTimer = setTimeout(() => {
      this.resizeTimer = null;
      if (
        this.disposed ||
        !this.host?.isConnected ||
        !this.host.clientWidth ||
        !this.host.clientHeight
      )
        return;
      try {
        const proposed = this.fit.proposeDimensions();
        if (!proposed) return;
        const cols = Math.max(2, Math.min(300, proposed.cols)),
          rows = Math.max(1, Math.min(120, proposed.rows));
        this.term.resize(cols, rows);
        if (this.info?.running)
          void terminalApi.resize(this.info.id, cols, rows).catch((error) => {
            if (!this.disposed) {
              this.error = String(error);
              this.changed();
            }
          });
      } catch (error) {
        this.error = String(error);
        this.changed();
      }
    }, 80);
  }
  appearance(preferences: Preferences) {
    const style = getComputedStyle(document.documentElement);
    this.term.options.fontFamily =
      style.getPropertyValue("--font-mono").trim() || "monospace";
    this.term.options.fontSize = preferences.fontSize;
    this.term.options.theme = {
      background: style.getPropertyValue("--bg").trim(),
      foreground: style.getPropertyValue("--fg").trim(),
      cursor: style.getPropertyValue("--accent").trim(),
      selectionBackground: style.getPropertyValue("--bg-3").trim(),
    };
    this.resize();
  }
  async copy() {
    const selection = this.term.getSelection();
    if (selection) await writeText(selection);
  }
  dispose() {
    this.disposed = true;
    if (this.ackTimer) clearTimeout(this.ackTimer);
    if (this.resizeTimer) clearTimeout(this.resizeTimer);
    this.term.dispose();
    this.waiting = [];
    this.host = null;
  }
}
let version = 0;
const listeners = new Set<() => void>();
const sessions = new Map<string, Session>();
let selected: string | null = null;
let starting = 0;
let prefs: Preferences = {
  version: 1,
  revision: 0,
  enabled: true,
  fontSize: 13,
  height: 280,
};
let error = "",
  saving = false;
let initPromise: Promise<void> | null = null;
const changed = () => {
  version++;
  for (const listener of listeners) listener();
};
const applyAppearance = () => {
  for (const session of sessions.values()) session.appearance(prefs);
};
export const terminalStore = {
  subscribe: (listener: () => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
  version: () => version,
  sessions: () => [...sessions.values()],
  selected: () => selected,
  select: (id: string) => {
    selected = id;
    changed();
  },
  preferences: () => prefs,
  error: () => error,
  saving: () => saving,
  init: () =>
    (initPromise ??= (async () => {
      prefs = await terminalApi.preferences();
      const old = await terminalApi.list();
      for (const info of old) {
        const session = new Session(changed);
        session.finishConnect({ ...info, running: false });
        session.term.options.disableStdin = true;
        session.error =
          "Output from the previous document is unavailable. Close this terminal tab before starting a new shell.";
        sessions.set(info.id, session);
      }
      new MutationObserver(applyAppearance).observe(document.documentElement, {
        attributes: true,
        attributeFilter: [
          "class",
          "style",
          "data-theme",
          "data-palette",
          "data-color-mode",
        ],
      });
      await listen<number>("terminal-preferences-changed", (event) => {
        if (event.payload > prefs.revision && !saving)
          void terminalApi
            .preferences()
            .then((value) => {
              prefs = value;
              applyAppearance();
              changed();
            })
            .catch((e) => {
              error = String(e);
              changed();
            });
      });
      applyAppearance();
      changed();
    })().catch((cause) => {
      initPromise = null;
      throw cause;
    })),
  start: async (context: string, worktree: string | null = null) => {
    await terminalStore.init();
    if (sessions.size + starting >= 2)
      throw Error(
        "Close a terminal tab before opening another (two tabs per window).",
      );
    starting++;
    const session = new Session(changed);
    try {
      const info = await terminalApi.start(context, worktree, session.receive);
      session.finishConnect(info);
      session.appearance(prefs);
      sessions.set(info.id, session);
      selected = info.id;
      changed();
      return info.id;
    } catch (error) {
      session.dispose();
      throw error;
    } finally {
      starting--;
    }
  },
  startAgent: async (preview: string) => {
    await terminalStore.init();
    if (sessions.size + starting >= 2)
      throw Error(
        "Close a terminal tab before launching an agent (two tabs per window).",
      );
    starting++;
    const session = new Session(changed);
    try {
      const started = await terminalApi.agent(preview, session.receive);
      session.finishConnect(started.terminal);
      session.appearance(prefs);
      sessions.set(started.terminal.id, session);
      selected = started.terminal.id;
      changed();
      return started;
    } catch (error) {
      session.dispose();
      throw error;
    } finally {
      starting--;
    }
  },
  close: async (id: string) => {
    const session = sessions.get(id);
    if (!session) return;
    await terminalApi.close(id);
    session.dispose();
    sessions.delete(id);
    changed();
  },
  save: async (patch: Partial<Preferences>) => {
    if (saving) return;
    saving = true;
    error = "";
    changed();
    try {
      prefs = await terminalApi.save({ ...prefs, ...patch });
      applyAppearance();
    } catch (e) {
      error = String(e);
      try {
        prefs = await terminalApi.preferences();
        applyAppearance();
      } catch {
        /* Keep the current values if native reload is unavailable. */
      }
      throw e;
    } finally {
      saving = false;
      changed();
    }
  },
};
