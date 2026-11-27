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
type PositionLine = {
  type?: "old" | "new";
  old_line?: number;
  new_line?: number;
};
type Position = PositionLine & {
  new_path?: string;
  old_path?: string;
  position_type: "file" | "text";
  head_sha: string;
  line_range?: { start: PositionLine; end: PositionLine };
};
type ExportPreview = {
  version: number;
  drafts: { id: string; path: string; position: Position }[];
  skipped: { id: string; path: string; reason: string }[];
};

function location(position: Position) {
  const path = position.new_path ?? position.old_path;
  if (position.position_type === "file")
    return `${path} · ${uiText("whole file")}`;
  const label = (line: PositionLine) => {
    const parts = [];
    if (line.old_line != null) parts.push(`${uiText("old")} L${line.old_line}`);
    if (line.new_line != null) parts.push(`${uiText("new")} L${line.new_line}`);
    return parts.join(" / ");
  };
  const range = position.line_range;
  return `${path} · ${range ? `${label(range.start)}–${label(range.end)}` : label(position)}`;
}

type Data = {
  mergeRequest: {
    title: string;
    web_url: string;
    iid: number;
    diff_refs: { head_sha: string };
  };
  discussions: Discussion[];
  drafts: {
    id: number;
    note: string;
    position: Position | null;
    publishable: boolean;
  }[];
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
  const [preview, setPreview] = useState<ExportPreview>();
  const [confirmPublish, setConfirmPublish] = useState(false);
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
  async function previewExport() {
    setBusy(true);
    setError("");
    setStatus("");
    setConfirmPublish(false);
    setPreview(undefined);
    try {
      const next = await call<ExportPreview>("preview_export");
      if (mounted.current) setPreview(next);
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  }
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
      setPreview(undefined);
      setConfirmPublish(false);
      setStatus(
        action === "export"
          ? `${result.exported} drafts saved; ${result.skipped} threads skipped. Local comments are kept.`
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
  const publishable = data?.drafts.filter((draft) => draft.publishable) ?? [];
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
            <button disabled={busy} onClick={() => void previewExport()}>
              {uiText("Preview drafts")}
            </button>
            <button
              disabled={busy || !publishable.length || confirmPublish}
              aria-expanded={confirmPublish}
              onClick={() => {
                setPreview(undefined);
                setConfirmPublish(true);
              }}
            >
              {uiText("Publish drafts")}
            </button>
            <button
              disabled={busy}
              onClick={() => {
                setError("");
                setPreview(undefined);
                setConfirmPublish(false);
                void load().catch((e) => setError(String(e)));
              }}
            >
              {uiText("Refresh")}
            </button>
          </div>
          {preview && (
            <section aria-label={uiText("Draft positions")}>
              <h3>{uiText("Draft positions")}</h3>
              <code>{data.mergeRequest.diff_refs.head_sha.slice(0, 12)}</code>
              {preview.drafts.length > 0 ? (
                <ul>
                  {preview.drafts.map((draft) => (
                    <li key={draft.id}>
                      <code>{location(draft.position)}</code>
                    </li>
                  ))}
                </ul>
              ) : (
                <p>{uiText("No drafts ready to save.")}</p>
              )}
              {preview.skipped.length > 0 && (
                <ul>
                  {preview.skipped.map((draft) => (
                    <li key={draft.id}>
                      <code>{draft.path}</code> · {draft.reason}
                    </li>
                  ))}
                </ul>
              )}
              <div className="actions">
                <button
                  disabled={busy || !preview.drafts.length}
                  onClick={() => void act("export", preview)}
                >
                  {uiText("Save drafts")}
                </button>
                <button disabled={busy} onClick={() => setPreview(undefined)}>
                  {uiText("Cancel")}
                </button>
              </div>
            </section>
          )}
          {confirmPublish && (
            <section aria-label={uiText("Confirm publication")}>
              <h3>{uiText("Confirm publication")}</h3>
              <code>{data.mergeRequest.diff_refs.head_sha.slice(0, 12)}</code>
              <ul>
                {publishable.map((draft) => (
                  <li key={draft.id}>
                    <code>{draft.position && location(draft.position)}</code>
                  </li>
                ))}
              </ul>
              <div className="actions">
                <button
                  disabled={busy || !publishable.length}
                  onClick={() =>
                    void act(
                      "publish",
                      publishable.map(({ id, position }) => ({ id, position })),
                    )
                  }
                >
                  {uiText("Confirm and publish drafts")}
                </button>
                <button
                  disabled={busy}
                  onClick={() => setConfirmPublish(false)}
                >
                  {uiText("Cancel")}
                </button>
              </div>
            </section>
          )}
          {data.drafts.map((d) => (
            <article key={d.id}>
              <strong>{uiText("Draft")}</strong>
              {d.publishable && d.position ? (
                <p>
                  <code>{location(d.position)}</code>
                </p>
              ) : (
                <p role="status">
                  {uiText(
                    "This draft has no matching diff position. Choose a new anchor in Changes.",
                  )}
                </p>
              )}
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
