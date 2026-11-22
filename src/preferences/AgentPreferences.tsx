import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { uiText } from "../../vendor/diffle/src/client/i18n/runtime";
import { AgentSettings } from "../agents/AgentSettings";
import { agentApi, type View } from "../agents/api";
export default function AgentPreferences({
  onDirty,
}: {
  onDirty: (dirty: boolean) => void;
}) {
  const [view, setView] = useState<View | null>(null),
    [error, setError] = useState(""),
    [loadError, setLoadError] = useState(""),
    [busy, setBusy] = useState(""),
    [message, setMessage] = useState("");
  const live = useRef(true),
    pending = useRef(false),
    version = useRef(-1);
  const accept = (v: View) => {
    if (live.current && v.version >= version.current) {
      version.current = v.version;
      setView(v);
      setLoadError("");
    }
  };
  const load = async () => {
    accept(await agentApi.settings());
  };
  useEffect(() => {
    live.current = true;
    void load().catch((e) => live.current && setLoadError(String(e)));
    const listener = listen("agent-activity-changed", () => {
      void load().catch((e) => live.current && setLoadError(String(e)));
    });
    void listener.catch((e) => live.current && setError(String(e)));
    return () => {
      live.current = false;
      void listener.then((stop) => stop()).catch(() => {});
    };
  }, []);
  const act = async (label: string, task: () => Promise<unknown>) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(label);
    setError("");
    setMessage("");
    try {
      await task();
    } catch (e) {
      if (live.current) setError(String(e));
    } finally {
      pending.current = false;
      if (live.current) setBusy("");
    }
  };
  return (
    <>
      {(error || loadError) && (
        <p role="alert" className="settings-error">
          {error || loadError}
        </p>
      )}
      {message && <p role="status">{message}</p>}
      {view ? (
        <AgentSettings
          view={view}
          busy={busy}
          act={act}
          accept={accept}
          reload={async () => accept(await agentApi.reload())}
          notify={setMessage}
          onDirty={onDirty}
        />
      ) : (
        <>
          <p role="status">
            {error || loadError
              ? uiText("Agent preferences could not be loaded.")
              : uiText("Loading agent preferences…")}
          </p>
          {(error || loadError) && (
            <button onClick={() => void act("reload", load)}>
              {uiText("Retry")}
            </button>
          )}
        </>
      )}
    </>
  );
}
