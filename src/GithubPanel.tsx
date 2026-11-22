import { uiText } from "../vendor/diffle/src/client/i18n/runtime";
import { useEffect, useMemo, useRef, useState } from "react";
import { ExternalLink, GitPullRequest, RefreshCw, X } from "lucide-react";
import { useApi, type ReviewApi } from "../vendor/diffle/src/client/api";
import { useStore, useReviewStore } from "../vendor/diffle/src/client/store";
import type { GithubExportResponse } from "../vendor/diffle/src/shared/protocol";

type Context = {
  connected: boolean;
  host: string;
  repository: string | null;
  headRepository: string | null;
  branch: string | null;
  head: string | null;
  base: string | null;
  title: string | null;
  reason: string | null;
};
type Pull = { url: string; number: number; existing: boolean };
async function callApi<T>(
  api: ReviewApi,
  body?: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const response = await api.request("api/desktop/github", {
    method: body ? "POST" : "GET",
    headers: { "content-type": "application/json" },
    body: body ? JSON.stringify(body) : undefined,
    signal,
  });
  const value = await response.json();
  if (!response.ok) throw new Error(value.error || "GitHub request failed.");
  return value;
}

export function GithubPanel({
  onClose,
  onConnections,
}: {
  onClose: () => void;
  onConnections: () => void;
}) {
  const useStore = useReviewStore();
  const api = useApi();
  const call = <T,>(body?: unknown, signal?: AbortSignal) =>
    callApi<T>(api, body, signal);
  const authGeneration = useStore((s) => s.authGeneration);
  const github = useStore((s) => s.github);
  const threads = useStore((s) => s.threads);
  const snapshot = useStore((s) => s.snapshot);
  const [context, setContext] = useState<Context>();
  const [repository, setRepository] = useState("");
  const [base, setBase] = useState("");
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [draft, setDraft] = useState(true);
  const [creating, setCreating] = useState(false);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [pull, setPull] = useState<Pull>();
  const [selector, setSelector] = useState("");
  const [exported, setExported] = useState<GithubExportResponse>();
  const live = useRef(true);
  const pending = useRef(false);
  const controller = useRef<AbortController | null>(null);
  const panel = useRef<HTMLElement>(null);
  const previousSnapshot = useRef(snapshot);
  const metadata = github.data;
  const pr = metadata?.pullRequest;
  const { eligible, stale } = useMemo(() => {
    let eligible = 0;
    let stale = 0;
    for (const thread of threads) {
      if (thread.resolved) continue;
      if (thread.stale) stale++;
      else eligible++;
    }
    return { eligible, stale };
  }, [threads]);
  const canExport = metadata?.reason === null;
  const reason =
    context && !context.connected
      ? "Connect GitHub in Connection settings, then reopen this review."
      : metadata?.reason;

  async function refresh() {
    controller.current?.abort();
    const abort = new AbortController();
    controller.current = abort;
    setError("");
    setContext(undefined);
    void useStore.getState().refreshGithub();
    try {
      const next = await call<Context>(undefined, abort.signal);
      if (!live.current || abort.signal.aborted) return;
      setContext(next);
      setRepository((value) => value || next.repository || "");
      setBase((value) => value || next.base || "");
      setTitle((value) => value || next.title || "");
    } catch (e) {
      if (live.current && !abort.signal.aborted) setError(String(e));
    }
  }
  useEffect(() => {
    if (!authGeneration) return;
    controller.current?.abort();
    setContext(undefined);
    setPull(undefined);
    setExported(undefined);
    void refresh();
  }, [authGeneration]);
  useEffect(() => {
    live.current = true;
    panel.current?.focus();
    void refresh();
    return () => {
      live.current = false;
      controller.current?.abort();
    };
  }, []);
  useEffect(() => {
    if (previousSnapshot.current !== snapshot) {
      previousSnapshot.current = snapshot;
      setExported(undefined);
    }
  }, [snapshot]);
  async function act(action: string, work: () => Promise<void>) {
    if (pending.current) return;
    pending.current = true;
    setBusy(action);
    setError("");
    try {
      await work();
    } catch (e) {
      if (live.current) setError(String(e));
    } finally {
      pending.current = false;
      if (live.current) setBusy("");
    }
  }
  return (
    <aside
      id="desktop-github-panel"
      className="gitlab-panel github-panel"
      aria-label={uiText("GitHub review")}
      ref={panel}
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="github-heading">
        <h2>
          <GitPullRequest size={18} />
          {uiText(" GitHub")}
        </h2>
        <button onClick={onClose} aria-label={uiText("Close GitHub panel")}>
          <X size={16} />
        </button>
      </div>
      <div className="actions">
        <button disabled={!!busy} onClick={() => void refresh()}>
          <RefreshCw size={14} />
          {uiText(" Refresh")}
        </button>
        <button onClick={onConnections}>{uiText("Connection settings")}</button>
      </div>
      {metadata?.repositoryUrl && (
        <a href={metadata.repositoryUrl}>{metadata.repository}</a>
      )}
      <section>
        <h3>{uiText("Pull request")}</h3>
        {pr ? (
          <>
            <a href={pr.url} className="github-pr-link">
              #{pr.number} {pr.title} <ExternalLink size={13} />
            </a>
            <small>
              {pr.state === "OPEN"
                ? pr.isDraft
                  ? uiText("Draft")
                  : uiText("Open")
                : pr.state === "MERGED"
                  ? uiText("Merged")
                  : uiText("Closed")}
            </small>
          </>
        ) : (
          <p>
            {github.status === "loading"
              ? uiText("Finding the pull request…")
              : reason ||
                uiText(
                  "Open an existing PR or create one from your current branch.",
                )}
          </p>
        )}
        {github.status === "error" && <p role="alert">{github.error}</p>}
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void act("open", async () => {
              const result = await useStore
                .getState()
                .enter({ kind: "pr", pr: selector.trim() });
              if (typeof result === "object") throw new Error(result.error);
            });
          }}
        >
          <label>
            {uiText("Open an existing PR")}
            <input
              required
              value={selector}
              onChange={(e) => setSelector(e.target.value)}
              placeholder={uiText("PR URL or number")}
            />
          </label>
          <button disabled={!!busy || !selector.trim() || !context?.connected}>
            {busy === "open" ? uiText("Opening…") : uiText("Open PR in mdiff")}
          </button>
        </form>
      </section>
      <section>
        <h3>{uiText("Transfer comments")}</h3>
        <p>
          {eligible}
          {uiText(" open ")}
          {eligible === 1 ? uiText("thread") : uiText("threads")}
          {uiText(" ready")}
          {stale > 0
            ? uiText(" · {value0} stale skipped", { value0: String(stale) })
            : ""}
          .
        </p>
        <p>
          {uiText(
            "Save comments to a pending GitHub review. Review and submit them on GitHub when you’re ready.",
          )}
        </p>
        {!canExport && (
          <p className="github-hint">
            {reason || uiText("Open the PR comparison to transfer comments.")}
          </p>
        )}
        <button
          className="primary"
          disabled={!!busy || !canExport || !eligible}
          onClick={() =>
            void act("export", async () => {
              const target = useStore.getState().snapshot;
              const result = await api.exportToGithub({});
              if (live.current && useStore.getState().snapshot === target)
                setExported(result);
            })
          }
        >
          {busy === "export"
            ? uiText("Saving comments…")
            : uiText("Save comments to GitHub")}
        </button>
        {exported && (
          <div role="status" className="github-result">
            <p>
              {exported.posted}
              {uiText(" added · ")}
              {exported.updated}
              {uiText(" updated ·")} {exported.skipped.length}
              {uiText(" skipped. Local comments are kept.")}
            </p>
            {exported.skipped.length > 0 && (
              <details>
                <summary>{uiText("Skipped threads")}</summary>
                <ul>
                  {exported.skipped.map((s) => (
                    <li key={s.id}>
                      {threads.find((t) => t.id === s.id)?.anchor.path || s.id}:{" "}
                      {s.reason}
                    </li>
                  ))}
                </ul>
              </details>
            )}
            <a href={exported.url}>
              {uiText("Review and submit on GitHub ")}
              <ExternalLink size={13} />
            </a>
          </div>
        )}
      </section>
      <section>
        <button aria-expanded={creating} onClick={() => setCreating((v) => !v)}>
          {uiText("Create a pull request")}
        </button>
        {creating && (
          <form
            onSubmit={(event) => {
              event.preventDefault();
              if (!context || context.reason || pull) return;
              void act("create", async () => {
                const result = await call<Pull>({
                  repository: repository.trim(),
                  headRepository: context.headRepository,
                  branch: context.branch,
                  head: context.head,
                  base: base.trim(),
                  title: title.trim(),
                  body,
                  draft,
                });
                if (live.current) setPull(result);
              });
            }}
          >
            {!context && !error && (
              <p>{uiText("Checking your pushed branch…")}</p>
            )}
            {context?.reason && <p className="github-hint">{context.reason}</p>}
            {context?.branch && (
              <p>
                {uiText("From ")}
                {context.headRepository}:{context.branch}
                {uiText(" on ")}
                {context.host}
              </p>
            )}
            <p>
              {uiText(
                "Includes pushed commits from the checked-out branch. Commit and push local changes in Git first.",
              )}
            </p>
            <fieldset
              disabled={!!busy || !!context?.reason || !context || !!pull}
            >
              <label>
                {uiText("Destination repository")}
                <input
                  required
                  value={repository}
                  placeholder={uiText("owner/repository")}
                  onChange={(e) => setRepository(e.target.value)}
                />
              </label>
              <label>
                {uiText("Base branch")}
                <input
                  required
                  value={base}
                  onChange={(e) => setBase(e.target.value)}
                />
              </label>
              <label>
                {uiText("Title")}
                <input
                  required
                  maxLength={256}
                  value={title}
                  onChange={(e) => setTitle(e.target.value)}
                />
              </label>
              <label>
                {uiText("Description")}
                <textarea
                  value={body}
                  maxLength={65536}
                  onChange={(e) => setBody(e.target.value)}
                  placeholder={uiText("What changed and how was it tested?")}
                />
              </label>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={draft}
                  onChange={(e) => setDraft(e.target.checked)}
                />
                {uiText("Create as draft")}
              </label>
              <button
                className="primary"
                disabled={
                  !title.trim() ||
                  !base.trim() ||
                  !repository.trim() ||
                  !context?.head
                }
              >
                {busy === "create"
                  ? uiText("Creating…")
                  : draft
                    ? uiText("Create draft PR")
                    : uiText("Create PR")}
              </button>
            </fieldset>
            {pull && (
              <div role="status" className="github-result">
                <p>
                  {pull.existing
                    ? uiText("An open pull request already exists")
                    : uiText("Pull request created")}
                  : #{pull.number}.
                </p>
                <a href={pull.url}>
                  {uiText("Open on GitHub ")}
                  <ExternalLink size={13} />
                </a>
                <button
                  type="button"
                  disabled={!!busy}
                  onClick={() =>
                    void act("open", async () => {
                      const result = await useStore
                        .getState()
                        .enter({ kind: "pr", pr: pull.url });
                      if (typeof result === "object")
                        throw new Error(result.error);
                      if (live.current && result === "applied")
                        setCreating(false);
                    })
                  }
                >
                  {uiText("Review this PR in mdiff")}
                </button>
              </div>
            )}
          </form>
        )}
      </section>
      {error && (
        <p role="alert" className="github-error">
          {error.replace(/^Error: /, "")}
        </p>
      )}
    </aside>
  );
}
