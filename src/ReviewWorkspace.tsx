import { useAppearance } from "../vendor/diffle/src/client/appearance/preferences";
import { useEffect } from "react";
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
}: {
  gitlab: boolean;
  onDiscussions: () => void;
}) {
  useEffect(() => () => useStore.getState().endReview(), []);
  return (
    <WorkerPoolContextProvider
      poolOptions={poolOptions}
      highlighterOptions={highlighterOptions}
    >
      <SyncAppearance />
      <App
        desktop
        hostingControl={
          gitlab ? (
            <button className="gitlab-control" onClick={onDiscussions}>
              GitLab discussions
            </button>
          ) : undefined
        }
      />
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
