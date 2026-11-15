import { useAppearance } from "../vendor/diffle/src/client/appearance/preferences";
import { useEffect, useRef } from "react";
import { GitPullRequest } from "lucide-react";
import { GithubPanel } from "./GithubPanel";
import { useStore } from "../vendor/diffle/src/client/store";
import DiffsWorker from "@pierre/diffs/worker/worker.js?worker";
import { WorkerPoolContextProvider, useWorkerPool } from "@pierre/diffs/react";
import { App } from "../vendor/diffle/src/client/App";
import {
  applyTheme,
  readTheme,
  HIGHLIGHTER,
  CODE_THEMES,
} from "../vendor/diffle/src/client/theme";
applyTheme(readTheme());
const poolOptions = {
  poolSize: Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1)),
  workerFactory: () => new DiffsWorker(),
};
const highlighterOptions = {
  preferredHighlighter: HIGHLIGHTER,
  useTokenTransformer: true,
  theme: CODE_THEMES,
};
export function ReviewWorkspace({
  gitlab,
  onDiscussions,
  githubEnabled,
  onConnections,
}: {
  gitlab: boolean;
  onDiscussions: () => void;
  githubEnabled: boolean;
  onConnections: () => void;
}) {
  const githubOpen = useStore((s) => s.githubMenuOpen);
  const setGithubOpen = useStore((s) => s.setGithubMenuOpen);
  const githubState = useStore((s) => s.github);
  const githubTrigger = useRef<HTMLButtonElement>(null);
  const closeGithub = () => {
    setGithubOpen(false);
    githubTrigger.current?.focus();
  };
  useEffect(() => () => useStore.getState().endReview(), []);
  useEffect(() => {
    if (!githubEnabled) {
      setGithubOpen(false);
      if (githubState.status !== "idle")
        useStore.setState({ github: { status: "idle" } });
    }
  }, [githubEnabled, githubState, setGithubOpen]);
  return (
    <WorkerPoolContextProvider
      poolOptions={poolOptions}
      highlighterOptions={highlighterOptions}
    >
      <SyncAppearance />
      <div className="review-workspace">
        <div className="review-content">
          <App
            desktop
            hostingControl={
              gitlab ? (
                <button className="gitlab-control" onClick={onDiscussions}>
                  GitLab discussions
                </button>
              ) : githubEnabled ? (
                <button
                  ref={githubTrigger}
                  className="gitlab-control"
                  aria-expanded={githubOpen}
                  aria-controls="desktop-github-panel"
                  title="GitHub review (o)"
                  onClick={() => setGithubOpen(!githubOpen)}
                >
                  <GitPullRequest size={15} /> GitHub
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
    </WorkerPoolContextProvider>
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
