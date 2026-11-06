import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Braces } from "lucide-react";
import { Dialog, DialogContent } from "../vendor/diffle/src/client/ui/dialog";
import { useStore } from "../vendor/diffle/src/client/store";
import type { LspStatus } from "../vendor/diffle/src/shared/protocol";

export function CodeIntelligence({ id, root }: { id: string; root: string }) {
  const [open, setOpen] = useState(false);
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const pending = useRef(false);
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  async function enable() {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError("");
    try {
      const lsp = await invoke<LspStatus>("enable_review_lsp", { id });
      if (!live.current) return;
      useStore.getState().setLspStatus(lsp);
      setEnabled(true);
      setOpen(false);
    } catch (e) {
      setError(String(e));
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  return (
    <>
      <button
        disabled={enabled}
        aria-label={
          enabled ? "Code intelligence enabled" : "Enable code intelligence"
        }
        onClick={() => setOpen(true)}
      >
        <Braces size={15} />
        <span className="code-intelligence-label">Code intelligence</span>
      </button>
      {open && (
        <Dialog open onOpenChange={setOpen}>
          <DialogContent
            className="settings code-trust"
            aria-labelledby="code-trust-title"
          >
            <h2 id="code-trust-title">Trust this repository?</h2>
            <p>
              Language servers can run build scripts and other code from this
              repository with your account’s access. Enable code intelligence
              only if you trust its contents.
            </p>
            <p className="trust-path">{root}</p>
            <p>This applies to this review only.</p>
            {error && <p role="alert">{error}</p>}
            <div className="trust-actions">
              <button disabled={busy} onClick={() => setOpen(false)}>
                Keep disabled
              </button>
              <button
                className="primary"
                disabled={busy}
                onClick={() => void enable()}
              >
                {busy ? "Enabling…" : "Trust and enable"}
              </button>
            </div>
          </DialogContent>
        </Dialog>
      )}
    </>
  );
}
