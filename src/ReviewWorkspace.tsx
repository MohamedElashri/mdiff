import { uiText } from "../vendor/diffle/src/client/i18n/runtime";
import { CodeIntelligence } from "./CodeIntelligence";
import { OpenInEditor } from "./OpenInEditor";
import { LegacyDraftRecovery } from "../vendor/diffle/src/client/LegacyDraftRecovery";
import { ReviewScopePool } from "../vendor/diffle/src/client/scopePool";
import { ReviewScopeProvider } from "../vendor/diffle/src/client/runtime";
import { nativeReviewTransport } from "./transport";
import { useCommandAction } from "../vendor/diffle/src/client/commands/CommandProvider";
import { useAppearance } from "../vendor/diffle/src/client/appearance/preferences";
import { useEffect, useRef, type ReactNode } from "react";
import { GitPullRequest } from "lucide-react";
import { GithubPanel } from "./GithubPanel";
import { useStore, useReviewStore } from "../vendor/diffle/src/client/store";
import DiffsWorker from "@pierre/diffs/worker/worker.js?worker";
import { WorkerPoolContextProvider, useWorkerPool } from "@pierre/diffs/react";
import { App } from "../vendor/diffle/src/client/App";
import { HIGHLIGHTER, CODE_THEMES } from "../vendor/diffle/src/client/theme";
const poolOptions = {
  poolSize: Math.max(1, Math.min(2, (navigator.hardwareConcurrency || 4) - 1)),
  workerFactory: () => new DiffsWorker(),
};
const highlighterOptions = {
  preferredHighlighter: HIGHLIGHTER,
  useTokenTransformer: true,
  theme: CODE_THEMES,
};
// Two admitted WebViews share the existing 96 MiB aggregate / four-worker ceiling.
const scopes = new ReviewScopePool(48 * 1024 * 1024, 1);
const adoptions = new Map<string, { key: string; owner: string }>();
function composerOwner(repository: string) {
  const key = `mdiff:composer-owner:${repository}`;
  try {
    const old = sessionStorage.getItem(key);
    if (old) return old;
    const owner = crypto.randomUUID();
    sessionStorage.setItem(key, owner);
    return owner;
  } catch {
    return repository;
  }
}
export function initializeWorkspace(
  id: string,
  repository: string,
  owner: string,
  view: {
    revision?: number;
    unsent: Record<string, string>;
    navigation: unknown;
  },
) {
  if (scopes.peek(id)) return;
  const scope = reviewScope(id, repository, owner, view.navigation);
  let legacy: Record<string, string> = {};
  try {
    const logical = repository.replace(/^workspace\/[^/]+\//, "");
    const previous = sessionStorage.getItem(`mdiff:composer-owner:${logical}`);
    if (previous) {
      const key = `diffle:unsent:v2:${JSON.stringify([logical, previous])}`;
      if (!view.revision && !sessionStorage.getItem(`mdiff:adopted:${key}`)) {
        legacy = JSON.parse(sessionStorage.getItem(key) ?? "{}");
        if (!legacy || typeof legacy !== "object" || Array.isArray(legacy))
          legacy = {};
        legacy = Object.fromEntries(
          Object.entries(legacy).filter(([, v]) => typeof v === "string"),
        );
        // Originals remain available; only one tab adopts a Phase 4 composer owner.
        adoptions.set(id, { key: `mdiff:adopted:${key}`, owner });
      }
    }
  } catch {
    /* Existing copies stay intact if storage is unavailable. */
  }
  scope.store.setState({
    unsent: { ...legacy, ...view.unsent },
  });
}
export function acknowledgeWorkspaceSaved(id: string) {
  const adoption = adoptions.get(id);
  if (adoption) {
    try {
      sessionStorage.setItem(adoption.key, adoption.owner);
      adoptions.delete(id);
    } catch {
      /* The original copy remains recoverable. */
    }
  }
}
export function clearWorkspaceDrafts(id: string) {
  const scope = scopes.peek(id);
  if (scope) {
    for (const key of Object.keys(scope.store.getState().unsent))
      scope.store.getState().setUnsent(key, "");
  }
}
export function getHostingDraft(id: string, key: string) {
  return scopes.peek(id)?.store.getState().unsent[key] ?? "";
}
export function setHostingDraft(id: string, key: string, value: string) {
  scopes.peek(id)?.store.getState().setUnsent(key, value);
}
export function captureWorkspace(id: string) {
  return scopes.capture(id);
}
export function subscribeWorkspace(id: string, changed: () => void) {
  const scope = scopes.peek(id);
  if (!scope) return () => {};
  const stop = scope.store.subscribe((s, p) => {
    if (
      s.unsent !== p.unsent ||
      s.activePath !== p.activePath ||
      s.selection !== p.selection ||
      s.draft !== p.draft ||
      s.paneScroll !== p.paneScroll ||
      s.collapsed !== p.collapsed ||
      s.jumps !== p.jumps ||
      s.jumpIndex !== p.jumpIndex ||
      s.snapshot?.oldSha !== p.snapshot?.oldSha ||
      s.snapshot?.newSha !== p.snapshot?.newSha ||
      s.snapshot?.review !== p.snapshot?.review
    )
      changed();
  });
  const views = scope.views.store.subscribe(() => changed());
  const graph = scope.graph.store.subscribe((s, p) => {
    if (
      s.viewport !== p.viewport ||
      s.selected !== p.selected ||
      s.selectedEdge !== p.selectedEdge ||
      s.filters !== p.filters
    )
      changed();
  });
  return () => {
    stop();
    views();
    graph();
  };
}
export function closeWorkspace(id: string) {
  scopes.close(id);
}
function reviewScope(
  id: string,
  repository: string,
  owner?: string,
  navigation?: unknown,
) {
  return scopes.acquire(
    id,
    () => nativeReviewTransport(id),
    {
      repository,
      composerOwner: owner ?? composerOwner(repository),
    },
    navigation,
  );
}
export function ReviewChrome({
  id,
  repository,
  root,
  lspEnabled,
  composerOwner: owner,
}: {
  id: string;
  repository: string;
  root: string;
  lspEnabled: boolean;
  composerOwner?: string;
}) {
  const scope = reviewScope(id, repository, owner);
  return (
    <ReviewScopeProvider scope={scope} attachDom={false}>
      <CodeIntelligence id={id} root={root} initialEnabled={lspEnabled} />
      <OpenInEditor id={id} />
    </ReviewScopeProvider>
  );
}
export function ReviewEnvironment({ children }: { children: ReactNode }) {
  return (
    <WorkerPoolContextProvider
      poolOptions={poolOptions}
      highlighterOptions={highlighterOptions}
    >
      <SyncAppearance />
      {children}
    </WorkerPoolContextProvider>
  );
}
export function ReviewWorkspace({
  id,
  repository,
  active = true,
  composerOwner: owner,
  ...props
}: {
  id: string;
  repository: string;
  active?: boolean;
  composerOwner?: string;
  gitlab: boolean;
  onDiscussions: () => void;
  githubEnabled: boolean;
  onConnections: () => void;
  onLspChange: (enabled: boolean) => void;
}) {
  const scope = reviewScope(id, repository, owner);
  useEffect(() => () => scope.suspend(), [scope]);
  return (
    <ReviewScopeProvider scope={scope} active={active}>
      <ReviewBody {...props} />
    </ReviewScopeProvider>
  );
}
function ReviewBody({
  gitlab,
  onDiscussions,
  githubEnabled,
  onConnections,
  onLspChange,
}: {
  gitlab: boolean;
  onDiscussions: () => void;
  githubEnabled: boolean;
  onConnections: () => void;
  onLspChange: (enabled: boolean) => void;
}) {
  const useStore = useReviewStore();
  const lspEnabled = useStore((s) => s.lsp.enabled);
  useEffect(() => onLspChange(lspEnabled), [lspEnabled, onLspChange]);
  const githubOpen = useStore((s) => s.githubMenuOpen);
  const setGithubOpen = useStore((s) => s.setGithubMenuOpen);
  const githubCommand = useCommandAction("review.github", () =>
    setGithubOpen(!githubOpen),
  );
  const disabledGithubStatus = useStore((s) =>
    githubEnabled ? "idle" : s.github.status,
  );
  const githubTrigger = useRef<HTMLButtonElement>(null);
  const closeGithub = () => {
    setGithubOpen(false);
    githubTrigger.current?.focus();
  };
  useEffect(() => {
    if (!githubEnabled) {
      setGithubOpen(false);
      if (disabledGithubStatus !== "idle")
        useStore.setState({ github: { status: "idle" } });
    }
  }, [githubEnabled, disabledGithubStatus, setGithubOpen]);
  return (
    <>
      <div className="review-workspace desktop-review-runtime">
        <div className="review-content review-body-content">
          <LegacyDraftRecovery />
          <App
            desktop
            githubAvailable={!gitlab && githubEnabled}
            hostingControl={
              gitlab ? (
                <button className="gitlab-control" onClick={onDiscussions}>
                  {"GitLab discussions"}
                </button>
              ) : githubEnabled ? (
                <button
                  ref={githubTrigger}
                  className="gitlab-control"
                  aria-expanded={githubOpen}
                  aria-controls="desktop-github-panel"
                  title={"GitHub review (o)"}
                  onClick={githubCommand.run}
                >
                  <GitPullRequest size={15} />
                  {" GitHub"}
                </button>
              ) : (
                false
              )
            }
          />
        </div>
        {githubEnabled && githubOpen && (
          <GithubPanel onClose={closeGithub} onConnections={onConnections} />
        )}
      </div>
    </>
  );
}

function SyncAppearance() {
  const appearance = useAppearance();
  const pool = useWorkerPool();
  useEffect(() => {
    void pool?.setRenderOptions({ theme: CODE_THEMES });
  }, [pool, appearance.light, appearance.dark]);
  return null;
}
