import { uiText } from "../vendor/diffle/src/client/i18n/runtime";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Braces } from "lucide-react";
import { Dialog, DialogContent } from "../vendor/diffle/src/client/ui/dialog";
import type { LspStatus } from "../vendor/diffle/src/shared/protocol";

export function CodeIntelligence({
  id,
  root,
  initialEnabled = false,
}: {
  id: string;
  root: string;
  initialEnabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [enabled, setEnabled] = useState(initialEnabled);
  useEffect(() => setEnabled(initialEnabled), [initialEnabled]);
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
      void lsp; // Native status is broadcast to this review's scoped subscription.
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
          enabled
            ? uiText("Code intelligence enabled")
            : uiText("Enable code intelligence")
        }
        onClick={() => setOpen(true)}
      >
        <Braces size={15} />
        <span className="code-intelligence-label">
          {uiText("Code intelligence")}
        </span>
      </button>
      {open && (
        <Dialog open onOpenChange={setOpen}>
          <DialogContent
            className="settings code-trust"
            aria-labelledby="code-trust-title"
          >
            <h2 id="code-trust-title">{uiText("Trust this repository?")}</h2>
            <p>
              {uiText(
                "Language servers can run build scripts and other code from this repository with your account’s access. Enable code intelligence only if you trust its contents.",
              )}
            </p>
            <p className="trust-path">{root}</p>
            <p>{uiText("This applies to this review only.")}</p>
            {error && <p role="alert">{error}</p>}
            <div className="trust-actions">
              <button disabled={busy} onClick={() => setOpen(false)}>
                {uiText("Keep disabled")}
              </button>
              <button
                className="primary"
                disabled={busy}
                onClick={() => void enable()}
              >
                {busy ? uiText("Enabling…") : uiText("Trust and enable")}
              </button>
            </div>
          </DialogContent>
        </Dialog>
      )}
    </>
  );
}
