import { invoke } from "@tauri-apps/api/core";
export type RecoveryCommand = "continue" | "skip" | "abort";
export type RebaseItem = {
  oid: string;
  command: "pick" | "drop" | "fixup" | "squash";
};
export type Recovery = {
  operation: string | null;
  commands: RecoveryCommand[];
  conflicts: {
    path: string;
    stages: { stage: number; oid: string; mode: string }[];
  }[];
  currentCommit: string | null;
  onto: string | null;
  branch: string | null;
  explanation: string;
};
export type Conflict = {
  path: string;
  draftKey: string;
  expected: string;
  supported: boolean;
  reason: string;
  labels: string[];
  contents: string[];
  result: string;
};
export type RebaseCommit = { oid: string; subject: string };
export type Action =
  | { kind: "merge"; revision: string }
  | { kind: "cherryPick" | "revert"; revision: string; mainline: number | null }
  | { kind: "rebase"; upstream: string }
  | { kind: "rebaseInteractive"; upstream: string; todo: RebaseItem[] }
  | { kind: "recover"; operation: string; command: RecoveryCommand }
  | { kind: "resolveText"; path: string; content: string; expected: string }
  | { kind: "init"; path: string; branch: string }
  | { kind: "clone"; path: string; url: string }
  | { kind: "branchCreate" | "tagCreate"; name: string; start: string }
  | {
      kind: "branchSwitch" | "branchDelete" | "tagDelete" | "remoteRemove";
      name: string;
    }
  | { kind: "remoteAdd"; name: string; url: string }
  | {
      kind:
        | "stage"
        | "unstage"
        | "restore"
        | "discard"
        | "worktreeUnlock"
        | "worktreeRemove";
      path: string;
    }
  | { kind: "hunk"; path: string; staged: boolean; hunk: number }
  | { kind: "commit"; message: string; amend: boolean }
  | { kind: "fetch"; remote: string }
  | { kind: "pull"; remote: string; branch: string }
  | { kind: "push"; remote: string; branch: string; force: boolean }
  | { kind: "worktreeCreate"; path: string; branch: string }
  | { kind: "worktreeLock"; path: string; reason: string }
  | { kind: "worktreePrune" }
  | { kind: "stashSave"; message: string; untracked: boolean }
  | { kind: "stashApply" | "stashPop" | "stashDrop"; oid: string };
export type Snapshot = {
  identity: { root: string; git: string; common: string };
  head: string | null;
  branch: string | null;
  operations: string[];
  conflicts: boolean;
};
export type Change = {
  path: string;
  index: string;
  worktree: string;
  untracked: boolean;
  oldPath: string | null;
};
export type Patch = { text: string; hunks: string[]; partial: boolean };
export type Catalog = {
  state: Snapshot;
  files: Change[];
  branches: string[];
  tags: string[];
  remotes: string[];
  stashes: { oid: string; label: string }[];
  recovery?: Recovery;
  locks: string[];
  worktrees: {
    path: string;
    branch: string | null;
    locked: string | null;
    prunable: string | null;
    bare: boolean;
    canModify: boolean;
  }[];
};
export type Preview = {
  root?: string;
  id: string;
  label: string;
  text: string;
  before?: Snapshot;
  patch?: Patch | null;
};
export type Receipt = {
  targetRoot?: string | null;
  intent?: {
    path?: string;
    name?: string;
    branch?: string;
    remote?: string;
    oid?: string;
  };
  operation: string;
  root: string;
  label: string;
  status: string;
  message: string;
  before: Snapshot | null;
  after: Snapshot | null;
};
export type Job = { receipt: Receipt; active: boolean; canCancel: boolean };
export type Draft = { revision: number; message: string };
function read<T>(command: string, fields: object, signal?: AbortSignal) {
  if (signal?.aborted)
    return Promise.reject(new DOMException("Cancelled", "AbortError"));
  const requestId = crypto.randomUUID();
  const cancel = () => {
    void invoke("git_operation_cancel_read", { requestId }).catch(() => {});
  };
  signal?.addEventListener("abort", cancel, { once: true });
  return invoke<T>(command, { ...fields, requestId }).finally(() =>
    signal?.removeEventListener("abort", cancel),
  );
}
export const gitApi = {
  conflict: (context: string, path: string, signal?: AbortSignal) =>
    read<Conflict>(
      "git_operation_read",
      { context, query: { kind: "conflict", path } },
      signal,
    ),
  rebaseCommits: (context: string, upstream: string, signal?: AbortSignal) =>
    read<RebaseCommit[]>(
      "git_operation_read",
      { context, query: { kind: "rebaseCommits", upstream } },
      signal,
    ),
  upstream: (context: string, signal?: AbortSignal) =>
    read<{
      upstream: string | null;
      ahead: number | null;
      behind: number | null;
    }>("git_operation_read", { context, query: { kind: "upstream" } }, signal),
  catalog: (context: string, signal?: AbortSignal) =>
    read<Catalog>(
      "git_operation_read",
      { context, query: { kind: "catalog" } },
      signal,
    ),
  patch: (
    context: string,
    path: string,
    staged: boolean,
    signal?: AbortSignal,
  ) =>
    read<Patch>(
      "git_operation_read",
      { context, query: { kind: "patch", path, staged } },
      signal,
    ),
  preview: (context: string | null, action: Action, signal?: AbortSignal) =>
    read<Preview>("git_operation_preview", { context, action }, signal),
  forget: (preview: string) =>
    invoke<void>("git_operation_forget_preview", { preview }),
  execute: (preview: string, context: string | null) =>
    invoke<Receipt>("git_operation_execute", { preview, context }),
  jobs: () => invoke<Job[]>("git_operation_receipts"),
  cancel: (operation: string) =>
    invoke<void>("git_operation_cancel", { operation }),
  inspect: (operation: string) =>
    invoke<{ receipt: Receipt; current: Catalog; remoteHead: string | null }>(
      "git_operation_inspect",
      { operation },
    ),
  draft: (context: string, purpose?: string) =>
    invoke<Draft>("git_operation_draft", {
      context,
      purpose: purpose ?? null,
      revision: null,
      message: null,
    }),
  saveDraft: (
    context: string,
    revision: number,
    message: string,
    purpose?: string,
  ) =>
    invoke<Draft>("git_operation_draft", {
      context,
      revision,
      message,
      purpose: purpose ?? null,
    }),
};
