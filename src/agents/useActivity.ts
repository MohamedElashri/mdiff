import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { agentApi, type Activity, type View } from "./api";
/** One observer per desktop window, independent of feature dialogs. */
export function useAgentActivity(
  context: string | null,
  ready: boolean,
  onActivity: (rows: Activity[]) => void,
) {
  const latest = useRef(onActivity);
  latest.current = onActivity;
  useEffect(() => {
    if (!ready) return;
    let live = true,
      registered = false,
      pending = false,
      queued = false,
      version = -1;
    const observer = crypto.randomUUID();
    latest.current([]);
    const accept = (view: View) => {
      if (!live || view.version < version) return;
      version = view.version;
      latest.current(
        view.preferences.enabled
          ? view.activities.filter((a) => a.context === context)
          : [],
      );
    };
    const refresh = async () => {
      if (pending) {
        queued = true;
        return;
      }
      pending = true;
      try {
        do {
          queued = false;
          let view = await agentApi.settings();
          if (!live) return;
          if (view.preferences.enabled && context) {
            if (!registered) {
              view = await agentApi.view(context, observer);
              if (!live) return;
              registered = true;
            }
            accept(view);
          } else {
            if (registered) {
              await agentApi.unwatch(context, observer);
              registered = false;
            }
            if (live) latest.current([]);
          }
        } while (queued && live);
      } catch {
        // The panel/Preferences expose native diagnostics; never strand stale badges.
        if (live) latest.current([]);
      } finally {
        pending = false;
      }
    };
    const listener = listen("agent-activity-changed", () => {
      void refresh();
    });
    void listener
      .then(() => {
        if (live) void refresh();
      })
      .catch(() => {
        if (live) void refresh();
      });
    return () => {
      live = false;
      void listener.then((stop) => stop()).catch(() => {});
      void agentApi.unwatch(context, observer).catch(() => {});
    };
  }, [context, ready]);
}
