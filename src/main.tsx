import {
  formatLocale,
  currentLocale,
} from "../vendor/diffle/src/client/i18n/locale";
import { uiText } from "../vendor/diffle/src/client/i18n/runtime";
import { Bell, Bot } from "lucide-react";
import type { Target as AttentionTarget } from "./attention/api";
import type { Activity as AgentActivity } from "./agents/api";
import type { Worktree, Kind as ExternalKind } from "./worktrees/api";
import type { Summary as HostedSummary } from "./hosting/api";
import { listen } from "@tauri-apps/api/event";
import { useShellPreferences } from "./workspaces/preferences";
import { useWorkspaces } from "./workspaces/model";
import type {
  Activated,
  Document as WorkspaceDocument,
  Operation,
  View as WorkspaceView,
  Request,
  ConnectionProfiles,
} from "./workspaces/types";
import type {
  EnterRequest,
  HistoryInfo,
} from "../vendor/diffle/src/shared/protocol";
import type { HistoryTransport } from "../vendor/diffle/src/client/history/api";
import {
  coreMessages as messages,
  translate,
} from "../vendor/diffle/src/client/i18n/runtime";
import {
  CommandProvider,
  useCommands,
  useCommandVersion,
} from "../vendor/diffle/src/client/commands/CommandProvider";
import { shortcutLabel } from "../vendor/diffle/src/client/commands/shortcuts";
import { AppearancePanel } from "../vendor/diffle/src/client/appearance/AppearancePanel";
import { Dialog, DialogContent } from "../vendor/diffle/src/client/ui/dialog";
import "./fonts.css";
import {
  lazy,
  memo,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open, confirm } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Search,
  FolderOpen,
  Settings,
  ArrowLeft,
  X,
  GitPullRequest,
  GitCompareArrows,
  GitFork,
  GitCommitHorizontal,
  History,
  Terminal,
  LoaderCircle,
} from "lucide-react";
import { applyTheme, readTheme } from "../vendor/diffle/src/client/theme";
applyTheme(readTheme());
document.documentElement.lang = formatLocale;
document.documentElement.dir = "ltr";
if (isTauri())
  void Promise.resolve()
    .then(() => invoke("desktop_locale", { locale: currentLocale }))
    .catch(() => {});
import { BrandMark } from "./Brand";
import { About } from "./About";
import { Updates, updates } from "./Updates";
import { GitlabPanel } from "./GitlabPanel";
import "../vendor/diffle/src/client/styles.css";
import "./style.css";

const GitOperations = lazy(() => import("./git-operations/GitOperations"));
const AgentPanel = lazy(() => import("./agents/AgentPanel"));
const AttentionHub = lazy(() => import("./attention/AttentionHub"));
const WorktreesHub = lazy(() => import("./worktrees/WorktreesHub"));
const HostingHub = lazy(() => import("./hosting/HostingHub"));
const WorkspaceBar = lazy(() =>
  import("./workspaces/WorkspaceBar").then((m) => ({
    default: m.WorkspaceBar,
  })),
);
const HistoryBrowser = lazy(
  () => import("../vendor/diffle/src/client/history/HistoryBrowser"),
);

const ShortcutSettings = lazy(() =>
  import("../vendor/diffle/src/client/commands/ShortcutSettings").then((m) => ({
    default: m.ShortcutSettings,
  })),
);

const loadReviewModule = () => import("./ReviewWorkspace");
type WorkspaceModule = Awaited<ReturnType<typeof loadReviewModule>>;
function Environment({
  module,
  children,
}: {
  module: WorkspaceModule | null;
  children: ReactNode;
}) {
  if (!module) return children;
  const Provider = module.ReviewEnvironment;
  return <Provider>{children}</Provider>;
}

type Recent = { path: string; request: Request; label: string };
type Review = {
  tab: string;
  root: string;
  id: string;
  gitlab: boolean;
  github_enabled: boolean;
  key: string;
  lsp_enabled?: boolean;
};
function read<T>(key: string, fallback: T): T {
  try {
    return JSON.parse(localStorage.getItem(key) || "null") ?? fallback;
  } catch {
    return fallback;
  }
}
function App() {
  const registry = useCommands();
  useCommandVersion(registry);
  const workspaces = useWorkspaces(() => read<Recent[]>("mdiff.reviews", []));
  const activeWorkspace = workspaces.document?.workspaces.find(
    (w) => w.id === workspaces.document?.activeWorkspace,
  );
  const viewRevisions = useRef<Record<string, number>>({});
  const runtimes = useRef<Record<string, string>>({});
  const saving = useRef<Promise<unknown>>(Promise.resolve());
  const [closing, setClosing] = useState(false);
  const flushOnClose = useRef<(exiting?: boolean) => Promise<unknown>>(
    async () => {},
  );
  const closingPending = useRef(false);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [notice, setNotice] = useState("");
  const [draftViewer, setDraftViewer] = useState<WorkspaceView | null>(null);

  const [repository, setRepository] = useState<{
    id: string;
    info: HistoryInfo;
    transport: HistoryTransport;
  } | null>(null);
  const [toolsPanel, setToolsPanel] = useState<{
    id: string;
    initial: "worktrees" | ExternalKind;
  } | null>(null);
  const [worktreeCache, setWorktreeCache] = useState<{
    id: string;
    rows: Worktree[];
  } | null>(null);
  const toolsSequence = useRef(0);
  const [gitPanel, setGitPanel] = useState<{
    context: string | null;
    initialSection?: "Advanced and recovery";
  } | null>(null);
  const gitFlushRef = useRef<() => Promise<unknown>>(async () => {});
  const [agentLoaded, setAgentLoaded] = useState(false),
    [agentOpen, setAgentOpen] = useState(false),
    [agentActivity, setAgentActivity] = useState<AgentActivity[]>([]);
  const showAgents = () => {
    setAgentLoaded(true);
    setAgentOpen(true);
  };
  const [terminalModule, setTerminalModule] = useState<
    typeof import("./terminal") | null
  >(null);
  const [terminalVisible, setTerminalVisible] = useState(false);
  async function showTerminal(context?: string, worktree?: string) {
    try {
      const module = terminalModule ?? (await import("./terminal"));
      await module.terminalStore.init();
      setTerminalModule(module);
      setTerminalVisible(true);
      if (context) {
        await module.terminalStore.start(context, worktree ?? null);
        setToolsPanel(null);
      }
    } catch (error) {
      setError(String(error));
      if (context) throw error;
    }
  }

  async function openWorktrees(
    initial: "worktrees" | ExternalKind | "git" = "worktrees",
    forceChoose = false,
  ) {
    if (historyOpening.current || opening.current) return;
    const ticket = ++toolsSequence.current;
    let id = historyOpen ? repository?.id : (review?.id ?? repository?.id);
    if (forceChoose) id = undefined;
    try {
      if (!id) {
        const folder =
          (!forceChoose && path) ||
          (await open({
            directory: true,
            multiple: false,
            title: uiText("Choose repository for worktrees and tools"),
          }));
        if (!folder || ticket !== toolsSequence.current) return;
        const context = await invoke<{ id: string; info: HistoryInfo }>(
          "open_repository_context",
          { path: folder, review: null },
        );
        if (ticket !== toolsSequence.current) {
          void invoke("close_repository_context", { id: context.id });
          return;
        }
        const { repositoryTransport } = await import("./history");
        setRepository({
          ...context,
          transport: repositoryTransport(context.id),
        });
        setPath(context.info.root);
        id = context.id;
      }
      if (initial === "git") setGitPanel({ context: id });
      else setToolsPanel({ id, initial });
    } catch (error) {
      setError(String(error));
    }
  }
  async function openWorktree(path: string) {
    const existing = workspaces.current.current?.workspaces
      .find(
        (workspace) =>
          workspace.id === workspaces.current.current?.activeWorkspace,
      )
      ?.tabs.find(
        (tab) =>
          tab.source.path === path && tab.source.request.kind === "working",
      );
    const id = existing
      ? await activateTab(existing.id)
      : await launch(path, { kind: "working" }, "Working changes");
    if (!id)
      throw Error(
        "Could not open the worktree. Close the worktree browser to inspect the opening error.",
      );
    setToolsPanel(null);
  }
  const [historyOpen, setHistoryOpen] = useState(false);
  const historyOpening = useRef(false);
  const historySequence = useRef(0);
  async function openHistory(chooseNew = false) {
    if (historyOpening.current || opening.current) return;
    if (
      !chooseNew &&
      repository &&
      ((!review && (!path || path === repository.info.root)) ||
        repository.info.root === review?.root)
    ) {
      setHistoryOpen(true);
      return;
    }
    const ticket = ++historySequence.current;
    historyOpening.current = true;
    setError("");
    try {
      const folder =
        (!chooseNew && (review?.root || path)) ||
        (await open({
          directory: true,
          multiple: false,
          title: uiText("Browse repository history"),
        }));
      if (!folder) return;
      const context = await invoke<{ id: string; info: HistoryInfo }>(
        "open_repository_context",
        { path: folder, review: chooseNew ? null : (review?.id ?? null) },
      );
      if (ticket !== historySequence.current) {
        void invoke("close_repository_context", { id: context.id });
        return;
      }
      const { repositoryTransport } = await import("./history");
      setPath(context.info.root);
      setRepository({ ...context, transport: repositoryTransport(context.id) });
      setHistoryOpen(true);
    } catch (error) {
      setError(String(error));
    } finally {
      historyOpening.current = false;
    }
  }
  const [workspaceModule, setWorkspaceModule] =
    useState<WorkspaceModule | null>(null);
  const ReviewWorkspace = workspaceModule?.ReviewWorkspace;
  const ReviewChrome = workspaceModule?.ReviewChrome;
  const [review, setReview] = useState<Review | null>(null),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const shellPreferences = useShellPreferences(registry, setError);
  const lspChanged = useCallback(
    (enabled: boolean) =>
      setReview((current) =>
        current && current.lsp_enabled !== enabled
          ? { ...current, lsp_enabled: enabled }
          : current,
      ),
    [],
  );
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
  const [githubEnabled, setGithubEnabled] = useState(true);
  const [githubDraft, setGithubDraft] = useState(true);
  const [settingsReady, setSettingsReady] = useState(false);
  const [settingsLoading, setSettingsLoading] = useState(false);
  const [settingsSaving, setSettingsSaving] = useState(false);
  const [settingsSaved, setSettingsSaved] = useState(false);
  const [settingsError, setSettingsError] = useState("");
  const settingsRevision = useRef(0);
  const [settingsTab, setSettingsTab] = useState("appearance");
  const opening = useRef(false);
  const [gitlabToken, setGitlabToken] = useState("");
  const [gitlabHost, setGitlabHost] = useState("gitlab.com");
  const [hostingResults, setHostingResults] = useState<HostedSummary[]>([]);
  const [hostingOpen, setHostingOpen] = useState(false);
  const [attentionOpen, setAttentionOpen] = useState(false),
    [attentionUnread, setAttentionUnread] = useState(0),
    [attentionTarget, setAttentionTarget] = useState<AttentionTarget | null>(
      null,
    );
  const [discussions, setDiscussions] = useState(false);
  const toggleDiscussions = useCallback(() => setDiscussions((v) => !v), []);
  const persistReview = useCallback(
    (target: Review) => {
      if (saveTimer.current) {
        clearTimeout(saveTimer.current);
        saveTimer.current = null;
      }
      const run = saving.current.then(async () => {
        const view = workspaceModule?.captureWorkspace?.(target.id);
        if (!view) return;
        const revision = viewRevisions.current[target.tab] ?? 0;
        let saved: WorkspaceView;
        try {
          saved = await invoke<WorkspaceView>("workspace_save_view", {
            tab: target.tab,
            revision,
            view: { ...view, revision },
          });
        } catch (error) {
          // The view may have committed before its metadata draft count failed.
          // Reconcile only an identical durable value; another writer's edits still conflict.
          const durable = await invoke<WorkspaceView>("workspace_read_view", {
            tab: target.tab,
          }).catch(() => null);
          if (
            durable &&
            JSON.stringify([durable.unsent, durable.navigation]) ===
              JSON.stringify([view.unsent, view.navigation])
          )
            viewRevisions.current[target.tab] = durable.revision;
          throw error;
        }
        viewRevisions.current[target.tab] = saved.revision;
        workspaceModule?.acknowledgeWorkspaceSaved(target.id);
        const doc = await invoke<WorkspaceDocument>("workspace_capture", {
          tab: target.tab,
        });
        workspaces.accept(doc);
      });
      saving.current = run.then(
        () => {},
        () => {},
      );
      return run;
    },
    [workspaceModule, workspaces.accept],
  );
  useEffect(() => {
    if (!review || !workspaceModule?.subscribeWorkspace) return;
    const stop = workspaceModule.subscribeWorkspace(review.id, () => {
      if (saveTimer.current) clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => {
        saveTimer.current = null;
        void persistReview(review).catch((e) =>
          setError(
            `Tab state could not be saved. Keep this tab open: ${String(e)}`,
          ),
        );
      }, 650);
    });
    return () => {
      stop();
      if (saveTimer.current) {
        clearTimeout(saveTimer.current);
        saveTimer.current = null;
      }
    };
  }, [review?.id, workspaceModule, persistReview]);
  async function activateTab(
    tab: string,
    refresh = false,
    previous: Review | null = review,
  ) {
    if (opening.current) return null;
    if (!isTauri()) {
      setError("Open the mdiff desktop app to review repositories.");
      return null;
    }
    opening.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    historySequence.current++;
    try {
      if (previous) await persistReview(previous);
      const loading = loadReviewModule();
      void loading.catch(() => {});
      const result = await invoke<Activated>("workspace_activate", {
        tab,
        refresh,
      });
      const workspace = await loading;
      viewRevisions.current[result.tab.id] = result.view.revision;
      runtimes.current[result.tab.id] = result.opened.id;
      workspace.initializeWorkspace(
        result.opened.id,
        result.opened.key,
        result.tab.id,
        result.view,
      );
      if (result.opened.id !== review?.id) flushSync(() => setReview(null));
      setWorkspaceModule(workspace);
      setHistoryOpen(false);
      setDiscussions(result.gitlab);
      flushSync(() =>
        setReview({
          ...result.opened,
          tab: result.tab.id,
          gitlab: result.gitlab,
        }),
      );
      setNotice(
        result.notice ??
          ((result.tab.source.request.kind === "pr" ||
            result.tab.source.request.kind === "gitlab") &&
          !result.opened.github_enabled &&
          !result.gitlab
            ? "Saved revision restored offline. Refresh source to reconnect hosting."
            : ""),
      );
      await workspaces.load();
      return result.opened.id;
    } catch (e) {
      setError(String(e));
      return null;
    } finally {
      setBusy(false);
      opening.current = false;
    }
  }
  async function launch(folder: string, request: Request, label: string) {
    if (opening.current) return null;
    if (request.kind === "pr" && !githubEnabled) {
      setError(
        "GitHub integration is disabled. Enable it in Preferences → Connections.",
      );
      return null;
    }
    try {
      if (!activeWorkspace)
        throw Error("Workspaces are still loading. Try again.");
      const doc = await workspaces.update({
        kind: "addTab",
        workspace: activeWorkspace.id,
        source: { path: folder, request, label },
      });
      const tab = doc.workspaces.find(
        (w) => w.id === activeWorkspace.id,
      )?.active;
      return tab ? await activateTab(tab) : null;
    } catch (e) {
      setError(String(e));
      return null;
    }
  }
  async function workspaceOperation(operation: Operation) {
    if (opening.current)
      throw Error("Wait for the current review to finish opening.");
    const oldWorkspace = workspaces.current.current?.activeWorkspace;
    let doc: WorkspaceDocument;
    opening.current = true;
    setBusy(true);
    try {
      if (review) await persistReview(review);
      doc = await workspaces.update(operation);
    } finally {
      opening.current = false;
      setBusy(false);
    }
    const retained = new Set(
      doc.workspaces.flatMap((w) => w.tabs.map((t) => t.id)),
    );
    for (const [tab, runtime] of Object.entries(runtimes.current)) {
      if (!retained.has(tab) && tab !== review?.tab) {
        workspaceModule?.closeWorkspace(runtime);
        delete runtimes.current[tab];
      }
    }
    if (operation.kind === "closeTab") {
      if (review?.tab === operation.id) {
        const current = review;
        flushSync(() => setReview(null));
        workspaceModule?.closeWorkspace(current.id);
        const next = doc.workspaces.find(
          (w) => w.id === doc.activeWorkspace,
        )?.active;
        if (next) await activateTab(next, false, null);
      }
    } else if (operation.kind === "setPolicy" && review?.tab === operation.id) {
      workspaceModule?.closeWorkspace(review.id);
      flushSync(() => setReview(null));
      await activateTab(operation.id, false, null);
    } else if (operation.kind === "reopenTab") {
      const next = doc.workspaces.find(
        (w) => w.id === doc.activeWorkspace,
      )?.active;
      if (next) await activateTab(next);
    } else if (
      doc.activeWorkspace !== oldWorkspace ||
      (review &&
        !doc.workspaces
          .find((w) => w.id === doc.activeWorkspace)
          ?.tabs.some((t) => t.id === review.tab))
    ) {
      await home(true);
    }
  }
  async function workspaceWindow(
    action: "open" | "duplicate" | "move",
    tab?: string,
    target?: string,
  ) {
    if (opening.current || !activeWorkspace) return;
    opening.current = true;
    setBusy(true);
    setError("");
    try {
      await flushOnClose.current(false);
      const doc = await invoke<WorkspaceDocument>(
        action === "move" ? "workspace_move_window" : "workspace_open_window",
        {
          workspace: activeWorkspace.id,
          duplicateTab: action === "duplicate" ? tab : null,
          tab,
          target,
          revision: workspaces.current.current!.revision,
        },
      );
      workspaces.accept(doc);
      if (
        (action === "open" && doc.activeWorkspace !== activeWorkspace.id) ||
        (action === "move" && tab === review?.tab)
      ) {
        if (review) workspaceModule?.closeWorkspace(review.id);
        flushSync(() => setReview(null));
        setHistoryOpen(false);
        setRepository(null);
        historySequence.current++;
      }
      if (action === "move" && tab && runtimes.current[tab]) {
        workspaceModule?.closeWorkspace(runtimes.current[tab]);
        delete runtimes.current[tab];
      }
    } catch (e) {
      setError(String(e));
      await workspaces.load().catch(() => {});
    } finally {
      opening.current = false;
      setBusy(false);
    }
  }
  async function unloadTab(tab: string) {
    try {
      if (review?.tab === tab) {
        await persistReview(review);
        workspaceModule?.closeWorkspace(review.id);
        flushSync(() => setReview(null));
      }
      if (runtimes.current[tab])
        workspaceModule?.closeWorkspace(runtimes.current[tab]);
      await invoke("workspace_release", { tab });
      setNotice("Review unloaded. Its tab and saved drafts are kept.");
    } catch (e) {
      setError(String(e));
    }
  }
  async function showDrafts(tab: string) {
    try {
      const local =
        review?.tab === tab
          ? workspaceModule?.captureWorkspace(review.id)
          : null;
      setDraftViewer(
        local
          ? { revision: viewRevisions.current[tab] ?? 0, ...local }
          : await invoke<WorkspaceView>("workspace_read_view", { tab }),
      );
    } catch (e) {
      setError(String(e));
    }
  }
  const connectionChanged = useRef<(auth: boolean) => Promise<void>>(
    async () => {},
  );
  connectionChanged.current = async (authChanged) => {
    const config = await invoke<{ githubEnabled: boolean }>("desktop_settings");
    setGithubEnabled(config.githubEnabled);
    if (review && authChanged) {
      await home();
      setNotice(
        "Connection settings changed. Select the tab to reopen with the current connection.",
      );
    }
  };
  flushOnClose.current = async (exiting = true) => {
    await gitFlushRef.current();
    if (review) await persistReview(review);
    await saving.current;
    await shellPreferences.flush.current();
    await updates.settled();
    if (exiting && isTauri()) {
      const terminals = await invoke<{ running: boolean }[]>("terminal_list");
      const count = terminals.filter((t) => t.running).length;
      if (
        count &&
        !(await confirm(
          `Stop ${count} running terminal shell${count === 1 ? "" : "s"} and their jobs before closing? Terminal output is not saved.`,
          { title: uiText("Close terminals and window"), kind: "warning" },
        ))
      )
        throw new DOMException("Terminal close cancelled", "AbortError");
    }
  };
  useEffect(() => {
    if (!isTauri()) return;
    let live = true;
    const stops: (() => void)[] = [];
    const track = (promise: Promise<() => void>) => {
      void promise
        .then((stop) => {
          if (live) stops.push(stop);
          else stop();
        })
        .catch((e) => live && setError(String(e)));
    };
    track(
      listen("git-repository-changed", () => {
        setWorktreeCache(null);
        setNotice(
          "Git operation finished. Related reviews refresh from disk and keep their drafts; inspect Git → Operations and refresh History refs if needed.",
        );
      }),
    );
    track(
      listen<string>("terminal-shutdown-error", (event) =>
        setError(event.payload),
      ),
    );
    track(
      listen("workspace-exit-cancelled", () => {
        closingPending.current = false;
        setClosing(false);
        updates.cancelRestart();
        setNotice("Exit cancelled. Your windows remain open.");
      }),
    );
    track(
      listen<{ authChanged: boolean }>("desktop-settings", (event) => {
        setHostingResults([]);
        void connectionChanged
          .current(event.payload.authChanged)
          .catch((e) => setError(String(e)));
      }),
    );
    const windowLabel = getCurrentWebviewWindow().label;
    void listen<
      string | { owner?: string; action: string; generation: number | null }
    >(
      "workspace-flush",
      (event) => {
        if (
          typeof event.payload !== "string" &&
          event.payload.owner &&
          event.payload.owner !== windowLabel
        )
          return;
        if (closingPending.current) return;
        closingPending.current = true;
        const request =
          typeof event.payload === "string"
            ? { action: event.payload, generation: null }
            : event.payload;
        setClosing(true);
        void flushOnClose
          .current()
          .then(() =>
            invoke("workspace_finish_exit", {
              action: request.action,
              generation: request.generation,
            }),
          )
          .catch((e) => {
            void invoke("workspace_cancel_exit", {
              generation: request.generation,
            }).catch(() => {});
            setClosing(false);
            setError(
              e instanceof DOMException && e.name === "AbortError"
                ? ""
                : `Could not save tab state or stop terminals; the window stayed open. ${String(e)}`,
            );
          })
          .finally(() => {
            closingPending.current = false;
          });
      },
      { target: { kind: "WebviewWindow", label: windowLabel } },
    )
      .then((unlisten) => {
        if (!live) {
          unlisten();
          return;
        }
        stops.push(unlisten);
        void invoke("workspace_register_ui").catch((e) => setError(String(e)));
      })
      .catch((e) => setError(String(e)));
    return () => {
      live = false;
      stops.forEach((stop) => stop());
    };
  }, []);
  const launchRef = useRef(launch);
  launchRef.current = launch;
  async function choose() {
    if (opening.current) return;
    try {
      const folder = await open({
        directory: true,
        multiple: false,
        title: uiText("Open a Git repository"),
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
        title: uiText("Choose repository"),
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
  async function home(skipSave = false) {
    if (opening.current) return;
    try {
      if (review) {
        if (!skipSave) await persistReview(review);
        await invoke("suspend_review", { id: review.id, evict: false }).catch(
          (e) => {
            if (!skipSave) throw e;
          },
        );
      }
      historySequence.current++;
      setHistoryOpen(false);
      flushSync(() => setReview(null));
      setError("");
    } catch (e) {
      setError(String(e));
    }
  }
  const showSettings = useCallback(async () => {
    setSettings(true);
    setSettingsSaved(false);
    setSettingsError("");
    setSettingsLoading(true);
    setSettingsReady(false);
    try {
      const config = await invoke<{
        lspCommands: Record<string, string>;
        githubHosts: Record<string, string>;
        githubEnabled: boolean;
        settingsRevision: number;
        connectionProfiles?: ConnectionProfiles;
      }>("desktop_settings");
      settingsRevision.current = config.settingsRevision ?? 0;
      setToken(config.connectionProfiles?.token ?? "");
      setTokenHost(config.connectionProfiles?.tokenHost ?? "github.com");
      setGitlabToken(config.connectionProfiles?.gitlabToken ?? "");
      setGitlabHost(config.connectionProfiles?.gitlabHost ?? "gitlab.com");
      setCommands(JSON.stringify(config.lspCommands, null, 2));
      setHosts(JSON.stringify(config.githubHosts, null, 2));
      setGithubEnabled(config.githubEnabled);
      setGithubDraft(config.githubEnabled);
      setSettingsReady(true);
    } catch (e) {
      setSettingsError(String(e));
    } finally {
      setSettingsLoading(false);
    }
  }, []);
  const showConnections = useCallback(() => {
    setSettingsTab("connections");
    void showSettings();
  }, [showSettings]);
  async function saveSettings() {
    if (!settingsReady || settingsLoading || settingsSaving) return;
    setSettingsSaving(true);
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
        githubEnabled: githubDraft,
        revision: settingsRevision.current,
        connectionProfiles: {
          token: token || null,
          tokenHost,
          gitlabToken: gitlabToken || null,
          gitlabHost,
        },
      });
      settingsRevision.current += 1;
      setGithubEnabled(githubDraft);
      setReview((r) =>
        r
          ? {
              ...r,
              github_enabled: githubDraft && r.github_enabled && !r.gitlab,
            }
          : r,
      );
      if (!githubDraft) {
        setToken("");
        if (mode === "pr") setMode("working");
      }
      setSettingsSaved(true);
    } catch (e) {
      setSettingsError(String(e));
    } finally {
      setSettingsSaving(false);
    }
  }
  useEffect(() => {
    if (isTauri()) {
      void invoke<{ githubEnabled: boolean }>("desktop_settings")
        .then((config) => setGithubEnabled(config.githubEnabled))
        .catch((e) => setError(String(e)));
    }
  }, []);
  useEffect(() => {
    if (activeWorkspace) setRecent(activeWorkspace.recent);
  }, [activeWorkspace]);
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
  const commandState = useRef({
    busy,
    review,
    recent,
    choose,
    home,
    showSettings,
    openHistory,
    launch,
    activateTab,
    workspaceOperation,
    activeWorkspace,
    hostingResults,
    worktreeCache,
    openWorktrees,
    openWorktree,
    showTerminal,
    showAgents,
    toolsContext: historyOpen ? repository?.id : (review?.id ?? repository?.id),
  });
  commandState.current = {
    busy,
    review,
    recent,
    choose,
    home,
    showSettings,
    openHistory,
    launch,
    activateTab,
    workspaceOperation,
    activeWorkspace,
    hostingResults,
    worktreeCache,
    openWorktrees,
    openWorktree,
    showTerminal,
    showAgents,
    toolsContext: historyOpen ? repository?.id : (review?.id ?? repository?.id),
  };
  useEffect(() => {
    if (!isTauri()) return;
    let live = true,
      revision = -1;
    const badge = () => {
      void invoke<{ revision: number; unread: number }>("attention_badge")
        .then((v) => {
          if (live && v.revision >= revision) {
            revision = v.revision;
            setAttentionUnread(v.unread);
          }
        })
        .catch(() => {});
    };
    const listeners = [
      listen<{ revision: number; unread: number }>("attention-changed", (e) => {
        if (e.payload.revision >= revision) {
          revision = e.payload.revision;
          setAttentionUnread(e.payload.unread);
        }
      }),
      listen("attention-settings", badge),
      listen("attention-open", () => {
        setAttentionTarget(null);
        setAttentionOpen(true);
      }),
      listen<AttentionTarget>("attention-route", (e) => {
        const target = e.payload;
        void (async () => {
          if (commandState.current.busy)
            throw Error(
              "Finish opening the current review, then open the inbox item again.",
            );
          await gitFlushRef.current();
          flushSync(() => {
            setGitPanel(null);
            setHostingOpen(false);
            setToolsPanel(null);
            setAgentOpen(false);
          });
          if (target.kind === "checks") {
            setAttentionTarget(target);
            setAttentionOpen(true);
            return;
          }
          if (target.kind === "review" && target.review) {
            const r = target.review;
            const url = `https://${r.site.host}/${r.repository}/${r.site.provider === "github" ? "pull" : "-/merge_requests"}/${r.number}`;
            const id = await commandState.current.launch(
              "",
              r.site.provider === "github"
                ? { kind: "pr", pr: url }
                : { kind: "gitlab", url },
              `${r.repository} #${r.number}`,
            );
            if (!id)
              throw Error(
                "Could not open the requested review. Inspect Connections and retry from the inbox.",
              );
          } else if (target.kind === "conflicts" && target.tab) {
            if (
              target.workspace &&
              target.workspace !== commandState.current.activeWorkspace?.id
            )
              await commandState.current.workspaceOperation({
                kind: "selectWorkspace",
                id: target.workspace,
              });
            const context = await commandState.current.activateTab(
              target.tab,
              false,
            );
            if (!context)
              throw Error("Could not activate the conflict checkout.");
            setGitPanel({ context, initialSection: "Advanced and recovery" });
          } else if (target.kind === "agents") {
            if (target.terminal) {
              const module = await import("./terminal");
              await module.terminalStore.init();
              if (
                !module.terminalStore
                  .sessions()
                  .some((s) => s.info?.id === target.terminal)
              )
                throw Error("The owned terminal is no longer available.");
              module.terminalStore.select(target.terminal);
              setTerminalModule(module);
              setTerminalVisible(true);
              requestAnimationFrame(() =>
                module.terminalStore
                  .sessions()
                  .find((s) => s.info?.id === target.terminal)
                  ?.term.focus(),
              );
            } else commandState.current.showAgents();
          }
          setAttentionOpen(false);
        })().catch((e) => setError(String(e)));
      }),
    ];
    badge();
    return () => {
      live = false;
      for (const p of listeners) void p.then((stop) => stop());
    };
  }, []);
  useEffect(() => {
    if (!registry) return;
    const available = () =>
      commandState.current.busy ? translate(messages.opening) : null;
    const remove = registry.register({
      "app.gitOperations": {
        run: () =>
          setGitPanel({ context: commandState.current.toolsContext ?? null }),
        unavailable: available,
      },
      "app.worktrees": {
        run: () => commandState.current.openWorktrees(),
        unavailable: available,
      },
      "app.agents": {
        run: () => commandState.current.showAgents(),
        unavailable: available,
      },
      "app.attention": {
        run: () => {
          setAttentionTarget(null);
          setAttentionOpen(true);
        },
        unavailable: available,
      },
      "app.embeddedTerminal": {
        run: () => commandState.current.showTerminal(),
        unavailable: available,
      },
      "app.terminal": {
        run: () => commandState.current.openWorktrees("terminal"),
        unavailable: available,
      },
      "app.editor": {
        run: () => commandState.current.openWorktrees("editor"),
        unavailable: available,
      },
      "app.hosting": {
        run: () => setHostingOpen(true),
        unavailable: available,
      },
      "app.newTab": {
        run: () => commandState.current.home(),
        unavailable: available,
      },
      "app.closeTab": {
        run: () => {
          const c = commandState.current;
          return c.review
            ? c.workspaceOperation({ kind: "closeTab", id: c.review.tab })
            : false;
        },
        unavailable: () =>
          available() ??
          (commandState.current.review ? null : "Open a review tab first."),
      },
      "app.reopenTab": {
        run: () =>
          commandState.current.workspaceOperation({ kind: "reopenTab" }),
        unavailable: available,
      },
      "app.nextTab": {
        run: () => {
          const c = commandState.current;
          const tabs = c.activeWorkspace?.tabs ?? [];
          const i = tabs.findIndex((t) => t.id === c.review?.tab);
          return tabs.length
            ? c.activateTab(tabs[(i + 1) % tabs.length]!.id)
            : false;
        },
        unavailable: available,
      },
      "app.previousTab": {
        run: () => {
          const c = commandState.current;
          const tabs = c.activeWorkspace?.tabs ?? [];
          const i = tabs.findIndex((t) => t.id === c.review?.tab);
          return tabs.length
            ? c.activateTab(tabs[(i - 1 + tabs.length) % tabs.length]!.id)
            : false;
        },
        unavailable: available,
      },
      "app.history": {
        run: () => commandState.current.openHistory(),
        unavailable: available,
      },
      "app.changes": { run: () => setHistoryOpen(false) },
      "app.openRepository": {
        run: () => commandState.current.choose(),
        unavailable: available,
      },
      "app.preferences": {
        run: () => commandState.current.showSettings(),
        unavailable: available,
      },
      "app.home": {
        run: () => commandState.current.home(),
        unavailable: () =>
          available() ??
          (commandState.current.review ? null : translate(messages.home)),
      },
    });
    const removeSource = registry.registerSource({
      id: "desktop.recent",
      group: "recent",
      open() {
        const entries = commandState.current.recent.slice(0, 12);
        return {
          async search(query, signal) {
            const { rankMatches } =
              await import("../vendor/diffle/src/client/commands/search");
            if (signal.aborted) return [];
            return rankMatches(
              entries,
              query,
              (entry) => `${entry.path} ${entry.label}`,
            ).map((entry, index) => ({
              id: `recent:${index}`,
              group: "recent" as const,
              label: entry.path.split("/").pop() || entry.label,
              detail: `${entry.label} · ${entry.path}`,
              reason: available(),
              run: () =>
                commandState.current.busy
                  ? false
                  : commandState.current.launch(
                      entry.path,
                      entry.request,
                      entry.label,
                    ),
            }));
          },
        };
      },
    });
    const removeHostingSource = registry.registerSource({
      id: "desktop.hosting",
      group: "hosting",
      open: () => ({
        async search(query, signal) {
          const { hostingEntries } = await import("./hosting/palette");
          if (signal.aborted) return [];
          return hostingEntries(
            commandState.current.hostingResults,
            query,
            () => commandState.current.busy,
            (reference, title) =>
              commandState.current.launch(
                "",
                reference.site.provider === "github"
                  ? {
                      kind: "pr",
                      pr: `https://${reference.site.host}/${reference.repository}/pull/${reference.number}`,
                    }
                  : {
                      kind: "gitlab",
                      url: `https://${reference.site.host}/${reference.repository}/-/merge_requests/${reference.number}`,
                    },
                title,
              ),
          );
        },
      }),
    });
    const removeWorktrees = registry.registerSource({
      id: "desktop.worktrees",
      group: "worktrees",
      open: () => ({
        async search(query, signal) {
          const { worktreeEntries } = await import("./worktrees/palette");
          if (signal.aborted) return [];
          const state = commandState.current;
          const rows =
            state.worktreeCache && state.worktreeCache.id === state.toolsContext
              ? state.worktreeCache.rows
              : [];
          return worktreeEntries(rows, query, state.busy, async (row) => {
            const state = commandState.current;
            if (
              !state.toolsContext ||
              state.worktreeCache?.id !== state.toolsContext
            )
              return false;
            const { worktreeApi } = await import("./worktrees/api");
            const result = await worktreeApi.read<{ path: string }>(
              state.toolsContext,
              { kind: "open", worktree: row.id },
            );
            return state.openWorktree(result.path);
          });
        },
      }),
    });
    return () => {
      remove();
      removeSource();
      removeHostingSource();
      removeWorktrees();
    };
  }, [registry]);
  useEffect(() => {
    registry?.refresh();
  }, [
    registry,
    busy,
    review,
    recent,
    hostingResults,
    worktreeCache,
    repository,
    historyOpen,
  ]);
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
    <div
      className={`app ${String(review ? "reviewing" : "")}`}
      aria-busy={busy || closing}
      inert={closing}
    >
      <header className="desktop-chrome">
        <button
          className="brand"
          aria-label={uiText("Recent reviews")}
          onClick={() => registry?.execute("app.home")}
          disabled={busy}
        >
          <BrandMark />
          <strong>{uiText("mdiff")}</strong>
        </button>
        <span className="header-divider" />
        {(review || (historyOpen && repository)) && (
          <span className="repository-name">
            {(historyOpen ? repository?.info.root : review?.root)
              ?.split("/")
              .pop()}
          </span>
        )}
        <span
          className="subtitle"
          title={historyOpen ? repository?.info.root : review?.root}
        >
          {historyOpen
            ? repository?.info.root
            : review
              ? review.root
              : uiText("Code review")}
        </span>
        <div className="window-actions" inert={busy}>
          <button
            aria-label={uiText("Needs attention{value0}", {
              value0: String(
                attentionUnread ? ` (${attentionUnread} unread)` : "",
              ),
            })}
            onClick={() => registry?.execute("app.attention")}
          >
            <Bell size={15} />
            <span className="action-label">
              {uiText("Attention")}
              {attentionUnread ? ` (${attentionUnread})` : ""}
            </span>
          </button>
          <button
            onClick={() => registry?.execute("app.gitOperations")}
            aria-label={uiText("Git operations")}
          >
            <GitCommitHorizontal size={15} />
            <span className="action-label">{uiText("Git")}</span>
          </button>
          <button
            aria-label={uiText("Agents and review")}
            onClick={() => registry?.execute("app.agents")}
          >
            <Bot size={15} />
            <span className="action-label">{uiText("Agents")}</span>
          </button>
          <button
            onClick={() => registry?.execute("app.embeddedTerminal")}
            id="embedded-terminal-toggle"
            aria-label={uiText("Embedded terminal")}
          >
            <Terminal size={15} />
            <span className="action-label">{uiText("Terminal")}</span>
          </button>
          <button
            onClick={() => registry?.execute("app.worktrees")}
            aria-label={uiText("Worktrees and tools")}
          >
            <GitFork size={15} />
            <span className="action-label">{uiText("Worktrees")}</span>
          </button>
          <button
            onClick={() => registry?.execute("app.hosting")}
            aria-label={uiText("Hosting reviews")}
          >
            <GitPullRequest size={15} />
            <span className="action-label">{uiText("Hosting")}</span>
          </button>
          <button
            onClick={() => registry?.execute("app.history")}
            aria-label={uiText("History")}
          >
            <History size={15} />
            <span className="action-label">{uiText("History")}</span>
          </button>
          <button
            onClick={() => registry?.execute("app.palette")}
            aria-label={translate(messages.palette)}
            title={`${translate(messages.palette)} (${shortcutLabel(registry?.shortcut("app.palette"), registry?.platform ?? "other")})`}
          >
            <Search size={15} />
          </button>
          {review && !historyOpen && ReviewChrome && (
            <ReviewChrome
              key={review.id}
              id={review.id}
              repository={review.key}
              root={review.root}
              lspEnabled={review.lsp_enabled ?? false}
              composerOwner={review.tab}
            />
          )}
          {review && (
            <button
              onClick={() => registry?.execute("app.home")}
              title={uiText("Recent reviews")}
              aria-label={uiText("Recent reviews")}
            >
              <ArrowLeft size={15} />
              <span className="action-label">{uiText("Reviews")}</span>
            </button>
          )}
          <button
            onClick={() => registry?.execute("app.preferences")}
            title={uiText("Preferences")}
            aria-label={uiText("Preferences")}
          >
            <Settings size={15} />
            <span className="action-label">{uiText("Preferences")}</span>
          </button>
          <button
            className="open"
            disabled={busy}
            onClick={() => registry?.execute("app.openRepository")}
            title={uiText("Open repository ({value0})", {
              value0: String(
                shortcutLabel(
                  registry?.shortcut("app.openRepository"),
                  registry?.platform ?? "other",
                ),
              ),
            })}
            aria-label={uiText("Open repository")}
          >
            <FolderOpen size={15} />
            <span className="action-label">{uiText("Open repository")}</span>
          </button>
        </div>
      </header>
      {workspaces.document && (
        <Suspense
          fallback={
            <div className="workspace-bar" role="status">
              {"Loading review tabs…"}
            </div>
          }
        >
          <WorkspaceBar
            document={workspaces.document}
            windows={workspaces.windows}
            onWindow={workspaceWindow}
            onFocusWindow={(target) =>
              void invoke("workspace_focus_window", { target }).catch((e) =>
                setError(String(e)),
              )
            }
            active={review?.tab ?? null}
            busy={busy}
            onOperation={workspaceOperation}
            onActivate={(tab) => void activateTab(tab)}
            onNew={() => void home()}
            onRefresh={(tab) => void activateTab(tab, true)}
            onUnload={(tab) => void unloadTab(tab)}
            onDrafts={(tab) => void showDrafts(tab)}
            agentStatus={
              review
                ? {
                    [review.tab]: agentActivity
                      .filter(
                        (a) =>
                          a.context === review.id &&
                          a.state !== "unavailable" &&
                          (a.root === review.root || !!a.terminal),
                      )
                      .map((a) => a.state)
                      .join(", "),
                  }
                : {}
            }
          />
        </Suspense>
      )}
      {closing && (
        <div className="workspace-notice" role="status">
          {uiText("Saving tabs and finishing current operations…")}
        </div>
      )}
      {notice && (
        <div className="workspace-notice" role="status">
          <span>{notice}</span>
          <button
            aria-label={uiText("Dismiss notice")}
            onClick={() => setNotice("")}
          >
            <X size={14} />
          </button>
        </div>
      )}
      {(error || workspaces.error) && (
        <div role="alert" className="error">
          <span>{error || workspaces.error}</span>
          {!workspaces.document && (
            <button
              onClick={() =>
                void workspaces.load().catch((e) => setError(String(e)))
              }
            >
              {uiText("Retry loading workspaces")}
            </button>
          )}
          {error.includes("preferences") && (
            <button
              onClick={() =>
                void shellPreferences.reload
                  .current()
                  .then(() => setError(""))
                  .catch((e) => setError(String(e)))
              }
            >
              {uiText("Reload saved preferences")}
            </button>
          )}
          <button
            aria-label={uiText("Dismiss error")}
            onClick={() => {
              setError("");
              workspaces.setError("");
            }}
          >
            <X size={14} />
          </button>
        </div>
      )}
      {draftViewer && (
        <Dialog
          open
          onOpenChange={(open) => {
            if (!open) setDraftViewer(null);
          }}
        >
          <DialogContent
            className="workspace-dialog"
            aria-labelledby="unsent-title"
          >
            <h2 id="unsent-title">{uiText("Unsent drafts")}</h2>
            <p>
              {uiText(
                "Drafts stay with this tab. Saved review comments are separate.",
              )}
            </p>
            {Object.entries(draftViewer.unsent)
              .filter(([, text]) => text)
              .map(([key, text]) => (
                <label key={key}>
                  <span>{key.split("\0").filter(Boolean).join(" · ")}</span>
                  <textarea readOnly value={text} />
                  <button onClick={() => void writeText(text)}>
                    {uiText("Copy draft")}
                  </button>
                </label>
              ))}
            {!Object.values(draftViewer.unsent).some(Boolean) && (
              <p>{uiText("No unsent drafts in this tab.")}</p>
            )}
            <button onClick={() => setDraftViewer(null)}>
              {uiText("Done")}
            </button>
          </DialogContent>
        </Dialog>
      )}
      {gitPanel && (
        <Suspense
          fallback={<div role="status">{"Loading Git operations…"}</div>}
        >
          <GitOperations
            context={gitPanel.context}
            initialSection={gitPanel.initialSection}
            flushRef={gitFlushRef}
            onClose={() => setGitPanel(null)}
            onChoose={() => openWorktrees("git", true)}
            onTools={async () => {
              if (gitPanel.context) {
                setToolsPanel({ id: gitPanel.context, initial: "worktrees" });
                setGitPanel(null);
              }
            }}
            onOpen={openWorktree}
          />
        </Suspense>
      )}
      {attentionOpen && (
        <Suspense fallback={<div role="status">{"Loading inbox…"}</div>}>
          <AttentionHub
            initialTarget={attentionTarget}
            onClose={() => setAttentionOpen(false)}
          />
        </Suspense>
      )}
      {toolsPanel && (
        <Suspense
          fallback={<div role="status">{"Loading worktrees and tools…"}</div>}
        >
          <WorktreesHub
            key={toolsPanel.id}
            id={toolsPanel.id}
            initial={toolsPanel.initial}
            onClose={() => {
              toolsSequence.current++;
              setToolsPanel(null);
            }}
            onOpen={openWorktree}
            onEmbedded={(worktree) => showTerminal(toolsPanel.id, worktree)}
            agentActivity={agentActivity}
            onInventory={(rows) =>
              setWorktreeCache({ id: toolsPanel.id, rows: rows.slice(0, 256) })
            }
          />
        </Suspense>
      )}
      {hostingOpen && (
        <Suspense
          fallback={<div role="status">{"Loading hosting reviews…"}</div>}
        >
          <HostingHub
            key={review?.id ?? "discovery"}
            id={review?.id}
            onDiscover={(items) => setHostingResults(items.slice(0, 20))}
            onClose={() => setHostingOpen(false)}
            onConnections={() => {
              setHostingOpen(false);
              showConnections();
            }}
            getDraft={(key) =>
              review
                ? (workspaceModule?.getHostingDraft(review.id, key) ?? "")
                : ""
            }
            setDraft={(key, value) => {
              if (review)
                workspaceModule?.setHostingDraft(review.id, key, value);
            }}
            onOpen={async (hosted, title) => {
              const id = await launch(
                "",
                hosted.site.provider === "github"
                  ? {
                      kind: "pr",
                      pr: `https://${hosted.site.host}/${hosted.repository}/pull/${hosted.number}`,
                    }
                  : {
                      kind: "gitlab",
                      url: `https://${hosted.site.host}/${hosted.repository}/-/merge_requests/${hosted.number}`,
                    },
                title,
              );
              if (id) setHostingOpen(false);
              else
                throw Error(
                  "Could not open this review. Close Hosting to inspect the opening error, then retry.",
                );
            }}
          />
        </Suspense>
      )}
      <Environment module={workspaceModule}>
        <div
          className="desktop-content"
          id="workspace-review-panel"
          role="tabpanel"
          hidden={historyOpen}
        >
          {busy && !review ? (
            <main className="loading">
              <LoaderCircle size={28} className="spin" />
              <h2>{uiText("Opening review…")}</h2>
              <p>{uiText("Loading changes.")}</p>
            </main>
          ) : review && ReviewWorkspace ? (
            <div className="review-workspace" inert={busy}>
              <div className="review-content">
                <Suspense
                  fallback={
                    <div className="loading">{"Loading review workspace…"}</div>
                  }
                >
                  <ReviewWorkspace
                    key={review.id}
                    id={review.id}
                    repository={review.key}
                    composerOwner={review.tab}
                    active={!historyOpen && !busy}
                    gitlab={review.gitlab}
                    onDiscussions={toggleDiscussions}
                    githubEnabled={review.github_enabled}
                    onConnections={showConnections}
                    onLspChange={lspChanged}
                  />
                </Suspense>
              </div>
              {review.gitlab && discussions && (
                <GitlabPanel
                  id={review.id}
                  onClose={() => setDiscussions(false)}
                />
              )}
            </div>
          ) : (
            <main className="home">
              <section className="intro">
                <h1>{uiText("Review code")}</h1>
                <p>{uiText("Open a repository or choose a comparison.")}</p>
                <button
                  className="primary"
                  onClick={() => registry?.execute("app.openRepository")}
                >
                  <FolderOpen size={17} />
                  {uiText("Open repository")}
                </button>
                <small>{uiText("Or drop a folder here")}</small>
              </section>
              <div className="home-columns">
                <section className="review-form">
                  <h2>
                    <GitCompareArrows size={18} />
                    {uiText("New review")}
                  </h2>
                  <label>
                    {uiText("Repository")}
                    <div className="folder-field">
                      <input
                        value={path}
                        onChange={(e) => setPath(e.target.value)}
                        placeholder={uiText("Choose a local Git repository")}
                      />
                      <button
                        onClick={selectFolder}
                        aria-label={uiText("Choose repository")}
                      >
                        <FolderOpen size={16} />
                      </button>
                    </div>
                  </label>
                  <label>
                    {uiText("Comparison")}
                    <select
                      value={mode}
                      onChange={(e) => {
                        setMode(e.target.value);
                        setRevisions("");
                      }}
                    >
                      <option value="working">
                        {uiText("All working changes")}
                      </option>
                      <option value="revspec">
                        {uiText("Branch or revision range")}
                      </option>
                      <option value="commit">{uiText("Single commit")}</option>
                      {githubEnabled && (
                        <option value="pr">
                          {uiText("GitHub pull request")}
                        </option>
                      )}
                      <option value="gitlab">
                        {uiText("GitLab merge request")}
                      </option>
                      <option value="range-diff">
                        {uiText("Compare two commit series")}
                      </option>
                    </select>
                  </label>
                  {mode === "pr" || mode === "gitlab" ? (
                    <label>
                      {mode === "gitlab"
                        ? uiText("Merge request URL")
                        : uiText("Pull request URL or number")}
                      <input
                        value={pr}
                        onChange={(e) => setPr(e.target.value)}
                        placeholder={
                          mode === "gitlab"
                            ? uiText(
                                "https://gitlab.com/group/project/-/merge_requests/123",
                              )
                            : uiText("https://github.com/owner/repo/pull/123")
                        }
                      />
                      <small>
                        {mode === "gitlab"
                          ? uiText("GitLab.com or self-managed.")
                          : uiText("Repository is optional for a URL.")}
                      </small>
                    </label>
                  ) : (
                    mode !== "working" && (
                      <label>
                        {mode === "range-diff"
                          ? uiText("Commit ranges")
                          : mode === "commit"
                            ? uiText("Commit")
                            : uiText("Branch or revisions")}
                        <input
                          value={revisions}
                          onChange={(e) => setRevisions(e.target.value)}
                          placeholder={
                            mode === "range-diff"
                              ? uiText("main..v1 main..v2")
                              : mode === "commit"
                                ? uiText("HEAD")
                                : uiText(
                                    "main, main..feature, or main..worktree",
                                  )
                          }
                        />
                        <small>
                          {mode === "revspec"
                            ? uiText(
                                "A single branch compares changes since its merge base.",
                              )
                            : uiText("Use Git revision names.")}
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
                    {uiText("Open review")}
                  </button>
                </section>
                <section className="recent">
                  <h2>
                    <History size={18} />
                    {uiText("Recent reviews")}
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
                            {r.path.split("/").pop() || uiText("Pull request")}
                          </strong>
                          <small>{r.label}</small>
                          <small className="recent-path">{r.path}</small>
                        </span>
                      </button>
                    ))
                  ) : (
                    <p>{uiText("No recent reviews.")}</p>
                  )}
                </section>
              </div>
            </main>
          )}
        </div>
      </Environment>
      {repository && (
        <div className="desktop-history" hidden={!historyOpen}>
          <Suspense fallback={<p>{"Loading history…"}</p>}>
            <HistoryBrowser
              key={repository.id}
              onAgentReview={async (request, root) => {
                const id = await launch(
                  root,
                  request,
                  "Agent review comparison",
                );
                if (!id) throw Error("Could not open comparison");
                setHistoryOpen(false);
                showAgents();
              }}
              agentCommits={Object.fromEntries(
                agentActivity
                  .filter((a) => a.commit)
                  .map((a) => [a.commit!, `${a.tool}: ${a.state}`]),
              )}
              initialInfo={repository.info}
              transport={repository.transport}
              active={historyOpen}
              onChooseRepository={() => void openHistory(true)}
              onClose={() => setHistoryOpen(false)}
              onReview={async (request, root) => {
                const id = await launch(root, request, "History comparison");
                if (!id)
                  throw new Error(
                    "Could not open comparison. Check the error above.",
                  );
              }}
            />
          </Suspense>
        </div>
      )}
      {agentLoaded && (
        <Suspense fallback={null}>
          <AgentPanel
            draft={
              review
                ? workspaceModule?.getHostingDraft(review.id, "agents:findings")
                : ""
            }
            onDraft={(value) => {
              if (review)
                workspaceModule?.setHostingDraft(
                  review.id,
                  "agents:findings",
                  value,
                );
            }}
            open={agentOpen}
            context={review?.id ?? null}
            onClose={() => setAgentOpen(false)}
            onActivity={setAgentActivity}
            onExternal={() => openWorktrees("terminal")}
            onLaunch={async (preview) => {
              const module = terminalModule ?? (await import("./terminal"));
              await module.terminalStore.startAgent(preview);
              setTerminalModule(module);
              setTerminalVisible(true);
            }}
            onTerminal={async (id) => {
              const module = terminalModule ?? (await import("./terminal"));
              await module.terminalStore.init();
              if (
                !module.terminalStore.sessions().some((s) => s.info?.id === id)
              )
                throw Error(
                  "This terminal belongs to another or closed document.",
                );
              module.terminalStore.select(id);
              setTerminalModule(module);
              setTerminalVisible(true);
              setAgentOpen(false);
              requestAnimationFrame(() =>
                module.terminalStore
                  .sessions()
                  .find((s) => s.info?.id === id)
                  ?.term.focus(),
              );
            }}
          />
        </Suspense>
      )}
      {terminalModule && (
        <terminalModule.TerminalPanel
          hidden={!terminalVisible}
          context={
            historyOpen
              ? (repository?.id ?? null)
              : (review?.id ?? repository?.id ?? null)
          }
          onHide={() => {
            setTerminalVisible(false);
            document.getElementById("embedded-terminal-toggle")?.focus();
          }}
          onExternal={() => openWorktrees("terminal")}
        />
      )}
      {busy && review && (
        <div
          className="loading review-opening"
          role="status"
          aria-live="polite"
        >
          <LoaderCircle size={28} className="spin" />
          <h2>{uiText("Opening review…")}</h2>
          <p>{uiText("Loading changes.")}</p>
        </div>
      )}
      {settings && (
        <Dialog open onOpenChange={() => setSettings(false)}>
          <DialogContent className="settings" aria-labelledby="settings-title">
            <button
              className="delete"
              aria-label={uiText("Close settings")}
              onClick={() => setSettings(false)}
            >
              <X size={18} />
            </button>
            <h2 id="settings-title">{uiText("Preferences")}</h2>
            <nav
              className="preferences-tabs"
              aria-label={uiText("Preference sections")}
            >
              {[
                "appearance",
                "shortcuts",
                "connections",
                "tools",
                "updates",
                "about",
              ].map((tab) => (
                <button
                  key={tab}
                  aria-pressed={settingsTab === tab}
                  onClick={() => setSettingsTab(tab)}
                >
                  {tab === "appearance"
                    ? uiText("Appearance")
                    : tab === "shortcuts"
                      ? uiText("Shortcuts")
                      : tab === "connections"
                        ? uiText("Connections")
                        : tab === "tools"
                          ? uiText("Code intelligence")
                          : tab === "updates"
                            ? uiText("Updates")
                            : uiText("About")}
                </button>
              ))}
            </nav>
            {settingsTab === "shortcuts" && (
              <Suspense fallback={null}>
                <ShortcutSettings />
              </Suspense>
            )}
            {settingsTab === "updates" && <Updates />}
            {settingsTab === "about" && <About />}
            {settingsTab === "appearance" && (
              <AppearancePanel showHeading={false} />
            )}
            {settingsTab === "connections" && (
              <>
                <p>
                  {uiText(
                    "Connection settings are shared by all workspace windows. Tokens stay in memory until you quit. Save to apply changes, then reopen affected reviews.",
                  )}
                </p>
                <h3>{uiText("GitHub connection")}</h3>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={githubDraft}
                    disabled={settingsLoading || settingsSaving}
                    onChange={(e) => {
                      setGithubDraft(e.target.checked);
                      setSettingsSaved(false);
                    }}
                  />
                  {uiText("Enable GitHub integration")}
                </label>
                <p>
                  {uiText(
                    "Save to apply. Turning this off stops GitHub lookups, PR actions, and comment transfers immediately. Local reviews remain available. Reopen your review after enabling or changing credentials.",
                  )}
                </p>
                <fieldset
                  disabled={!githubDraft || settingsLoading || settingsSaving}
                >
                  <p>
                    {uiText("Use your GitHub CLI sign-in or enter a token.")}
                  </p>
                  <label>
                    {uiText("GitHub host")}
                    <input
                      value={tokenHost}
                      onChange={(e) => setTokenHost(e.target.value.trim())}
                      placeholder={uiText("github.com")}
                    />
                  </label>
                  <label>
                    {uiText("Personal access token")}
                    <input
                      type="password"
                      autoComplete="off"
                      spellCheck={false}
                      value={token}
                      onChange={(e) => setToken(e.target.value)}
                      placeholder={uiText(
                        "Optional when already signed in with GitHub CLI",
                      )}
                    />
                  </label>
                  <button onClick={() => setToken("")} disabled={!token}>
                    {uiText("Clear session token")}
                  </button>
                </fieldset>
                <h3>{uiText("GitLab connection")}</h3>
                <p>
                  {uiText(
                    "GitLab.com or self-managed. Token scopes: api, read_repository.",
                  )}
                </p>
                <label>
                  {uiText("GitLab host (include port when needed)")}
                  <input
                    value={gitlabHost}
                    onChange={(e) => setGitlabHost(e.target.value.trim())}
                    placeholder={uiText("gitlab.com")}
                  />
                </label>
                <label>
                  {uiText("GitLab personal access token")}
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
                  {uiText("Clear GitLab token")}
                </button>
              </>
            )}
            {settingsTab === "tools" && (
              <>
                <h3>{uiText("Language servers")}</h3>
                <p>
                  {uiText(
                    "Enable code intelligence from the review toolbar after trusting the repository. Each new review starts with it off.",
                  )}
                </p>
                <p>
                  {uiText(
                    "Detected from PATH. Set commands by language; an empty value disables it.",
                  )}
                </p>
                <label>
                  {uiText("Language commands (JSON)")}
                  <textarea
                    spellCheck={false}
                    value={commands}
                    onChange={(e) => {
                      setCommands(e.target.value);
                      setSettingsSaved(false);
                    }}
                    placeholder={uiText(
                      '{"rust":"rust-analyzer","python":"pyright-langserver --stdio"}',
                    )}
                  />
                </label>
              </>
            )}
            {settingsTab === "connections" && (
              <details>
                <summary>{uiText("Enterprise host mapping")}</summary>
                <label>
                  {uiText("Git remote host → GitHub web host (JSON)")}
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
            {(settingsTab === "connections" || settingsTab === "tools") && (
              <div className="settings-footer">
                <span role="status">
                  {settingsSaved
                    ? uiText(
                        "Saved. GitHub disable applies now; reopen to apply connection changes.",
                      )
                    : ""}
                </span>
                <button
                  className="primary"
                  disabled={!settingsReady || settingsLoading || settingsSaving}
                  onClick={saveSettings}
                >
                  {settingsSaving
                    ? uiText("Saving…")
                    : settingsLoading
                      ? uiText("Loading…")
                      : uiText("Save settings")}
                </button>
              </div>
            )}
          </DialogContent>
        </Dialog>
      )}
    </div>
  );
}
createRoot(document.getElementById("root")!).render(
  <CommandProvider>
    <App />
  </CommandProvider>,
);
