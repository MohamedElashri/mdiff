import {
  uiText,
  formatDate,
} from "../../vendor/diffle/src/client/i18n/runtime";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { X, Bot, RefreshCw } from "lucide-react";
import {
  Dialog,
  DialogContent,
} from "../../vendor/diffle/src/client/ui/dialog";
import { nativeReviewTransport } from "../transport";
import {
  agentApi,
  type View,
  type Activity,
  type Preview,
  type Finding,
  type Preferences,
} from "./api";
import "./agents.css";
type Props = {
  draft?: string;
  onDraft?: (value: string) => void;
  context: string | null;
  open: boolean;
  onClose: () => void;
  onActivity: (rows: Activity[]) => void;
  onLaunch: (preview: string) => Promise<unknown>;
  onTerminal: (id: string) => Promise<unknown>;
  onExternal: () => Promise<unknown>;
};
export default function AgentPanel(props: Props) {
  const [view, setView] = useState<View | null>(null),
    [preferences, setPreferences] = useState<Preferences | null>(null),
    [error, setError] = useState(""),
    [message, setMessage] = useState(""),
    [busy, setBusy] = useState(false),
    [tool, setTool] = useState("codex"),
    [profile, setProfile] = useState("general"),
    [destination, setDestination] = useState("embedded"),
    [preview, setPreview] = useState<Preview | null>(null),
    [document, setDocument] = useState(""),
    [findings, setFindings] = useState<Finding[]>([]),
    [attribution, setAttribution] = useState(""),
    [findingHead, setFindingHead] = useState("");
  const latest = useRef(props);
  latest.current = props;
  const generation = useRef(0),
    pending = useRef<number | null>(null),
    dirty = useRef(false);
  const accept = (value: View) => {
    setView(value);
    if (!dirty.current) setPreferences(value.preferences);
    latest.current.onActivity(
      value.preferences.enabled ? value.activities : [],
    );
  };
  useEffect(() => {
    let live = true;
    const ticket = ++generation.current;
    setPreview(null);
    setError("");
    setMessage("");
    setFindings([]);
    setDocument(props.draft ?? "");
    const read = () => {
      if (pending.current === ticket) return;
      pending.current = ticket;
      void agentApi
        .view(props.context)
        .then((v) => {
          if (live && ticket === generation.current) accept(v);
        })
        .catch((e) => live && setError(String(e)))
        .finally(() => {
          if (pending.current === ticket) pending.current = null;
        });
    };
    read();
    const listener = listen("agent-activity-changed", read);
    return () => {
      live = false;
      void listener.then((stop) => stop());
      void agentApi.unwatch(props.context).catch(() => {});
    };
  }, [props.context]);
  async function act(task: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true);
    setError("");
    setMessage("");
    const ticket = generation.current;
    try {
      await task();
    } catch (e) {
      if (ticket === generation.current) setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  const change = (patch: Partial<Preferences>) => {
    dirty.current = true;
    setPreferences((p) => (p ? { ...p, ...patch } : p));
    setPreview(null);
  };
  return (
    <Dialog open={props.open} onOpenChange={(open) => !open && props.onClose()}>
      <DialogContent className="agent-panel" aria-labelledby="agent-title">
        <header>
          <div>
            <h2 id="agent-title">{uiText("Agents and review")}</h2>
            <p>
              {uiText(
                "Local activity, with an explicit handoff for each review.",
              )}
            </p>
          </div>
          <button
            aria-label={uiText("Close agents and review")}
            onClick={props.onClose}
          >
            <X size={17} />
          </button>
        </header>
        <nav className="agent-actions">
          <button
            disabled={busy}
            onClick={() =>
              void act(async () => accept(await agentApi.view(props.context)))
            }
          >
            <RefreshCw size={14} />
            {uiText("Refresh")}
          </button>
          <button
            disabled={!props.context || busy}
            onClick={() => void act(props.onExternal)}
          >
            {uiText("Open external terminal")}
          </button>
        </nav>
        {(error || view?.error) && (
          <p role="alert" className="agent-error">
            {error || view?.error}
          </p>
        )}
        {message && <p role="status">{message}</p>}
        {preferences && (
          <details>
            <summary>{uiText("Activity and installed tools")}</summary>
            <div className="agent-preferences">
              <label>
                <input
                  type="checkbox"
                  checked={preferences.enabled}
                  onChange={(e) => change({ enabled: e.target.checked })}
                />
                {uiText("Detect agents in the selected repository")}
              </label>
              <p>
                {uiText(
                  "Opt-in metadata sampling; no conversations, command lines or environments are read. Hidden activity stays unknown. External processes are never stopped by mdiff.",
                )}
              </p>
              <label>
                <input
                  type="checkbox"
                  checked={preferences.hooks}
                  onChange={(e) => change({ hooks: e.target.checked })}
                />
                {uiText("Accept local state hooks")}
              </label>
              {(["codex", "claude"] as const).map((id) => (
                <label key={id}>
                  {id === "codex" ? uiText("Codex") : uiText("Claude Code")}
                  {uiText(" executable (optional)")}
                  <input
                    value={preferences[id] ?? ""}
                    placeholder={uiText("Discover from PATH")}
                    onChange={(e) => change({ [id]: e.target.value || null })}
                  />
                </label>
              ))}
              <label>
                {uiText("Additional review focus")}
                <textarea
                  value={preferences.focus}
                  maxLength={4000}
                  onChange={(e) => change({ focus: e.target.value })}
                />
              </label>
              <button
                disabled={busy}
                onClick={() =>
                  void act(async () => {
                    const value = await agentApi.save(preferences);
                    dirty.current = false;
                    accept(value);
                    setMessage("Agent preferences saved.");
                  })
                }
              >
                {uiText("Save agent preferences")}
              </button>
              <button
                disabled={busy}
                onClick={() =>
                  void act(async () => {
                    dirty.current = false;
                    accept(await agentApi.view(props.context));
                  })
                }
              >
                {uiText("Reload agent preferences")}
              </button>
              <div className="agent-actions">
                <button
                  disabled={busy}
                  onClick={() =>
                    void act(async () => {
                      accept(await agentApi.hooks(true));
                      setMessage(
                        "Owned hook bundle generated. Existing agent settings were not changed. Enable local hooks above, then merge the appropriate configuration for external sessions.",
                      );
                    })
                  }
                >
                  {uiText("Generate hook bundle")}
                </button>
                <button
                  disabled={busy || !view?.hooks}
                  onClick={() =>
                    void act(async () => {
                      accept(await agentApi.hooks(false));
                      setMessage(
                        "Hook delivery disabled and generated configs removed. An inert helper remains so existing references return success; remove those references before deleting it.",
                      );
                    })
                  }
                >
                  {uiText("Uninstall hook bundle")}
                </button>
              </div>
              {view?.hooks && (
                <p className="agent-paths">
                  {uiText("Helper: ")}
                  {view.hooks.helper}
                  <br />
                  {uiText("Claude settings: ")}
                  {view.hooks.claude}
                  <br />
                  {uiText("Codex hooks: ")}
                  {view.hooks.codex}
                  <br />
                  {uiText(
                    "New launches append these hooks in their isolated checkout when enabled. Existing sessions require their normal hook reload/trust flow.",
                  )}
                </p>
              )}
            </div>
          </details>
        )}
        <section>
          <h3>{uiText("Activity in this repository")}</h3>
          {!view?.preferences.enabled ? (
            <p>
              {uiText(
                "Detection is off. You can still prepare an explicit review below.",
              )}
            </p>
          ) : !props.context ? (
            <p>{uiText("Select a repository or review to watch.")}</p>
          ) : !view.activities.length ? (
            <p>
              {uiText(
                "No supported agents detected. Missing process access or wrappers may leave activity unknown.",
              )}
            </p>
          ) : (
            <ul className="agent-activity">
              {view.activities.map((row) => (
                <li key={row.id}>
                  <Bot size={15} />
                  <div>
                    <strong>
                      {row.tool}
                      {row.parent ? uiText(" subagent") : ""}
                    </strong>
                    <span className="agent-state">{row.state}</span>
                    <p>{row.root}</p>
                    <small>
                      {row.provenance === "detected"
                        ? uiText("Process detected; state is unknown")
                        : row.provenance === "hook"
                          ? uiText("Local hook report")
                          : uiText("Explicit mdiff launch")}{" "}
                      {uiText("· observed")}{" "}
                      {formatDate(new Date(row.seen * 1000), {
                        timeStyle: "medium",
                      })}
                      {row.commit ? ` · ${row.commit.slice(0, 12)}` : ""}
                    </small>
                  </div>
                  {row.terminal && (
                    <button
                      onClick={() =>
                        void act(() => props.onTerminal(row.terminal!))
                      }
                    >
                      {uiText("Show terminal")}
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>
        <section>
          <h3>{uiText("Review with agent")}</h3>
          <p>
            {uiText(
              "Prepare a pinned comparison in an isolated checkout. Existing working files and branch stay in place. Working-change reviews support prompt export; choose a commit comparison in History for direct launch.",
            )}
          </p>
          <div className="agent-actions">
            <label>
              {uiText("Agent")}
              <select
                value={tool}
                onChange={(e) => {
                  setTool(e.target.value);
                  setPreview(null);
                }}
              >
                <option value="codex">{uiText("Codex")}</option>
                <option value="claude">{uiText("Claude Code")}</option>
              </select>
            </label>
            <label>
              {uiText("Profile")}
              <select
                value={profile}
                onChange={(e) => {
                  setProfile(e.target.value);
                  setPreview(null);
                }}
              >
                <option value="general">{uiText("Correctness")}</option>
                <option value="security">{uiText("Security")}</option>
                <option value="performance">{uiText("Performance")}</option>
              </select>
            </label>
            <button
              disabled={busy || !props.context}
              onClick={() =>
                void act(async () => {
                  const context = props.context!;
                  const prepared = await agentApi.preview(
                    context,
                    tool,
                    profile,
                  );
                  if (latest.current.context === context) setPreview(prepared);
                })
              }
            >
              {uiText("Prepare agent review")}
            </button>
            <button
              disabled={busy || !props.context}
              onClick={() =>
                void act(async () => {
                  const response = await nativeReviewTransport(
                    props.context!,
                  ).request("/api/threads/export");
                  if (!response.ok) throw Error(await response.text());
                  await writeText(await response.text());
                  setMessage("Existing mdiff comment prompt copied.");
                })
              }
            >
              {uiText("Copy comment prompt")}
            </button>
          </div>
          {preview && (
            <div className="agent-preview">
              <label>
                {uiText("Terminal")}
                <select
                  value={destination}
                  onChange={(e) => setDestination(e.target.value)}
                >
                  <option value="embedded">
                    {uiText("Embedded terminal")}
                  </option>
                  {view?.terminals?.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name}
                    </option>
                  ))}
                </select>
              </label>
              <p>
                {uiText(
                  "Only installed external terminals with a command-vector launch strategy appear here. Folder-only launchers remain available through Open external terminal.",
                )}
              </p>
              <dl>
                <dt>{uiText("Agent executable")}</dt>
                <dd>{preview.executable}</dd>
                <dt>{uiText("Source checkout")}</dt>
                <dd>{preview.root}</dd>
                <dt>{uiText("Isolated checkout")}</dt>
                <dd>{preview.checkout}</dd>
                <dt>{uiText("Pinned comparison")}</dt>
                <dd>
                  {preview.base} → {preview.head}
                </dd>
              </dl>
              <details>
                <summary>
                  {uiText("Review instructions sent to ")}
                  {preview.tool}
                </summary>
                <pre>{preview.prompt}</pre>
              </details>
              <p>
                {uiText(
                  "The agent may contact its provider and will use its normal local authentication, repository trust and permission prompts. Isolated files are retained for inspection. mdiff does not publish results.",
                )}
              </p>
              <div className="agent-actions">
                <button
                  disabled={busy}
                  onClick={() =>
                    void act(async () => {
                      if (destination === "embedded")
                        await props.onLaunch(preview.id);
                      else {
                        const started = await agentApi.external(
                          preview.id,
                          destination,
                        );
                        setMessage(
                          `External handoff accepted. Isolated checkout retained at ${started.checkout}`,
                        );
                      }
                      setPreview(null);
                      props.onClose();
                    })
                  }
                >
                  {uiText("Launch ")}
                  {preview.tool}
                  {uiText(" in")}{" "}
                  {destination === "embedded"
                    ? uiText("embedded terminal")
                    : uiText("external terminal")}
                </button>
                <button
                  onClick={() =>
                    void act(async () => {
                      await writeText(preview.prompt);
                      setMessage(
                        "Pinned review instructions copied. External launch remains manual; verify the checkout before running your agent.",
                      );
                    })
                  }
                >
                  {uiText("Copy pinned review prompt")}
                </button>
              </div>
            </div>
          )}
        </section>
        <section>
          <h3>{uiText("Draft findings")}</h3>
          <p>
            {uiText(
              "Paste a JSON findings document for the currently pinned comparison. Imports validate changed paths and head-side text ranges; findings remain local drafts. Agent attribution is user-selected, not provider-verified.",
            )}
          </p>
          <textarea
            aria-label={uiText("Agent findings JSON")}
            value={document}
            maxLength={131072}
            onChange={(e) => {
              setDocument(e.target.value);
              props.onDraft?.(e.target.value);
              setFindings([]);
            }}
            placeholder={uiText(
              '{"findings":[{"path":"src/file.ts","start":1,"end":2,"body":"Explain the issue"}]}',
            )}
          />
          <button
            disabled={busy || !props.context || !document}
            onClick={() =>
              void act(async () => {
                const imported = await agentApi.import(
                  props.context!,
                  document,
                  tool,
                );
                if (latest.current.context === props.context) {
                  setFindings(imported.findings);
                  setAttribution(imported.tool);
                  setFindingHead(imported.head);
                }
              })
            }
          >
            {uiText("Validate draft findings")}
          </button>
          <ul>
            {findings.map((f, i) => (
              <li key={i}>
                <strong>
                  {uiText("Imported draft · ")}
                  {attribution} · {findingHead.slice(0, 12)} · {f.path}:
                  {f.start}–{f.end}
                </strong>
                <p>{f.body}</p>
                <button
                  onClick={() =>
                    void act(() =>
                      writeText(
                        `[User-attributed agent draft: ${attribution} @ ${findingHead}] ${f.path}:${f.start}-${f.end}\n${f.body}`,
                      ),
                    )
                  }
                >
                  {uiText("Copy draft finding")}
                </button>
              </li>
            ))}
          </ul>
        </section>
      </DialogContent>
    </Dialog>
  );
}
