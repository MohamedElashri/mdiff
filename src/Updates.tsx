import { uiText } from "../vendor/diffle/src/client/i18n/runtime";
import {
  lazy,
  Suspense,
  useEffect,
  useState,
  useSyncExternalStore,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { check } from "@tauri-apps/plugin-updater";
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

const ReleaseNotesMarkdown = lazy(() => import("react-markdown"));

// Closing Preferences does not cancel an update or allow a second installation.
export const updates = new UpdateController({
  info: () => invoke<UpdateInfo>("updater_info"),
  check: async () => {
    await invoke("updater_lease", { acquire: true });
    try {
      const update = await check({ timeout: 20_000 });
      if (!update) return null;
      return {
        version: update.version,
        body: update.body,
        async download(progress, options) {
          await invoke("updater_lease", { acquire: true });
          try {
            await update.download(progress, options);
          } catch (error) {
            await invoke("updater_lease", { acquire: false }).catch(() => {});
            throw error;
          }
        },
        async install() {
          try {
            await update.install();
          } finally {
            await invoke("updater_lease", { acquire: false });
          }
        },
        close: () => update.close(),
      };
    } finally {
      await invoke("updater_lease", { acquire: false });
    }
  },
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
    idle: failed
      ? uiText("Couldn’t check for updates")
      : uiText("Ready when you are"),
    checking: uiText("Checking for updates…"),
    current: uiText("You’re up to date"),
    available: failed
      ? uiText("Update couldn’t finish")
      : uiText("mdiff {value0} is available", {
          value0: String(state.version),
        }),
    downloading: uiText("Downloading mdiff {value0}", {
      value0: String(state.version),
    }),
    installing: uiText("Installing your update…"),
    ready: uiText("Ready to restart"),
    restarting: uiText("Restarting mdiff…"),
  }[state.phase];
  const description = {
    idle: failed
      ? uiText(
          "The update service is unavailable. Check your connection or try again later.",
        )
      : uiText("Check GitHub for the latest stable release."),
    checking: uiText("Looking for a newer version of mdiff."),
    current: uiText("You’re running the latest available version{value0}.", {
      value0: String(info ? `, ${info.version}` : ""),
    }),
    available: failed
      ? uiText("Review the error below, then try again.")
      : uiText("Review what’s changed, then install when it suits you."),
    downloading: uiText(
      "You can close Preferences while the download finishes.",
    ),
    installing: uiText("The download was verified. Please keep mdiff open."),
    ready: uiText(
      "Save any unfinished comment before restarting. Your saved reviews stay in place.",
    ),
    restarting: uiText("Closing the current review and its language servers."),
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
      ? uiText("Restart mdiff")
      : state.phase === "available"
        ? uiText("Download and install")
        : state.phase === "checking"
          ? uiText("Checking…")
          : uiText("Check for updates");
  function primaryAction() {
    if (state.phase === "ready") void updates.restart();
    else if (state.phase === "available") void updates.install();
    else void updates.check();
  }
  return (
    <section className="update-page" aria-label={uiText("App updates")}>
      <header className="settings-page-heading">
        <h3>{uiText("App updates")}</h3>
        {info && (
          <span className="app-version">
            {uiText("Installed · ")}
            {info.version}
          </span>
        )}
      </header>
      <div className={`update-status${String(failed ? " has-error" : "")}`}>
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
                aria-label={uiText("Update download")}
                max={100}
                value={progress}
              />
              <span>
                {progress === undefined
                  ? uiText("{value0} MB downloaded", {
                      value0: String((state.downloaded / 1048576).toFixed(1)),
                    })
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
              <summary>{uiText("Error details")}</summary>
              <p>{state.error || infoError}</p>
            </details>
          )}
        </div>
      </div>
      {state.notes && (
        <section
          className="update-release-notes"
          aria-label={uiText("Release notes")}
        >
          <h4>
            {uiText("What’s new in ")}
            {state.version}
          </h4>
          <div>
            <Suspense fallback={<p role="status">{uiText("Loading…")}</p>}>
              <ReleaseNotesMarkdown>{state.notes}</ReleaseNotesMarkdown>
            </Suspense>
          </div>
        </section>
      )}
      <footer className="update-footer">
        <span>
          <ShieldCheck size={15} />
          {uiText("Signed updates. Always your choice.")}
        </span>
        <a href="https://github.com/MohamedElashri/mdiff/releases">
          {uiText("All releases")}
          <ArrowUpRight size={14} />
        </a>
      </footer>
    </section>
  );
}
