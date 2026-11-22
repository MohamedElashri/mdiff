import { uiText } from "../../vendor/diffle/src/client/i18n/runtime";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { X } from "lucide-react";
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
} from "./api";
import { AgentActivity } from "./AgentActivity";
import { AgentSettings } from "./AgentSettings";
import "./agents.css";
type Props = {
  initialTab?: "activity" | "review";
  activityManaged?: boolean;
  commitComparison?: boolean;
  pinnedComparison?: boolean;
  reviewContext?: string | null;
  draft?: string;
  onDraft?: (value: string) => void;
  context: string | null;
  open: boolean;
  onClose: () => void;
  onPreferences?: () => void;
  onActivity: (rows: Activity[]) => void;
  onLaunch: (preview: string) => Promise<unknown>;
  onTerminal: (id: string) => Promise<unknown>;
  onExternal: () => Promise<unknown>;
};
export default function AgentPanel(props: Props) {
  const [view, setView] = useState<View | null>(null),
    [error, setError] = useState(""),
    [loadError, setLoadError] = useState(""),
    [settingsDirty, setSettingsDirty] = useState(false),
    [message, setMessage] = useState(""),
    [operation, setOperation] = useState(""),
    [loading, setLoading] = useState(true),
    [tab, setTab] = useState("activity"),
    [tool, setTool] = useState("codex"),
    [profile, setProfile] = useState("general"),
    [destination, setDestination] = useState("embedded"),
    [preview, setPreview] = useState<Preview | null>(null),
    [document, setDocument] = useState(""),
    [findings, setFindings] = useState<Finding[]>([]),
    [attribution, setAttribution] = useState(""),
    [findingHead, setFindingHead] = useState("");
  useEffect(() => {
    if (settingsDirty) setPreview(null);
  }, [settingsDirty]);
  useEffect(() => {
    if (props.open) setTab(props.initialTab ?? "activity");
  }, [props.open, props.initialTab]);
  const comparisonContext =
    props.reviewContext === undefined ? props.context : props.reviewContext;
  const busy = !!operation;
  const selectedTool = view?.tools.find((candidate) => candidate.id === tool);
  const toolAvailable = !!selectedTool?.path && !selectedTool.error;
  const latest = useRef(props);
  latest.current = props;
  const generation = useRef(0),
    version = useRef(-1),
    action = useRef(false);
  const reload = useRef<() => Promise<void>>(async () => {});
  const accept = (value: View) => {
    if (latest.current.context !== props.context) return;
    if (value.version < version.current) return;
    version.current = value.version;
    setView(value);
    if (!props.activityManaged)
      latest.current.onActivity(
        value.preferences.enabled ? value.activities : [],
      );
  };
  useEffect(() => {
    let live = true,
      pending = false,
      queued = false;
    const ticket = ++generation.current;
    const observer = crypto.randomUUID();
    version.current = -1;
    setPreview(null);
    setError("");
    setMessage("");
    setFindings([]);
    setOperation("");
    action.current = false;
    setLoading(true);
    setView((old) =>
      old
        ? {
            ...old,
            activities: [],
            roots: [],
            scan: { ...old.scan, running: false, completed: null, error: null },
          }
        : null,
    );
    if (!props.activityManaged) latest.current.onActivity([]);
    setDocument(props.draft ?? "");
    let inFlight: Promise<void> | null = null;
    const read = (): Promise<void> => {
      if (pending) {
        queued = true;
        return inFlight!;
      }
      pending = true;
      inFlight = (async () => {
        try {
          do {
            queued = false;
            const value = await (props.activityManaged
              ? agentApi.settings()
              : agentApi.view(props.context, observer));
            if (live && ticket === generation.current) {
              accept(value);
              setLoadError("");
            }
          } while (queued && live);
        } catch (e) {
          if (live && ticket === generation.current) setLoadError(String(e));
          throw e;
        } finally {
          pending = false;
          if (live && ticket === generation.current) setLoading(false);
        }
      })();
      return inFlight;
    };
    reload.current = read;
    // Listen before the first read so an immediate scan completion cannot be lost.
    const listener = listen("agent-activity-changed", () => {
      void read().catch(() => {});
    });
    void listener
      .then(() => {
        if (live) void read().catch(() => {});
      })
      .catch((e) => {
        if (live) {
          setError(
            uiText("Live updates unavailable: {error}", { error: String(e) }),
          );
          void read().catch(() => {});
        }
      });
    return () => {
      live = false;
      void listener.then((stop) => stop()).catch(() => {});
      if (!props.activityManaged)
        void agentApi.unwatch(props.context, observer).catch(() => {});
    };
  }, [
    props.context,
    props.activityManaged,
    props.reviewContext,
    props.commitComparison,
    props.pinnedComparison,
  ]);
  async function act(
    taskOrLabel: string | (() => Promise<unknown>),
    task?: () => Promise<unknown>,
  ) {
    if (action.current) return;
    action.current = true;
    const label = typeof taskOrLabel === "string" ? taskOrLabel : "working";
    const run = typeof taskOrLabel === "string" ? task! : taskOrLabel;
    const ticket = generation.current;
    setOperation(label);
    setError("");
    setMessage("");
    try {
      await run();
    } catch (e) {
      if (ticket === generation.current) setError(String(e));
    } finally {
      if (ticket === generation.current) {
        action.current = false;
        setOperation("");
      }
    }
  }
  const close = () => {
    if (settingsDirty) {
      setTab("settings");
      setError(uiText("Save or discard settings before closing."));
    } else props.onClose();
  };
  const tabs: [string, string][] = [
    ["activity", uiText("Activity")],
    ...(!props.onPreferences
      ? ([["settings", uiText("Settings")]] as [string, string][])
      : []),
    ["review", uiText("Review")],
  ];
  return (
    <Dialog open={props.open} onOpenChange={(open) => !open && close()}>
      <DialogContent className="agent-panel" aria-labelledby="agent-title">
        <header>
          <div>
            <h2 id="agent-title">{uiText("Agents and review")}</h2>
            <p>
              {uiText("Observe local agents or prepare an isolated review.")}
            </p>
          </div>
          <button
            aria-label={uiText("Close agents and review")}
            onClick={close}
          >
            <X size={17} />
          </button>
        </header>
        <div className="agent-navigation">
          <nav
            className="agent-tabs"
            role="tablist"
            aria-label={uiText("Agent views")}
          >
            {tabs.map(([id, label], index) => (
              <button
                key={id}
                id={`agent-tab-${id}`}
                role="tab"
                aria-selected={tab === id}
                aria-controls={`agent-content-${id}`}
                tabIndex={tab === id ? 0 : -1}
                onClick={() => setTab(id)}
                onKeyDown={(e) => {
                  const next =
                    e.key === "ArrowRight"
                      ? (index + 1) % tabs.length
                      : e.key === "ArrowLeft"
                        ? (index + tabs.length - 1) % tabs.length
                        : e.key === "Home"
                          ? 0
                          : e.key === "End"
                            ? tabs.length - 1
                            : -1;
                  if (next >= 0) {
                    e.preventDefault();
                    setTab(tabs[next]![0]);
                    e.currentTarget.parentElement
                      ?.querySelectorAll<HTMLButtonElement>("button")
                      [next]?.focus();
                  }
                }}
              >
                {label}
              </button>
            ))}
          </nav>{" "}
          {props.onPreferences && (
            <button onClick={props.onPreferences}>
              {uiText("Agent preferences")}
            </button>
          )}
        </div>
        {(error || loadError || view?.error) && (
          <div className="agent-feedback agent-error" role="alert">
            {error || loadError || view?.error}
            {(loadError || view?.error) && (
              <button
                disabled={busy}
                onClick={() =>
                  void act("reload", async () => {
                    if (view?.error) accept(await agentApi.reload());
                    else await reload.current();
                  })
                }
              >
                {uiText("Retry")}
              </button>
            )}
          </div>
        )}
        {message && (
          <p className="agent-feedback" role="status">
            {message}
          </p>
        )}
        {busy && operation !== "scan" && (
          <p role="status">
            {operation === "save"
              ? uiText("Saving settings…")
              : operation === "tools"
                ? uiText("Checking installed tools…")
                : operation === "hooks"
                  ? uiText("Generating hook configuration…")
                  : operation === "reload"
                    ? uiText("Reloading…")
                    : uiText("Working…")}
          </p>
        )}
        <div
          role="tabpanel"
          id="agent-content-activity"
          aria-labelledby="agent-tab-activity"
          hidden={tab !== "activity"}
        >
          <AgentActivity
            view={view}
            context={props.context}
            loading={loading}
            busy={operation}
            onScan={() =>
              void act("scan", async () => accept(await agentApi.scan()))
            }
            onEnable={() =>
              settingsDirty
                ? (setTab("settings"),
                  setError(uiText("Save settings to enable detection.")))
                : void act("save", async () => {
                    if (view) {
                      accept(
                        await agentApi.save({
                          ...view.preferences,
                          enabled: true,
                        }),
                      );
                      setMessage(
                        props.context
                          ? uiText(
                              "Detection enabled. Scanning this repository.",
                            )
                          : uiText(
                              "Detection enabled. Select a repository to scan.",
                            ),
                      );
                    }
                  })
            }
            onSettings={() =>
              props.onPreferences ? props.onPreferences() : setTab("settings")
            }
            onTerminal={(id) =>
              void act("terminal", () => props.onTerminal(id))
            }
          />
        </div>
        <div
          role="tabpanel"
          id="agent-content-settings"
          aria-labelledby="agent-tab-settings"
          hidden={tab !== "settings"}
        >
          {props.onPreferences ? null : view ? (
            <AgentSettings
              view={view}
              busy={operation}
              act={act}
              accept={(value) => {
                setPreview(null);
                accept(value);
              }}
              reload={async () => accept(await agentApi.reload())}
              notify={(message) => {
                setError("");
                setMessage(message);
              }}
              onDirty={setSettingsDirty}
            />
          ) : (
            <p role={loading ? "status" : undefined}>
              {loading
                ? uiText("Loading agent settings…")
                : uiText(
                    "Agent settings could not be loaded. Use Retry to try again.",
                  )}
            </p>
          )}
        </div>
        <div
          role="tabpanel"
          id="agent-content-review"
          aria-labelledby="agent-tab-review"
          hidden={tab !== "review"}
        >
          <section>
            <h3>{uiText("Review with agent")}</h3>
            <p>
              {uiText(
                "Review a pinned commit comparison in an isolated checkout. Your working files and branch stay in place.",
              )}
            </p>
            {!comparisonContext && (
              <p role="status">
                {uiText(
                  "Open a review to use comments and findings. For direct agent launch, choose a commit in History and open its comparison.",
                )}
              </p>
            )}
            {comparisonContext && props.commitComparison === false && (
              <p role="status">
                {uiText(
                  "Direct launch needs a commit comparison from History. You can still export existing comments below.",
                )}
              </p>
            )}
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
                disabled={
                  busy ||
                  settingsDirty ||
                  !comparisonContext ||
                  props.commitComparison === false ||
                  !toolAvailable
                }
                onClick={() =>
                  void act("prepare", async () => {
                    const context = comparisonContext!;
                    const prepared = await agentApi.preview(
                      context,
                      tool,
                      profile,
                    );
                    if (
                      (latest.current.reviewContext === undefined
                        ? latest.current.context
                        : latest.current.reviewContext) === context
                    )
                      setPreview(prepared);
                  })
                }
              >
                {operation === "prepare"
                  ? uiText("Preparing…")
                  : uiText("Prepare agent review")}
              </button>
              <button
                disabled={busy || !comparisonContext}
                onClick={() =>
                  void act(async () => {
                    const response = await nativeReviewTransport(
                      comparisonContext!,
                    ).request("/api/threads/export");
                    if (!response.ok) throw Error(await response.text());
                    await writeText(await response.text());
                    setMessage(uiText("Existing mdiff comment prompt copied."));
                  })
                }
              >
                {uiText("Copy comment prompt")}
              </button>
              <button
                disabled={!props.context || busy}
                onClick={() => void act(props.onExternal)}
              >
                {uiText("Open external terminal")}
              </button>
            </div>
            {view && !toolAvailable && (
              <p role="status">
                {uiText(
                  "Choose an available agent, or configure its executable in Preferences → Agents.",
                )}
              </p>
            )}
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
                          uiText(
                            "Pinned review instructions copied. Verify the checkout before running your agent.",
                          ),
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
                "Paste findings JSON for this comparison. mdiff checks file paths and line ranges before creating local drafts. Nothing is published.",
              )}
            </p>
            {comparisonContext && props.pinnedComparison === false && (
              <p role="status">
                {uiText(
                  "Choose a pinned comparison in History to validate findings. Your pasted JSON is kept with this tab.",
                )}
              </p>
            )}
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
              disabled={
                busy ||
                !comparisonContext ||
                !document ||
                props.pinnedComparison === false
              }
              onClick={() =>
                void act(async () => {
                  const imported = await agentApi.import(
                    comparisonContext!,
                    document,
                    tool,
                  );
                  if (
                    (latest.current.reviewContext === undefined
                      ? latest.current.context
                      : latest.current.reviewContext) === comparisonContext
                  ) {
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
                      void act(async () => {
                        await writeText(
                          `[User-attributed agent draft: ${attribution} @ ${findingHead}] ${f.path}:${f.start}-${f.end}\n${f.body}`,
                        );
                        setMessage(uiText("Draft finding copied."));
                      })
                    }
                  >
                    {uiText("Copy draft finding")}
                  </button>
                </li>
              ))}
            </ul>
          </section>
        </div>
      </DialogContent>
    </Dialog>
  );
}
