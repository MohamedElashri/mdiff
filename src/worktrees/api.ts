import { invoke } from "@tauri-apps/api/core";
export type Association = {
  path: string;
  tab: string;
  label: string;
  workspace: string;
  workspaceName: string;
};
export type Worktree = {
  id: string;
  path: string;
  head: string | null;
  branch: string | null;
  detached: boolean;
  bare: boolean;
  locked: string | null;
  prunable: string | null;
  current: boolean;
  canOpen: boolean;
  canInspect: boolean;
  reason: string | null;
  reviews: Association[];
};
export type Inventory = { root: string; managed: boolean; items: Worktree[] };
export type Status = {
  head: string | null;
  branch: string | null;
  upstream: string | null;
  ahead: number | null;
  behind: number | null;
  staged: number;
  unstaged: number;
  untracked: number;
  conflicts: number;
  shallow: boolean;
};
export type Kind = "editor" | "terminal";
export type Tool = { id: string; name: string };
export type Preferences = { revision: number; terminal: string | null };
export type Tools = {
  root: string;
  temporary: boolean;
  editors: Tool[];
  terminals: Tool[];
  preferences: Preferences;
};
export const worktreeApi = {
  read: <T>(id: string, query: object, signal?: AbortSignal) => {
    if (signal?.aborted)
      return Promise.reject(
        new DOMException("Request cancelled", "AbortError"),
      );
    const requestId = crypto.randomUUID();
    const cancel = () => {
      void invoke("worktree_cancel", { requestId }).catch(() => {});
    };
    signal?.addEventListener("abort", cancel, { once: true });
    return invoke<T>("worktree_read", { id, query, requestId }).finally(() =>
      signal?.removeEventListener("abort", cancel),
    );
  },
  tools: (id: string, worktree: string | null) =>
    invoke<Tools>("external_tools", { id, worktree }),
  launch: (id: string, worktree: string | null, kind: Kind, toolId: string) =>
    invoke<void>("open_external_tool", { id, worktree, kind, toolId }),
  save: (revision: number, terminal: string | null) =>
    invoke<Preferences>("save_external_preferences", { revision, terminal }),
};
