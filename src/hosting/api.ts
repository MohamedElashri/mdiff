import { invoke } from "@tauri-apps/api/core";
export type Site = { provider: "github" | "gitlab"; host: string };
export type ReviewRef = { site: Site; repository: string; number: number };
export type Summary = {
  review: ReviewRef;
  title: string;
  body: string;
  author: string;
  state: string;
  head: string | null;
  url: string;
};
export type Details = Summary & {
  base: string;
  mergeRevision: string | null;
  merged: boolean;
  draft: boolean;
  mergeStatus: string | null;
  capabilities: {
    comment: boolean;
    reply: boolean;
    resolve: boolean;
    review: string[];
    mergeMethods: string[];
  };
};
export type Page<T> = {
  items: T[];
  next: number | null;
  page: number;
  cursor?: string | null;
  warnings?: string[];
  incomplete?: boolean;
};
export type Check = {
  id: string;
  kind: string;
  name: string;
  status: string;
  sha: string;
  url: string | null;
  run: number | null;
  attempt: number | null;
};
export type Note = {
  id: string | number;
  body: string;
  author: string;
  url: string | null;
  sha?: string;
};
export type Thread = {
  id: string;
  resolved: boolean;
  canResolve: boolean;
  outdated: boolean | null;
  path: string | null;
  line: number | null;
  truncated: boolean;
  originalLine?: number | null;
  positionSha?: string | null;
  oldPath?: string | null;
  newPath?: string | null;
  oldLine?: number | null;
  newLine?: number | null;
  side?: string | null;
  comments: Note[];
};
export type Job = {
  id: number;
  name: string;
  status: string;
  url: string | null;
  steps: { name: string; status: string }[];
};
export type Action =
  | { kind: "comment"; body: string }
  | { kind: "submit"; verdict: string; body: string }
  | { kind: "reply"; thread: string; comment: string | null; body: string }
  | { kind: "resolve"; thread: string; resolved: boolean }
  | { kind: "merge"; method: string };
export type Write = {
  operation: string;
  review: ReviewRef;
  expectedHead: string;
  action: Action;
};
export type Receipt = {
  operation: string;
  status: "pending" | "unknown" | "succeeded" | "rejected";
  reviewedSha: string;
  url: string;
  message: string;
  remoteId: string | null;
  resultSha?: string | null;
};
export type Context = {
  sites: Site[];
  current: { review: ReviewRef; base: string; sha: string } | null;
  epoch: number;
};
export const hosting = {
  context: (id?: string) =>
    invoke<Context>("hosting_context", { id: id ?? null }),
  read: <T>(site: Site, request: object, signal?: AbortSignal) => {
    if (signal?.aborted)
      return Promise.reject(
        new DOMException("Request cancelled", "AbortError"),
      );
    const requestId = crypto.randomUUID();
    const cancel = () => {
      void invoke("hosting_cancel", { requestId }).catch(() => {});
    };
    signal?.addEventListener("abort", cancel, { once: true });
    return invoke<T>("hosting_read", { site, request, requestId }).finally(() =>
      signal?.removeEventListener("abort", cancel),
    );
  },
  write: (id: string, request: Write) =>
    invoke<Receipt>("hosting_write", { id, request }),
};
export function sameReview(a: ReviewRef | undefined, b: ReviewRef | undefined) {
  return (
    !!a &&
    !!b &&
    a.site.provider === b.site.provider &&
    a.site.host === b.site.host &&
    a.repository === b.repository &&
    a.number === b.number
  );
}
export function draftKey(review: ReviewRef, target = "review") {
  return `hosting:${review.site.provider}:${review.site.host}:${review.repository}:${review.number}:${target}`;
}
