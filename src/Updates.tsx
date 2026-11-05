import { useEffect, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { check } from "@tauri-apps/plugin-updater";
import Markdown from "react-markdown";
import {
  ArrowUpRight,
  Check,
  Download,
  RefreshCw,
  RotateCcw,
  ShieldCheck,
  AlertCircle,
} from "lucide-react";
import { UpdateController, type UpdateInfo } from "./update-controller";

// Closing Preferences does not cancel an update or allow a second installation.
export const updates = new UpdateController({
  info: () => invoke<UpdateInfo>("updater_info"),
  check: () => check({ timeout: 20_000 }),
  restart: () => invoke("restart_after_update"),
});

export function Updates() {
  const state = useSyncExternalStore(updates.subscribe, updates.getSnapshot);
  const [info, setInfo] = useState<UpdateInfo>();
  const [infoError, setInfoError] = useState("");
  useEffect(() => {
    let active = true;
    // Read local metadata; only the Check button makes a network request.
    void invoke<UpdateInfo>("updater_info")
      .then((value) => {
        if (active) setInfo(value);
      })
      .catch((error) => {
        if (active) setInfoError(String(error));
      });
    return () => {
      active = false;
    };
  }, []);
  const busy = ["checking", "downloading", "installing", "restarting"].includes(
    state.phase,
  );
  const failed = !!(state.error || infoError);
  const progress = state.total
    ? Math.min(100, Math.round((state.downloaded / state.total) * 100))
    : undefined;
  const title = {
    idle: failed ? "Couldn’t check for updates" : "Ready when you are",
    checking: "Checking for updates…",
    current: "You’re up to date",
    available: failed
      ? "Update couldn’t finish"
      : `mdiff ${state.version} is available`,
    downloading: `Downloading mdiff ${state.version}`,
    installing: "Installing your update…",
    ready: "Ready to restart",
    restarting: "Restarting mdiff…",
  }[state.phase];
  const description = {
    idle: failed
      ? "The update service is unavailable. Check your connection or try again later."
      : "Check GitHub for the latest stable release.",
    checking: "Looking for a newer version of mdiff.",
    current: `You’re running the latest available version${info ? `, ${info.version}` : ""}.`,
    available: failed
      ? "Review the error below, then try again."
      : "Review what’s changed, then install when it suits you.",
    downloading: "You can close Preferences while the download finishes.",
    installing: "The download was verified. Please keep mdiff open.",
    ready:
      "Save any unfinished comment before restarting. Your saved reviews stay in place.",
    restarting: "Closing the current review and its language servers.",
  }[state.phase];
  const Icon = failed
    ? AlertCircle
    : ["current", "ready"].includes(state.phase)
      ? Check
      : busy
        ? RefreshCw
        : Download;
  const primaryLabel =
    state.phase === "ready"
      ? "Restart mdiff"
      : state.phase === "available"
        ? "Download and install"
        : state.phase === "checking"
          ? "Checking…"
          : "Check for updates";
  function primaryAction() {
    if (state.phase === "ready") void updates.restart();
    else if (state.phase === "available") void updates.install();
    else void updates.check();
  }
  return (
    <section className="update-page" aria-label="App updates">
      <header className="settings-page-heading">
        <h3>App updates</h3>
        {info && (
          <span className="app-version">Installed · {info.version}</span>
        )}
      </header>
      <div className={`update-status${failed ? " has-error" : ""}`}>
        <div className="update-status-icon">
          <Icon size={22} className={busy ? "spin" : undefined} />
        </div>
        <div className="update-status-body">
          <div role="status" aria-live="polite">
            <h4>{info?.reason || title}</h4>
            {!info?.reason && <p>{description}</p>}
          </div>
          {state.phase === "downloading" && (
            <div className="update-progress">
              <progress
                aria-label="Update download"
                max={100}
                value={progress}
              />
              <span>
                {progress === undefined
                  ? `${(state.downloaded / 1048576).toFixed(1)} MB downloaded`
                  : `${progress}%`}
              </span>
            </div>
          )}
          {!busy && (
            <button
              className="primary"
              disabled={!info?.supported}
              onClick={primaryAction}
            >
              {state.phase === "ready" ? (
                <RotateCcw size={15} />
              ) : state.phase === "available" ? (
                <Download size={15} />
              ) : (
                <RefreshCw size={15} />
              )}
              {primaryLabel}
            </button>
          )}
          {failed && (
            <details className="update-error" role="alert">
              <summary>Error details</summary>
              <p>{state.error || infoError}</p>
            </details>
          )}
        </div>
      </div>
      {state.notes && (
        <section className="update-release-notes" aria-label="Release notes">
          <h4>What’s new in {state.version}</h4>
          <div>
            <Markdown>{state.notes}</Markdown>
          </div>
        </section>
      )}
      <footer className="update-footer">
        <span>
          <ShieldCheck size={15} />
          Signed updates. Always your choice.
        </span>
        <a href="https://github.com/MohamedElashri/mdiff/releases">
          All releases
          <ArrowUpRight size={14} />
        </a>
      </footer>
    </section>
  );
}
