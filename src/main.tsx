import { AppearancePanel } from "../vendor/diffle/src/client/appearance/AppearancePanel";
import { Dialog, DialogContent } from "../vendor/diffle/src/client/ui/dialog";
import "./fonts.css";
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  FolderOpen,
  Settings,
  ArrowLeft,
  X,
  GitPullRequest,
  GitCompareArrows,
  History,
  LoaderCircle,
} from "lucide-react";
const ReviewWorkspace = lazy(() =>
  import("./ReviewWorkspace").then((m) => ({ default: m.ReviewWorkspace })),
);
import { applyTheme, readTheme } from "../vendor/diffle/src/client/theme";
applyTheme(readTheme());
import { bindReview } from "./transport";
import { OpenInEditor } from "./OpenInEditor";
import { GitlabPanel } from "./GitlabPanel";
import "../vendor/diffle/src/client/styles.css";
import "./style.css";
type Request =
  | { kind: "gitlab"; url: string }
  | { kind: "working" }
  | { kind: "pr"; pr?: string }
  | { kind: "revspec" | "range-diff"; args: string[] };
type Recent = { path: string; request: Request; label: string };
type Review = { root: string; id: string; generation: number; gitlab: boolean };
function read<T>(key: string, fallback: T): T {
  try {
    return JSON.parse(localStorage.getItem(key) || "null") ?? fallback;
  } catch {
    return fallback;
  }
}
function Mark() {
  return (
    <svg viewBox="0 0 32 32" aria-hidden="true">
      <path
        d="M5 25V9h8v10h6V9h8v16"
        fill="none"
        stroke="currentColor"
        strokeWidth="4"
      />
      <path d="M5 4h8M19 29h8" stroke="currentColor" strokeWidth="3" />
    </svg>
  );
}
function App() {
  const [review, setReview] = useState<Review | null>(null),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const [recent, setRecent] = useState<Recent[]>(() =>
    read("mdiff.reviews", []),
  );
  const [path, setPath] = useState(""),
    [mode, setMode] = useState("working"),
    [revisions, setRevisions] = useState(""),
    [pr, setPr] = useState("");
  const [settings, setSettings] = useState(false),
    [commands, setCommands] = useState("{}"),
    [hosts, setHosts] = useState("{}");
  const [token, setToken] = useState(""),
    [tokenHost, setTokenHost] = useState("github.com");
  const [lsp, setLsp] = useState(() => read("mdiff.lsp", true));
  const [settingsSaved, setSettingsSaved] = useState(false);
  const [settingsError, setSettingsError] = useState("");
  const [settingsTab, setSettingsTab] = useState("appearance");
  const opening = useRef(false);
  const [gitlabToken, setGitlabToken] = useState("");
  const [gitlabHost, setGitlabHost] = useState("gitlab.com");
  const [discussions, setDiscussions] = useState(false);
  async function launch(folder: string, request: Request, label: string) {
    if (opening.current) return;
    if (!isTauri()) {
      setError("Open the mdiff desktop app to review repositories.");
      return;
    }
    opening.current = true;
    setBusy(true);
    setError("");
    // Unmount the old workspace before switching the native session.
    setReview(null);
    try {
      const opened = await invoke<{ root: string; id: string }>("open_review", {
        options: {
          path: folder || ".",
          request: request.kind === "gitlab" ? { kind: "working" } : request,
          gitlabUrl: request.kind === "gitlab" ? request.url : null,
          gitlabToken: gitlabToken || null,
          gitlabHost,
          token: token || null,
          tokenHost,
          enableLsp: lsp,
        },
      });
      bindReview(opened.id);
      setDiscussions(request.kind === "gitlab");
      setReview({
        ...opened,
        generation: Date.now(),
        gitlab: request.kind === "gitlab",
      });
      const entry = {
        path:
          request.kind === "pr" && request.pr?.startsWith("https://")
            ? folder
            : opened.root,
        request,
        label,
      };
      setRecent((prev) => {
        const next = [
          entry,
          ...prev.filter(
            (r) =>
              r.path !== entry.path ||
              JSON.stringify(r.request) !== JSON.stringify(request),
          ),
        ].slice(0, 12);
        localStorage.setItem("mdiff.reviews", JSON.stringify(next));
        return next;
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      opening.current = false;
    }
  }
  const launchRef = useRef(launch);
  launchRef.current = launch;
  async function choose() {
    try {
      const folder = await open({
        directory: true,
        multiple: false,
        title: "Open a Git repository",
      });
      if (folder) {
        setPath(folder);
        await launch(folder, { kind: "working" }, "Working changes");
      }
    } catch (e) {
      setError(String(e));
    }
  }
  async function selectFolder() {
    try {
      const folder = await open({
        directory: true,
        multiple: false,
        title: "Choose repository",
      });
      if (folder) setPath(folder);
    } catch (e) {
      setError(String(e));
    }
  }
  function start() {
    let request: Request = { kind: "working" },
      label = "Working changes";
    if (mode === "gitlab") {
      request = { kind: "gitlab", url: pr.trim() };
      label = pr.trim();
    } else if (mode === "pr") {
      request = { kind: "pr", ...(pr.trim() ? { pr: pr.trim() } : {}) };
      label = pr.trim() || "Current pull request";
    } else if (mode === "commit") {
      request = { kind: "revspec", args: [`${revisions.trim() || "HEAD"}^!`] };
      label = `Commit ${revisions.trim() || "HEAD"}`;
    } else if (mode === "revspec" || mode === "range-diff") {
      request = {
        kind: mode,
        args: revisions.trim().split(/\s+/).filter(Boolean),
      };
      label = revisions.trim();
    }
    void launch(path, request, label);
  }
  async function home() {
    setReview(null);
    setError("");
    try {
      await invoke("close_review");
    } catch (e) {
      setError(String(e));
    }
  }
  async function showSettings() {
    setSettings(true);
    setSettingsSaved(false);
    setSettingsError("");
    try {
      const config = await invoke<{
        lspCommands: Record<string, string>;
        githubHosts: Record<string, string>;
      }>("desktop_settings");
      setCommands(JSON.stringify(config.lspCommands, null, 2));
      setHosts(JSON.stringify(config.githubHosts, null, 2));
    } catch (e) {
      setSettingsError(String(e));
    }
  }
  async function saveSettings() {
    setSettingsError("");
    setSettingsSaved(false);
    const parseMap = (text: string, label: string) => {
      let value: unknown;
      try {
        value = JSON.parse(text);
      } catch {
        throw new Error(
          `${label}: enter valid JSON, for example {"key":"value"}.`,
        );
      }
      if (
        !value ||
        typeof value !== "object" ||
        Array.isArray(value) ||
        Object.values(value).some((v) => typeof v !== "string")
      ) {
        throw new Error(`${label}: use an object with text values.`);
      }
      return value as Record<string, string>;
    };
    try {
      await invoke("save_desktop_settings", {
        lspCommands: parseMap(commands, "Language commands"),
        githubHosts: parseMap(hosts, "Enterprise host mapping"),
      });
      localStorage.setItem("mdiff.lsp", JSON.stringify(lsp));
      setSettingsSaved(true);
    } catch (e) {
      setSettingsError(String(e));
    }
  }
  useEffect(() => {
    Object.defineProperty(navigator.clipboard, "writeText", {
      configurable: true,
      value: writeText,
    });
    Object.defineProperty(navigator.clipboard, "write", {
      configurable: true,
      value: async (items: ClipboardItem[]) => {
        const text = await Promise.all(
          items
            .filter((i) => i.types.includes("text/plain"))
            .map(async (i) => (await i.getType("text/plain")).text()),
        );
        await writeText(text.join("\n"));
      },
    });
    const external = (event: MouseEvent) => {
      const link = (event.target as Element)?.closest("a[href]");
      if (!(link instanceof HTMLAnchorElement) || !/^https?:/.test(link.href))
        return;
      event.preventDefault();
      void openUrl(link.href).catch((e) => setError(String(e)));
    };
    document.addEventListener("click", external);
    return () => document.removeEventListener("click", external);
  }, []);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "o") {
        e.preventDefault();
        void choose();
      }
      if (e.key === "Escape") setSettings(false);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  });
  useEffect(() => {
    if (!isTauri()) return;
    const result = getCurrentWebviewWindow().onDragDropEvent((e) => {
      if (e.payload.type === "drop" && e.payload.paths[0]) {
        setPath(e.payload.paths[0]);
        void launchRef.current(
          e.payload.paths[0],
          { kind: "working" },
          "Working changes",
        );
      }
    });
    return () => {
      void result.then((unlisten) => unlisten());
    };
  }, []);
  return (
    <div className={`app ${review ? "reviewing" : ""}`}>
      <header className="desktop-chrome">
        <button
          className="brand"
          aria-label="Recent reviews"
          onClick={home}
          disabled={busy}
        >
          <Mark />
          <strong>mdiff</strong>
        </button>
        <span className="header-divider" />
        <span className="subtitle">{review ? review.root : "Code review"}</span>
        <div className="window-actions">
          {review && <OpenInEditor key={review.id} id={review.id} />}
          {review && (
            <button onClick={home}>
              <ArrowLeft size={15} />
              Reviews
            </button>
          )}
          <button onClick={showSettings}>
            <Settings size={15} />
            Preferences
          </button>
          <button className="open" disabled={busy} onClick={choose}>
            <FolderOpen size={15} />
            Open repository
          </button>
        </div>
      </header>
      {error && (
        <div role="alert" className="error">
          <span>{error}</span>
          <button aria-label="Dismiss error" onClick={() => setError("")}>
            <X size={14} />
          </button>
        </div>
      )}
      {busy ? (
        <main className="loading">
          <LoaderCircle size={28} className="spin" />
          <h2>Opening review…</h2>
          <p>Loading changes.</p>
        </main>
      ) : review ? (
        <div className="review-workspace">
          <div className="review-content">
            <Suspense
              fallback={
                <div className="loading">Loading review workspace…</div>
              }
            >
              <ReviewWorkspace
                key={review.id}
                gitlab={review.gitlab}
                onDiscussions={() => setDiscussions((v) => !v)}
              />
            </Suspense>
          </div>
          {review.gitlab && discussions && (
            <GitlabPanel id={review.id} onClose={() => setDiscussions(false)} />
          )}
        </div>
      ) : (
        <main className="home">
          <section className="intro">
            <h1>Review code</h1>
            <p>Open a repository or choose a comparison.</p>
            <button className="primary" onClick={choose}>
              <FolderOpen size={17} />
              Open repository
            </button>
            <small>Or drop a folder here</small>
          </section>
          <div className="home-columns">
            <section className="review-form">
              <h2>
                <GitCompareArrows size={18} />
                New review
              </h2>
              <label>
                Repository
                <div className="folder-field">
                  <input
                    value={path}
                    onChange={(e) => setPath(e.target.value)}
                    placeholder="Choose a local Git repository"
                  />
                  <button onClick={selectFolder} aria-label="Choose repository">
                    <FolderOpen size={16} />
                  </button>
                </div>
              </label>
              <label>
                Comparison
                <select
                  value={mode}
                  onChange={(e) => {
                    setMode(e.target.value);
                    setRevisions("");
                  }}
                >
                  <option value="working">All working changes</option>
                  <option value="revspec">Branch or revision range</option>
                  <option value="commit">Single commit</option>
                  <option value="pr">GitHub pull request</option>
                  <option value="gitlab">GitLab merge request</option>
                  <option value="range-diff">Compare two commit series</option>
                </select>
              </label>
              {mode === "pr" || mode === "gitlab" ? (
                <label>
                  {mode === "gitlab"
                    ? "Merge request URL"
                    : "Pull request URL or number"}
                  <input
                    value={pr}
                    onChange={(e) => setPr(e.target.value)}
                    placeholder={
                      mode === "gitlab"
                        ? "https://gitlab.com/group/project/-/merge_requests/123"
                        : "https://github.com/owner/repo/pull/123"
                    }
                  />
                  <small>
                    {mode === "gitlab"
                      ? "GitLab.com or self-managed."
                      : "Repository is optional for a URL."}
                  </small>
                </label>
              ) : (
                mode !== "working" && (
                  <label>
                    {mode === "range-diff"
                      ? "Commit ranges"
                      : mode === "commit"
                        ? "Commit"
                        : "Branch or revisions"}
                    <input
                      value={revisions}
                      onChange={(e) => setRevisions(e.target.value)}
                      placeholder={
                        mode === "range-diff"
                          ? "main..v1 main..v2"
                          : mode === "commit"
                            ? "HEAD"
                            : "main, main..feature, or main..worktree"
                      }
                    />
                    <small>
                      {mode === "revspec"
                        ? "A single branch compares changes since its merge base."
                        : "Use Git revision names."}
                    </small>
                  </label>
                )
              )}
              <button
                className="primary"
                disabled={
                  (!path &&
                    !(
                      (mode === "pr" || mode === "gitlab") &&
                      pr.startsWith("https://")
                    )) ||
                  ((mode === "revspec" || mode === "range-diff") &&
                    !revisions.trim())
                }
                onClick={start}
              >
                {mode === "pr" || mode === "gitlab" ? (
                  <GitPullRequest size={15} />
                ) : (
                  <GitCompareArrows size={15} />
                )}
                Open review
              </button>
            </section>
            <section className="recent">
              <h2>
                <History size={18} />
                Recent reviews
              </h2>
              {recent.length ? (
                recent.map((r, i) => (
                  <button
                    key={JSON.stringify(r)}
                    onClick={() => launch(r.path, r.request, r.label)}
                  >
                    <FolderOpen size={17} />
                    <span>
                      <strong>
                        {r.path.split("/").pop() || "Pull request"}
                      </strong>
                      <small>{r.label}</small>
                      <small className="recent-path">{r.path}</small>
                    </span>
                  </button>
                ))
              ) : (
                <p>No recent reviews.</p>
              )}
            </section>
          </div>
        </main>
      )}
      {settings && (
        <Dialog open onOpenChange={() => setSettings(false)}>
          <DialogContent className="settings" aria-labelledby="settings-title">
            <button
              className="delete"
              aria-label="Close settings"
              onClick={() => setSettings(false)}
            >
              <X size={18} />
            </button>
            <h2 id="settings-title">Preferences</h2>
            <nav className="preferences-tabs" aria-label="Preference sections">
              {["appearance", "connections", "tools"].map((tab) => (
                <button
                  key={tab}
                  aria-pressed={settingsTab === tab}
                  onClick={() => setSettingsTab(tab)}
                >
                  {tab === "appearance"
                    ? "Appearance"
                    : tab === "connections"
                      ? "Connections"
                      : "Code intelligence"}
                </button>
              ))}
            </nav>
            {settingsTab === "appearance" && (
              <AppearancePanel showHeading={false} />
            )}
            {settingsTab === "connections" && (
              <>
                <p>
                  Tokens stay in memory until you quit. Reopen a review to apply
                  changes.
                </p>
                <h3>GitHub connection</h3>
                <p>Use your GitHub CLI sign-in or enter a token.</p>
                <label>
                  GitHub host
                  <input
                    value={tokenHost}
                    onChange={(e) => setTokenHost(e.target.value.trim())}
                    placeholder="github.com"
                  />
                </label>
                <label>
                  Personal access token
                  <input
                    type="password"
                    autoComplete="off"
                    spellCheck={false}
                    value={token}
                    onChange={(e) => setToken(e.target.value)}
                    placeholder="Optional when already signed in with GitHub CLI"
                  />
                </label>
                <button onClick={() => setToken("")} disabled={!token}>
                  Clear session token
                </button>
                <h3>GitLab connection</h3>
                <p>
                  GitLab.com or self-managed. Token scopes: api,
                  read_repository.
                </p>
                <label>
                  GitLab host (include port when needed)
                  <input
                    value={gitlabHost}
                    onChange={(e) => setGitlabHost(e.target.value.trim())}
                    placeholder="gitlab.com"
                  />
                </label>
                <label>
                  GitLab personal access token
                  <input
                    type="password"
                    autoComplete="off"
                    spellCheck={false}
                    value={gitlabToken}
                    onChange={(e) => setGitlabToken(e.target.value)}
                  />
                </label>
                <button
                  onClick={() => setGitlabToken("")}
                  disabled={!gitlabToken}
                >
                  Clear GitLab token
                </button>
              </>
            )}
            {settingsTab === "tools" && (
              <>
                <h3>Language servers</h3>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={lsp}
                    onChange={(e) => {
                      setLsp(e.target.checked);
                      setSettingsSaved(false);
                    }}
                  />
                  Enable code intelligence and change graphs
                </label>
                <p>
                  Detected from PATH. Set commands by language; an empty value
                  disables it.
                </p>
                <label>
                  Language commands (JSON)
                  <textarea
                    spellCheck={false}
                    value={commands}
                    onChange={(e) => {
                      setCommands(e.target.value);
                      setSettingsSaved(false);
                    }}
                    placeholder={
                      '{"rust":"rust-analyzer","python":"pyright-langserver --stdio"}'
                    }
                  />
                </label>
              </>
            )}
            {settingsTab === "connections" && (
              <details>
                <summary>Enterprise host mapping</summary>
                <label>
                  Git remote host → GitHub web host (JSON)
                  <textarea
                    spellCheck={false}
                    value={hosts}
                    onChange={(e) => {
                      setHosts(e.target.value);
                      setSettingsSaved(false);
                    }}
                  />
                </label>
              </details>
            )}
            {settingsError && (
              <p className="settings-error" role="alert">
                {settingsError.replace(/^Error: /, "")}
              </p>
            )}
            {settingsTab !== "appearance" && (
              <div className="settings-footer">
                <span role="status">
                  {settingsSaved ? "Saved. Reopen your review to apply." : ""}
                </span>
                <button className="primary" onClick={saveSettings}>
                  Save settings
                </button>
              </div>
            )}
          </DialogContent>
        </Dialog>
      )}
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<App />);
