import { useEffect, useState } from "react";
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
  const [data, setData] = useState<Data>();
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const call = <T,>(action: string, payload?: unknown) =>
    invoke<T>("gitlab_action", { id, action, payload: payload ?? null });
  async function load() {
    setData(await call<Data>("load"));
  }
  useEffect(() => {
    let live = true;
    void call<Data>("load")
      .then((d) => {
        if (live) setData(d);
      })
      .catch((e) => {
        if (live) setError(String(e));
      });
    return () => {
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
      setError(String(e));
      return false;
    } finally {
      setBusy(false);
    }
  }
  return (
    <aside className="gitlab-panel" aria-label="GitLab review">
      <button
        className="delete"
        onClick={onClose}
        aria-label="Close GitLab panel"
      >
        ×
      </button>
      <h2>GitLab review</h2>
      {data && (
        <>
          <a href={data.mergeRequest.web_url}>
            !{data.mergeRequest.iid} {data.mergeRequest.title}
          </a>
          <div className="actions">
            <button disabled={busy} onClick={() => act("export")}>
              Save drafts
            </button>
            <button
              disabled={
                busy || !data.drafts.some((d) => d.note.includes("<!-- mdiff:"))
              }
              onClick={() => act("publish")}
            >
              Publish drafts
            </button>
            <button
              disabled={busy}
              onClick={() => {
                setError("");
                void load().catch((e) => setError(String(e)));
              }}
            >
              Refresh
            </button>
          </div>
          <small>Only mdiff drafts for this revision are published.</small>
          {data.drafts
            .filter((d) => d.note.includes("<!-- mdiff:"))
            .map((d) => (
              <article key={d.id}>
                <strong>Draft</strong>
                <p>{d.note.replace(/<!-- mdiff:.*? -->/g, "")}</p>
              </article>
            ))}
          <h3>Discussions</h3>
          {data.discussions
            .filter((d) => d.notes.some((n) => !n.system))
            .map((d) => (
              <Thread key={d.id} discussion={d} disabled={busy} act={act} />
            ))}
          {!data.discussions.length && <p>No discussions yet.</p>}
        </>
      )}
      {!data && !error && <p>Loading merge request…</p>}
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
          {first.resolved ? "Reopen discussion" : "Resolve discussion"}
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
          aria-label="Reply to discussion"
          placeholder="Write a reply…"
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
        <button disabled={disabled || !body.trim()}>Publish reply</button>
      </form>
    </article>
  );
}
