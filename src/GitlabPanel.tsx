import { uiText } from "../vendor/diffle/src/client/i18n/runtime";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
type Note = {
  id: number;
  body: string;
  author: { name: string };
  system: boolean;
  resolvable: boolean;
  resolved: boolean;
  position?: { new_path: string; new_line?: number; old_line?: number };
};
type Discussion = { id: string; notes: Note[] };
type Data = {
  mergeRequest: { title: string; web_url: string; iid: number };
  discussions: Discussion[];
  drafts: { id: number; note: string }[];
};
export function GitlabPanel({
  id,
  onClose,
}: {
  id: string;
  onClose: () => void;
}) {
  return <GitlabReview key={id} id={id} onClose={onClose} />;
}
function GitlabReview({ id, onClose }: { id: string; onClose: () => void }) {
  const [data, setData] = useState<Data>();
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const mounted = useRef(true);
  const call = <T,>(action: string, payload?: unknown) =>
    invoke<T>("gitlab_action", { id, action, payload: payload ?? null });
  async function load() {
    const data = await call<Data>("load");
    if (mounted.current) setData(data);
  }
  useEffect(() => {
    mounted.current = true;
    let live = true;
    void call<Data>("load")
      .then((d) => {
        if (live) setData(d);
      })
      .catch((e) => {
        if (live) setError(String(e));
      });
    return () => {
      mounted.current = false;
      live = false;
    };
  }, [id]);
  async function act(action: string, payload?: unknown) {
    setBusy(true);
    setError("");
    setStatus("");
    try {
      const result = await call<{
        exported?: number;
        skipped?: number;
        published?: number;
      }>(action, payload);
      if (!mounted.current) return false;
      setStatus(
        action === "export"
          ? `${result.exported} drafts saved; ${result.skipped} stale or previously published threads skipped.`
          : action === "publish"
            ? `${result.published} drafts published.`
            : "Saved.",
      );
      await load();
      return true;
    } catch (e) {
      if (mounted.current) setError(String(e));
      return false;
    } finally {
      if (mounted.current) setBusy(false);
    }
  }
  return (
    <aside className="gitlab-panel" aria-label={uiText("GitLab review")}>
      <button
        className="delete"
        onClick={onClose}
        aria-label={uiText("Close GitLab panel")}
      >
        ×
      </button>
      <h2>{uiText("GitLab review")}</h2>
      {data && (
        <>
          <a href={data.mergeRequest.web_url}>
            !{data.mergeRequest.iid} {data.mergeRequest.title}
          </a>
          <div className="actions">
            <button disabled={busy} onClick={() => act("export")}>
              {uiText("Save drafts")}
            </button>
            <button
              disabled={
                busy || !data.drafts.some((d) => d.note.includes("<!-- mdiff:"))
              }
              onClick={() => act("publish")}
            >
              {uiText("Publish drafts")}
            </button>
            <button
              disabled={busy}
              onClick={() => {
                setError("");
                void load().catch((e) => setError(String(e)));
              }}
            >
              {uiText("Refresh")}
            </button>
          </div>
          <small>
            {uiText("Only mdiff drafts for this revision are published.")}
          </small>
          {data.drafts
            .filter((d) => d.note.includes("<!-- mdiff:"))
            .map((d) => (
              <article key={d.id}>
                <strong>{uiText("Draft")}</strong>
                <p>{d.note.replace(/<!-- mdiff:.*? -->/g, "")}</p>
              </article>
            ))}
          <h3>{uiText("Discussions")}</h3>
          {data.discussions
            .filter((d) => d.notes.some((n) => !n.system))
            .map((d) => (
              <Thread key={d.id} discussion={d} disabled={busy} act={act} />
            ))}
          {!data.discussions.length && <p>{uiText("No discussions yet.")}</p>}
        </>
      )}
      {!data && !error && <p>{uiText("Loading merge request…")}</p>}
      {error && <p role="alert">{error}</p>}
      {status && <p role="status">{status}</p>}
    </aside>
  );
}
function Thread({
  discussion,
  disabled,
  act,
}: {
  discussion: Discussion;
  disabled: boolean;
  act: (a: string, p?: unknown) => Promise<boolean>;
}) {
  const [body, setBody] = useState("");
  const first = discussion.notes[0];
  return (
    <article>
      {discussion.notes
        .filter((n) => !n.system)
        .map((n) => (
          <div key={n.id}>
            <strong>{n.author.name}</strong>
            {n.position && (
              <small>
                {" "}
                · {n.position.new_path}:
                {n.position.new_line ?? n.position.old_line}
              </small>
            )}
            <p>{n.body}</p>
          </div>
        ))}
      {first?.resolvable && (
        <button
          disabled={disabled}
          onClick={() =>
            act("resolve", {
              discussion: discussion.id,
              resolved: !first.resolved,
            })
          }
        >
          {first.resolved
            ? uiText("Reopen discussion")
            : uiText("Resolve discussion")}
        </button>
      )}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void act("reply", { discussion: discussion.id, body }).then((ok) => {
            if (ok) setBody("");
          });
        }}
      >
        <textarea
          aria-label={uiText("Reply to discussion")}
          placeholder={uiText("Write a reply…")}
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
        <button disabled={disabled || !body.trim()}>
          {uiText("Publish reply")}
        </button>
      </form>
    </article>
  );
}
