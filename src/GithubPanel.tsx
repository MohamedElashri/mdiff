import { useEffect, useRef, useState } from "react";
import { ExternalLink, GitPullRequest, RefreshCw, X } from "lucide-react";
import { api } from "../vendor/diffle/src/client/api";
import { request } from "../vendor/diffle/src/client/transport";
import { useStore } from "../vendor/diffle/src/client/store";
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
async function call<T>(body?: unknown, signal?: AbortSignal): Promise<T> {
  const response = await request("api/desktop/github", {
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
  const open = threads.filter((t) => !t.resolved);
  const eligible = open.filter((t) => !t.stale);
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
      aria-label="GitHub review"
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
          <GitPullRequest size={18} /> GitHub
        </h2>
        <button onClick={onClose} aria-label="Close GitHub panel">
          <X size={16} />
        </button>
      </div>
      <div className="actions">
        <button disabled={!!busy} onClick={() => void refresh()}>
          <RefreshCw size={14} /> Refresh
        </button>
        <button onClick={onConnections}>Connection settings</button>
      </div>
      {metadata?.repositoryUrl && (
        <a href={metadata.repositoryUrl}>{metadata.repository}</a>
      )}
      <section>
        <h3>Pull request</h3>
        {pr ? (
          <>
            <a href={pr.url} className="github-pr-link">
              #{pr.number} {pr.title} <ExternalLink size={13} />
            </a>
            <small>
              {pr.state === "OPEN"
                ? pr.isDraft
                  ? "Draft"
                  : "Open"
                : pr.state === "MERGED"
                  ? "Merged"
                  : "Closed"}
            </small>
          </>
        ) : (
          <p>
            {github.status === "loading"
              ? "Finding the pull request…"
              : reason ||
                "Open an existing PR or create one from your current branch."}
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
            Open an existing PR
            <input
              required
              value={selector}
              onChange={(e) => setSelector(e.target.value)}
              placeholder="PR URL or number"
            />
          </label>
          <button disabled={!!busy || !selector.trim() || !context?.connected}>
            {busy === "open" ? "Opening…" : "Open PR in mdiff"}
          </button>
        </form>
      </section>
      <section>
        <h3>Transfer comments</h3>
        <p>
          {eligible.length} open {eligible.length === 1 ? "thread" : "threads"}{" "}
          ready
          {open.length > eligible.length
            ? ` · ${open.length - eligible.length} stale skipped`
            : ""}
          .
        </p>
        <p>
          Save comments to a pending GitHub review. Review and submit them on
          GitHub when you’re ready.
        </p>
        {!canExport && (
          <p className="github-hint">
            {reason || "Open the PR comparison to transfer comments."}
          </p>
        )}
        <button
          className="primary"
          disabled={!!busy || !canExport || !eligible.length}
          onClick={() =>
            void act("export", async () => {
              const target = useStore.getState().snapshot;
              const result = await api.exportToGithub({});
              if (live.current && useStore.getState().snapshot === target)
                setExported(result);
            })
          }
        >
          {busy === "export" ? "Saving comments…" : "Save comments to GitHub"}
        </button>
        {exported && (
          <div role="status" className="github-result">
            <p>
              {exported.posted} added · {exported.updated} updated ·{" "}
              {exported.skipped.length} skipped. Local comments are kept.
            </p>
            {exported.skipped.length > 0 && (
              <details>
                <summary>Skipped threads</summary>
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
              Review and submit on GitHub <ExternalLink size={13} />
            </a>
          </div>
        )}
      </section>
      <section>
        <button aria-expanded={creating} onClick={() => setCreating((v) => !v)}>
          Create a pull request
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
            {!context && !error && <p>Checking your pushed branch…</p>}
            {context?.reason && <p className="github-hint">{context.reason}</p>}
            {context?.branch && (
              <p>
                From {context.headRepository}:{context.branch} on {context.host}
              </p>
            )}
            <p>
              Includes pushed commits from the checked-out branch. Commit and
              push local changes in Git first.
            </p>
            <fieldset
              disabled={!!busy || !!context?.reason || !context || !!pull}
            >
              <label>
                Destination repository
                <input
                  required
                  value={repository}
                  placeholder="owner/repository"
                  onChange={(e) => setRepository(e.target.value)}
                />
              </label>
              <label>
                Base branch
                <input
                  required
                  value={base}
                  onChange={(e) => setBase(e.target.value)}
                />
              </label>
              <label>
                Title
                <input
                  required
                  maxLength={256}
                  value={title}
                  onChange={(e) => setTitle(e.target.value)}
                />
              </label>
              <label>
                Description
                <textarea
                  value={body}
                  maxLength={65536}
                  onChange={(e) => setBody(e.target.value)}
                  placeholder="What changed and how was it tested?"
                />
              </label>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={draft}
                  onChange={(e) => setDraft(e.target.checked)}
                />
                Create as draft
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
                  ? "Creating…"
                  : draft
                    ? "Create draft PR"
                    : "Create PR"}
              </button>
            </fieldset>
            {pull && (
              <div role="status" className="github-result">
                <p>
                  {pull.existing
                    ? "An open pull request already exists"
                    : "Pull request created"}
                  : #{pull.number}.
                </p>
                <a href={pull.url}>
                  Open on GitHub <ExternalLink size={13} />
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
                  Review this PR in mdiff
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
