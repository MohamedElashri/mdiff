import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Code2, Copy, LoaderCircle, RefreshCw, X } from "lucide-react";
import { Dialog, DialogContent } from "../vendor/diffle/src/client/ui/dialog";
import { api } from "../vendor/diffle/src/client/api";
import { useStore } from "../vendor/diffle/src/client/store";

type Editor = { id: string; name: string };
type EditorList = { editors: Editor[]; temporary: boolean };

export function OpenInEditor({ id }: { id: string }) {
  const [open, setOpen] = useState(false);
  const [data, setData] = useState<EditorList>({
    editors: [],
    temporary: false,
  });
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [copyOnOpen, setCopyOnOpen] = useState(false);
  const hasComments = useStore((s) =>
    s.threads.some((thread) => !thread.resolved),
  );
  const generation = useRef(0);
  const pending = useRef(false);
  const lastEditor = (() => {
    try {
      return localStorage.getItem("mdiff.editor");
    } catch {
      return null;
    }
  })();

  async function refresh() {
    const request = ++generation.current;
    setLoading(true);
    setError("");
    try {
      const result = await invoke<EditorList>("discover_editors", { id });
      if (request === generation.current) setData(result);
    } catch (e) {
      if (request === generation.current) setError(String(e));
    } finally {
      if (request === generation.current) setLoading(false);
    }
  }
  useEffect(() => {
    if (open) {
      setStatus("");
      void refresh();
    }
    return () => {
      generation.current++;
    };
  }, [open, id]);

  async function copyPrompt() {
    const prompt = await api.exportComments();
    if (!prompt.trim())
      throw new Error("Add an open comment to create an agent prompt.");
    await writeText(prompt);
  }
  async function act(editor?: Editor) {
    if (pending.current) return;
    pending.current = true;
    setBusy(editor?.id ?? "copy");
    setError("");
    setStatus("");
    let copied = false;
    try {
      if (!editor || (copyOnOpen && hasComments)) {
        await copyPrompt();
        copied = true;
      }
      if (editor) {
        await invoke("open_in_editor", { id, editorId: editor.id });
        try {
          localStorage.setItem("mdiff.editor", editor.id);
        } catch {
          /* Optional preference. */
        }
      }
      setStatus(
        editor
          ? `${editor.name} opened.${copied ? " Paste the prompt into your agent." : ""}`
          : "Prompt copied. Paste it into your coding agent.",
      );
    } catch (e) {
      setError(
        `${copied ? "Prompt copied, but the editor could not open. " : ""}${String(e).replace(/^Error: /, "")}`,
      );
    } finally {
      pending.current = false;
      setBusy("");
    }
  }
  const editors = [...data.editors].sort((a, b) =>
    a.id === lastEditor ? -1 : b.id === lastEditor ? 1 : 0,
  );

  return (
    <>
      <button
        onClick={() => setOpen(true)}
        title="Open repository in an IDE"
        aria-label="Open in IDE"
      >
        <Code2 size={15} />
        <span className="editor-button-label">Open in IDE</span>
      </button>
      {open && (
        <Dialog open onOpenChange={setOpen}>
          <DialogContent
            className="settings editor-picker"
            aria-labelledby="editor-title"
          >
            <div className="editor-heading">
              <h2 id="editor-title">Open in IDE</h2>
              <button
                aria-label="Close editor picker"
                onClick={() => setOpen(false)}
              >
                <X size={18} />
              </button>
            </div>
            <p>Open the working folder in an installed editor.</p>
            {data.temporary && (
              <p className="editor-warning">
                This is a temporary clone. Keep this review open while using
                your editor.
              </p>
            )}
            <div className="editor-list" aria-busy={loading}>
              {loading ? (
                <p role="status">Finding editors…</p>
              ) : editors.length ? (
                editors.map((editor) => (
                  <button
                    key={editor.id}
                    onClick={() => void act(editor)}
                    disabled={!!busy}
                  >
                    {busy === editor.id ? (
                      <LoaderCircle size={17} className="spin" />
                    ) : (
                      <Code2 size={17} />
                    )}
                    <span>{editor.name}</span>
                    {editor.id === lastEditor && <small>Last used</small>}
                  </button>
                ))
              ) : (
                <p>
                  No supported editors found. Install an editor or enable its
                  command-line launcher, then refresh.
                </p>
              )}
            </div>
            <button onClick={() => void refresh()} disabled={loading || !!busy}>
              <RefreshCw size={14} />
              Refresh editors
            </button>
            <div className="editor-agent">
              <h3>AI agents</h3>
              <p>Copy open comments as a prompt for any coding agent.</p>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={copyOnOpen}
                  disabled={!hasComments || !!busy}
                  onChange={(e) => setCopyOnOpen(e.target.checked)}
                />
                Copy prompt when opening an editor
              </label>
              <button
                onClick={() => void act()}
                disabled={!hasComments || !!busy}
              >
                <Copy size={15} />
                Copy agent prompt
              </button>
              {!hasComments && (
                <small>Add an open comment to create a prompt.</small>
              )}
            </div>
            {error && (
              <p className="settings-error" role="alert">
                {error}
              </p>
            )}
            <p className="editor-status" role="status">
              {status}
            </p>
          </DialogContent>
        </Dialog>
      )}
    </>
  );
}
