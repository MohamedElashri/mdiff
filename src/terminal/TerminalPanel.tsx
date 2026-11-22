import { uiText } from "../../vendor/diffle/src/client/i18n/runtime";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  X,
  Plus,
  ChevronDown,
  Copy,
  Terminal as TerminalIcon,
} from "lucide-react";
import { terminalStore, type Session } from "./controller";
import "@xterm/xterm/css/xterm.css";
import "./terminal.css";
type Props = {
  hidden: boolean;
  context: string | null;
  onHide: () => void;
  onExternal: () => Promise<unknown>;
};
function Surface({ session, active }: { session: Session; active: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!host.current) return;
    session.attach(host.current);
    const observer = new ResizeObserver(() => session.resize());
    observer.observe(host.current);
    return () => observer.disconnect();
  }, [session]);
  useEffect(() => {
    if (active) {
      session.resize();
      session.term.focus();
    }
  }, [active, session]);
  return <div className="terminal-surface" hidden={!active} ref={host} />;
}
export default function TerminalPanel({
  hidden,
  context,
  onHide,
  onExternal,
}: Props) {
  useSyncExternalStore(terminalStore.subscribe, terminalStore.version);
  const sessions = terminalStore.sessions(),
    prefs = terminalStore.preferences();
  const [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [height, setHeight] = useState(prefs.height);
  const drag = useRef<{ y: number; height: number } | null>(null),
    heightRef = useRef(height);
  heightRef.current = height;
  useEffect(() => {
    void terminalStore.init().catch((e) => setError(String(e)));
  }, []);
  useEffect(() => setHeight(prefs.height), [prefs.height]);
  const current =
    sessions.find((s) => s.info?.id === terminalStore.selected()) ??
    sessions[0];
  async function perform(action: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  const saveHeight = () =>
    void perform(() =>
      terminalStore.save({ height: Math.round(heightRef.current) }),
    );
  return (
    <section
      className="terminal-panel"
      data-terminal
      hidden={hidden}
      aria-label={uiText("Embedded terminal")}
      style={{ height: `min(${height}px,65vh)` }}
    >
      <div
        className="terminal-divider"
        role="separator"
        aria-label={uiText("Resize terminal panel")}
        aria-orientation="horizontal"
        aria-valuenow={height}
        aria-valuemin={160}
        aria-valuemax={600}
        tabIndex={0}
        onPointerDown={(e) => {
          drag.current = { y: e.clientY, height };
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          if (drag.current)
            setHeight(
              Math.max(
                160,
                Math.min(600, drag.current.height + drag.current.y - e.clientY),
              ),
            );
        }}
        onPointerUp={(e) => {
          if (drag.current) {
            drag.current = null;
            e.currentTarget.releasePointerCapture(e.pointerId);
            saveHeight();
          }
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowUp" || e.key === "ArrowDown") {
            e.preventDefault();
            const next = Math.max(
              160,
              Math.min(600, height + (e.key === "ArrowUp" ? 16 : -16)),
            );
            setHeight(next);
            void perform(() => terminalStore.save({ height: next }));
          }
        }}
      />
      <header>
        <div
          className="terminal-tabs"
          role="tablist"
          aria-label={uiText("Terminal tabs")}
        >
          {sessions.map((session) => (
            <div className="terminal-tab" key={session.info!.id}>
              <button
                role="tab"
                aria-selected={session === current}
                onClick={() => terminalStore.select(session.info!.id)}
                onKeyDown={(e) => {
                  const keys = ["ArrowLeft", "ArrowRight", "Home", "End"];
                  if (!keys.includes(e.key)) return;
                  e.preventDefault();
                  const i = sessions.indexOf(session);
                  const next =
                    e.key === "Home"
                      ? 0
                      : e.key === "End"
                        ? sessions.length - 1
                        : (i +
                            (e.key === "ArrowRight"
                              ? 1
                              : sessions.length - 1)) %
                          sessions.length;
                  terminalStore.select(sessions[next]!.info!.id);
                }}
                title={session.info!.root}
              >
                <TerminalIcon size={13} />
                {session.info!.root.split("/").pop()}
                {!session.info!.running && " (exited)"}
              </button>
              <button
                aria-label={uiText("Close terminal {value0}", {
                  value0: String(session.info!.root),
                })}
                disabled={busy}
                onClick={() =>
                  void perform(async () => {
                    if (
                      session.info!.running &&
                      !(await confirm(
                        "Stop this shell and its jobs? Terminal output is not saved.",
                        { title: uiText("Close terminal"), kind: "warning" },
                      ))
                    )
                      return;
                    await terminalStore.close(session.info!.id);
                  })
                }
              >
                <X size={13} />
              </button>
            </div>
          ))}
        </div>
        <button
          disabled={busy || !context || !prefs.enabled || sessions.length >= 2}
          onClick={() =>
            void perform(async () =>
              terminalStore.select(await terminalStore.start(context!)),
            )
          }
        >
          <Plus size={14} />
          {sessions.length ? uiText("New shell") : uiText("Start shell")}
        </button>
        <button
          aria-label={uiText("Copy terminal selection")}
          onClick={() => void perform(async () => current?.copy())}
        >
          <Copy size={14} />
        </button>
        <button aria-label={uiText("Hide terminal panel")} onClick={onHide}>
          <ChevronDown size={16} />
        </button>
      </header>
      <div className="terminal-settings">
        <label>
          <input
            type="checkbox"
            checked={prefs.enabled}
            disabled={terminalStore.saving()}
            onChange={(e) =>
              void perform(() =>
                terminalStore.save({ enabled: e.target.checked }),
              )
            }
          />
          {uiText("Allow new shells")}
        </label>
        <label>
          {uiText("Font size")}
          <select
            aria-label={uiText("Terminal font size")}
            value={prefs.fontSize}
            disabled={terminalStore.saving()}
            onChange={(e) =>
              void perform(() =>
                terminalStore.save({ fontSize: Number(e.target.value) }),
              )
            }
          >
            {Array.from({ length: 15 }, (_, i) => i + 10).map((size) => (
              <option key={size}>{size}</option>
            ))}
          </select>
        </label>
        <span>{uiText("Uses app theme and code font")}</span>
        <button onClick={() => void perform(onExternal)}>
          {uiText("Open external terminal")}
        </button>
      </div>
      {current && (
        <p className="terminal-target" title={current.info!.root}>
          {current.info!.root} · {current.info!.shell}
          {uiText(
            " · Shells stay with this window; changing review or workspace does not change their directory.",
          )}
        </p>
      )}
      {!sessions.length && (
        <div className="terminal-empty">
          <p>
            {prefs.enabled
              ? uiText("Start a shell explicitly in the selected checkout.")
              : uiText(
                  "Embedded shell startup is disabled. External terminals remain available.",
                )}
          </p>
          {!context && (
            <p>{uiText("Open a repository or select a review first.")}</p>
          )}
          <p>
            {uiText("Commands and output are not restored after app restart.")}
          </p>
        </div>
      )}
      <div className="terminal-views">
        {sessions.map((session) => (
          <Surface
            key={session.info!.id}
            session={session}
            active={!hidden && session === current}
          />
        ))}
      </div>
      {(error ||
        terminalStore.error() ||
        current?.error ||
        current?.info?.exit) && (
        <p
          className="terminal-status"
          role={
            error || terminalStore.error() || current?.error
              ? "alert"
              : "status"
          }
        >
          {error ||
            terminalStore.error() ||
            current?.error ||
            current?.info?.exit?.message}
        </p>
      )}
    </section>
  );
}
