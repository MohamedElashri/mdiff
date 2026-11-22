import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import type { Document, Operation, Source, WorkspaceWindow } from "./types";
export function useWorkspaces(legacy: () => Source[]) {
  const [document, setDocument] = useState<Document | null>(null);
  const [error, setError] = useState("");
  const [windows, setWindows] = useState<WorkspaceWindow[]>([]);
  const current = useRef<Document | null>(null);
  const queue = useRef(Promise.resolve());
  const legacyRef = useRef(legacy);
  legacyRef.current = legacy;
  const accept = useCallback((next: Document) => {
    if (!next || next.version !== 1 || !Array.isArray(next.workspaces))
      throw Error("Invalid workspace response");
    if (!current.current || next.revision >= current.current.revision) {
      current.current = next;
      setDocument(next);
    }
    return next;
  }, []);
  const load = useCallback(
    async () =>
      accept(
        await invoke<Document>("workspace_load", {
          legacy: legacyRef.current(),
        }),
      ),
    [accept],
  );
  useEffect(() => {
    if (!isTauri()) return;
    let live = true;
    let stop: (() => void) | undefined;
    void load().catch((e) => live && setError(String(e)));
    void listen("workspace-state", () => {
      void load().catch((e) => live && setError(String(e)));
    })
      .then((unlisten) => {
        if (live) stop = unlisten;
        else unlisten();
      })
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
      stop?.();
    };
  }, [load]);
  useEffect(() => {
    if (!isTauri()) return;
    let live = true;
    let stop: (() => void) | undefined;
    const refresh = () => {
      void invoke<WorkspaceWindow[]>("workspace_windows")
        .then((value) => {
          if (live) setWindows(value ?? []);
        })
        .catch((e) => live && setError(String(e)));
    };
    refresh();
    void listen("workspace-windows", () => {
      refresh();
      void load().catch((e) => live && setError(String(e)));
    }).then((unlisten) => {
      if (live) stop = unlisten;
      else unlisten();
    });
    return () => {
      live = false;
      stop?.();
    };
  }, [load]);
  const update = useCallback(
    (operation: Operation) => {
      const run = queue.current.then(async () => {
        if (!current.current)
          throw Error("Workspaces are still loading. Try again.");
        try {
          return accept(
            await invoke<Document>("workspace_update", {
              revision: current.current.revision,
              operation,
            }),
          );
        } catch (error) {
          await load().catch(() => {});
          throw error;
        }
      });
      queue.current = run.then(
        () => {},
        () => {},
      );
      return run;
    },
    [accept, load],
  );
  return { document, current, windows, error, setError, load, update, accept };
}
