import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { uiText } from "../../vendor/diffle/src/client/i18n/runtime";
import { ReviewPreferencesForm } from "../../vendor/diffle/src/client/header/SettingsDialog";
import type { UserConfig } from "../../vendor/diffle/src/shared/protocol";
export default function ReviewPreferences({
  onDirty,
}: {
  onDirty: (dirty: boolean) => void;
}) {
  const [config, setConfig] = useState<UserConfig | null>(null);
  const [error, setError] = useState("");
  const live = useRef(true);
  const load = async () => {
    const value = await invoke<UserConfig>("review_preferences", {
      settings: null,
    });
    if (live.current) {
      setConfig(value);
      setError("");
    }
  };
  useEffect(() => {
    live.current = true;
    void load().catch((e) => live.current && setError(String(e)));
    return () => {
      live.current = false;
    };
  }, []);
  return (
    <>
      {error && (
        <p role="alert" className="settings-error">
          {error}
        </p>
      )}
      {config ? (
        <ReviewPreferencesForm
          config={config}
          desktop
          onDirty={onDirty}
          onReload={load}
          saveConfig={async (patch) => {
            try {
              const saved = await invoke<UserConfig>("review_preferences", {
                settings: patch,
              });
              if (live.current) {
                setConfig(saved);
                setError("");
              }
              return true;
            } catch (e) {
              if (live.current) setError(String(e));
              return false;
            }
          }}
        />
      ) : (
        <>
          <p role="status">
            {error
              ? uiText("Review preferences could not be loaded.")
              : uiText("Loading review preferences…")}
          </p>
          {error && (
            <button
              onClick={() =>
                void load().catch((e) => live.current && setError(String(e)))
              }
            >
              {uiText("Retry")}
            </button>
          )}
        </>
      )}
    </>
  );
}
