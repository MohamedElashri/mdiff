import { useEffect, useRef, useState } from "react";
import { uiText } from "../../vendor/diffle/src/client/i18n/runtime";
import { terminalApi, type Preferences } from "../terminal/api";
import { worktreeApi, type Tools } from "../worktrees/api";
export default function TerminalPreferences({
  onDirty,
}: {
  onDirty: (dirty: boolean) => void;
}) {
  const [saved, setSaved] = useState<Preferences | null>(null),
    [draft, setDraft] = useState<Preferences | null>(null);
  const [tools, setTools] = useState<Omit<Tools, "root" | "temporary"> | null>(
      null,
    ),
    [terminal, setTerminal] = useState<string | null>(null);
  const [error, setError] = useState(""),
    [message, setMessage] = useState(""),
    [busy, setBusy] = useState<"save" | "reload" | null>(null);
  const live = useRef(true),
    pending = useRef(false);
  const load = async () => {
    const [p, t] = await Promise.all([
      terminalApi.preferences(),
      worktreeApi.preferences(),
    ]);
    if (!live.current) return;
    setSaved(p);
    setDraft(p);
    setTools(t);
    setTerminal(t.preferences.terminal);
  };
  useEffect(() => {
    live.current = true;
    void load().catch((e) => live.current && setError(String(e)));
    return () => {
      live.current = false;
    };
  }, []);
  const shellDirty =
    !!saved &&
    !!draft &&
    (saved.enabled !== draft.enabled || saved.fontSize !== draft.fontSize);
  const toolsDirty = !!tools && tools.preferences.terminal !== terminal;
  useEffect(
    () => onDirty(shellDirty || toolsDirty),
    [shellDirty, toolsDirty, onDirty],
  );
  const act = async (
    fn: () => Promise<void>,
    operation: "save" | "reload" = "save",
  ) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(operation);
    setError("");
    setMessage("");
    try {
      await fn();
    } catch (e) {
      if (live.current) setError(String(e));
    } finally {
      pending.current = false;
      if (live.current) setBusy(null);
    }
  };
  return (
    <>
      {error && (
        <p role="alert" className="settings-error">
          {error}
        </p>
      )}
      {message && <p role="status">{message}</p>}
      {!draft || !tools ? (
        <>
          <p role="status">
            {error
              ? uiText("Terminal preferences could not be loaded.")
              : uiText("Loading terminal preferences…")}
          </p>
          <button disabled={!!busy} onClick={() => void act(load, "reload")}>
            {busy ? uiText("Loading…") : uiText("Retry")}
          </button>
        </>
      ) : (
        <>
          <div className="preference-actions">
            <button
              className="primary"
              disabled={!!busy || (!shellDirty && !toolsDirty)}
              onClick={() =>
                void act(async () => {
                  // Each store has its own revision. Preserve successful writes if the other fails.
                  if (shellDirty) {
                    const p = await terminalApi.save(draft);
                    if (live.current) {
                      setSaved(p);
                      setDraft(p);
                    }
                  }
                  if (toolsDirty) {
                    const p = await worktreeApi.save(
                      tools.preferences.revision,
                      terminal,
                    );
                    if (live.current) setTools({ ...tools, preferences: p });
                  }
                  if (live.current)
                    setMessage(uiText("Terminal preferences saved."));
                })
              }
            >
              {busy === "save" ? uiText("Saving…") : uiText("Save changes")}
            </button>
            <button
              disabled={!!busy || (!shellDirty && !toolsDirty)}
              onClick={() => {
                setDraft(saved);
                setTerminal(tools.preferences.terminal);
                setError("");
                setMessage("");
              }}
            >
              {uiText("Discard changes")}
            </button>
            <button disabled={!!busy} onClick={() => void act(load, "reload")}>
              {busy === "reload"
                ? uiText("Reloading…")
                : shellDirty || toolsDirty
                  ? uiText("Discard and reload")
                  : uiText("Reload saved preferences")}
            </button>
          </div>
          <h4>{uiText("Embedded terminal")}</h4>
          <label className="preference-row">
            <input
              type="checkbox"
              checked={draft.enabled}
              disabled={!!busy}
              onChange={(e) => {
                setMessage("");
                setDraft({ ...draft, enabled: e.target.checked });
              }}
            />
            {uiText("Allow new shells")}
          </label>
          <p>
            {uiText(
              "Shells start only when requested. Disabling startup keeps existing shells alive until you close them.",
            )}
          </p>
          <label>
            {uiText("Terminal font size")}
            <select
              value={draft.fontSize}
              disabled={!!busy}
              onChange={(e) => {
                setMessage("");
                setDraft({ ...draft, fontSize: Number(e.target.value) });
              }}
            >
              {Array.from({ length: 15 }, (_, i) => i + 10).map((size) => (
                <option key={size} value={size}>
                  {size}
                </option>
              ))}
            </select>
          </label>
          <h4>{uiText("External tools")}</h4>
          <label>
            {uiText("Preferred terminal")}
            <select
              aria-label={uiText("Preferred terminal")}
              value={terminal ?? ""}
              disabled={!!busy}
              onChange={(e) => {
                setMessage("");
                setTerminal(e.target.value || null);
              }}
            >
              <option value="">{uiText("Ask when opening")}</option>
              {terminal && !tools.terminals.some((t) => t.id === terminal) && (
                <option value={terminal}>
                  {uiText("Unavailable: {tool}", { tool: terminal })}
                </option>
              )}
              {tools.terminals.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))}
            </select>
          </label>
          <p>
            {uiText(
              "Only installed tools with a supported folder-opening method appear. Refresh after installing a tool; no shell is launched here.",
            )}
          </p>
          <h4>{uiText("Available editors")}</h4>
          <p>
            {tools.editors.length
              ? tools.editors.map((t) => t.name).join(", ")
              : uiText(
                  "No supported editor found. Install its command-line launcher, then reload.",
                )}
          </p>
        </>
      )}
    </>
  );
}
