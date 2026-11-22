import {
  uiText,
  formatDate,
} from "../../vendor/diffle/src/client/i18n/runtime";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Bell, X } from "lucide-react";
import {
  Dialog,
  DialogContent,
} from "../../vendor/diffle/src/client/ui/dialog";
import {
  inbox,
  mergeDelta,
  mergeSnapshot,
  type View,
  type Item,
  type Preferences,
  type Delta,
  type Target,
} from "./api";
import { hosting, type Check, type Page } from "../hosting/api";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./attention.css";
const sourceNames: Record<string, string> = {
  requested: uiText("Review requested"),
  checks: uiText("Failed checks"),
  conflicts: uiText("Conflicts"),
  agents: uiText("Agent input"),
};
export default function AttentionHub({
  onClose,
  initialTarget,
}: {
  onClose: () => void;
  initialTarget?: Target | null;
}) {
  const [view, setView] = useState<View | null>(null),
    [preferences, setPreferences] = useState<Preferences | null>(null),
    [error, setError] = useState(""),
    [pending, setPending] = useState(false),
    [history, setHistory] = useState(false);
  const dirty = useRef(false),
    live = useRef(true);
  const queued = useRef<Delta[]>([]);
  const resync = useRef(false);
  const [provider, setProvider] = useState<"github" | "gitlab">("github"),
    [host, setHost] = useState("github.com"),
    [repository, setRepository] = useState("");
  const [checks, setChecks] = useState<Page<Check> | null>(null),
    [checkError, setCheckError] = useState("");
  useEffect(() => {
    setChecks(null);
    setCheckError("");
    if (
      initialTarget?.kind !== "checks" ||
      !initialTarget.review ||
      !initialTarget.head
    )
      return;
    const abort = new AbortController();
    void hosting
      .read<Page<Check>>(
        initialTarget.review.site,
        {
          kind: "checks",
          review: initialTarget.review,
          sha: initialTarget.head,
          page: 1,
        },
        abort.signal,
      )
      .then((v) => {
        if (!abort.signal.aborted) setChecks(v);
      })
      .catch((e) => {
        if (!abort.signal.aborted) setCheckError(String(e));
      });
    return () => abort.abort();
  }, [initialTarget]);
  const accept = (v: View) => {
    if (!live.current) return;
    setView((old) => {
      let next = mergeSnapshot(old, v);
      for (const delta of queued.current) next = mergeDelta(next, delta);
      queued.current = [];
      return next;
    });
    if (!dirty.current)
      setPreferences((old) =>
        old && old.revision > v.preferences.revision ? old : v.preferences,
      );
    if (resync.current) {
      resync.current = false;
      void inbox
        .view()
        .then(accept)
        .catch((e) => live.current && setError(String(e)));
    }
  };
  useEffect(() => {
    live.current = true;
    const load = () => {
      void inbox
        .view()
        .then(accept)
        .catch((e) => live.current && setError(String(e)));
    };
    const changed = listen<Delta>("attention-changed", (e) =>
      setView((v) => {
        if (v) return mergeDelta(v, e.payload);
        queued.current.push(e.payload);
        if (queued.current.length > 32) {
          queued.current.shift();
          resync.current = true;
        }
        return v;
      }),
    );
    const settings = listen("attention-settings", load);
    const status = listen<Pick<View, "warnings" | "error" | "metrics">>(
      "attention-status",
      (e) => setView((v) => (v ? { ...v, ...e.payload } : v)),
    );
    load();
    return () => {
      live.current = false;
      for (const p of [changed, settings, status])
        void p.then((stop) => stop());
    };
  }, []);
  async function act(task: () => Promise<unknown>) {
    if (pending) return;
    setPending(true);
    setError("");
    try {
      await task();
    } catch (e) {
      if (live.current) setError(String(e));
    } finally {
      if (live.current) setPending(false);
    }
  }
  function update(key: keyof Preferences, value: unknown) {
    dirty.current = true;
    setPreferences((p) => (p ? { ...p, [key]: value } : p));
  }
  const enabled = (source: string) =>
    !!view?.preferences.enabled &&
    !!view.preferences[source as keyof Preferences];
  const needs =
    view?.items
      .filter(
        (i) => enabled(i.source) && !i.resolved && !i.dismissed && !i.paused,
      )
      .sort(
        (a, b) => Number(a.read) - Number(b.read) || b.updated - a.updated,
      ) ?? [];
  function row(i: Item) {
    return (
      <li
        key={i.id}
        className={`attention-row ${String(i.read ? "is-read" : "")}`}
      >
        <div className="attention-row-body">
          <span className="attention-source">
            {sourceNames[i.source]}
            {i.stale ? uiText(" · Unconfirmed") : ""}
            {i.resolved ? uiText(" · Resolved") : ""}
            {!enabled(i.source) || i.paused ? uiText(" · Paused") : ""}
          </span>
          <strong>{i.title}</strong>
          <p>{i.detail}</p>
          <time dateTime={new Date(i.updated * 1000).toISOString()}>
            {formatDate(new Date(i.updated * 1000), {
              dateStyle: "medium",
              timeStyle: "short",
            })}
          </time>
          {i.target.head && (
            <code>Observed commit {i.target.head.slice(0, 12)}</code>
          )}
        </div>
        <div className="attention-actions">
          <button
            disabled={pending || !enabled(i.source) || i.paused}
            onClick={() => void act(() => inbox.open(i.id))}
          >
            {i.source === "checks"
              ? uiText("Inspect failed checks")
              : uiText("Open destination")}
          </button>
          {!i.read && (
            <button
              disabled={pending}
              onClick={() =>
                void act(async () => accept(await inbox.update("read", i.id)))
              }
            >
              {uiText("Mark read")}
            </button>
          )}
          <button
            disabled={pending}
            onClick={() =>
              void act(async () =>
                accept(
                  await inbox.update(i.dismissed ? "restore" : "dismiss", i.id),
                ),
              )
            }
          >
            {i.dismissed ? uiText("Restore") : uiText("Dismiss")}
          </button>
        </div>
      </li>
    );
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        className="attention-dialog"
        aria-labelledby="attention-title"
      >
        <header>
          <div>
            <h2 id="attention-title">
              <Bell size={20} />
              {uiText("Needs attention")}
            </h2>
            <p>
              {uiText(
                "Review requests, failures, conflicts, and agents waiting for you.",
              )}
            </p>
          </div>
          <button
            aria-label={uiText("Close needs attention")}
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </header>
        {error && <p role="alert">{error}</p>}
        {initialTarget?.kind === "checks" && (
          <section className="attention-checks">
            <h3>{uiText("Checks at the recorded commit")}</h3>
            <code>{initialTarget.head}</code>
            {checkError ? (
              <p role="alert">{checkError}</p>
            ) : !checks ? (
              <p role="status">{uiText("Loading recorded checks…")}</p>
            ) : (
              <>
                <ul>
                  {checks.items.map((c) => (
                    <li key={c.id}>
                      <strong>{c.name}</strong> · {c.status}
                      {c.url && (
                        <button onClick={() => void openUrl(c.url!)}>
                          {uiText("Open check on provider")}
                        </button>
                      )}
                    </li>
                  ))}
                </ul>
                {checks.next && (
                  <p>
                    {uiText(
                      "More checks exist. Open Hosting reviews for full pagination.",
                    )}
                  </p>
                )}
              </>
            )}
          </section>
        )}
        {!view || !preferences ? (
          <p role="status">{uiText("Loading inbox…")}</p>
        ) : (
          <>
            <div className="attention-toolbar">
              <span>
                {needs.length}
                {uiText(" open · ")}
                {view.unread}
                {uiText(" unread")}
              </span>
              <button
                disabled={pending}
                onClick={() =>
                  void act(async () => accept(await inbox.update("refresh")))
                }
              >
                {uiText("Refresh sources")}
              </button>
              <button
                disabled={pending || !view.unread}
                onClick={() =>
                  void act(async () => accept(await inbox.update("readAll")))
                }
              >
                {uiText("Mark all read")}
              </button>
              <button
                aria-pressed={history}
                onClick={() => setHistory(!history)}
              >
                {history
                  ? uiText("Show attention")
                  : uiText("Activity history")}
              </button>
            </div>
            {view.error && <p role="alert">{view.error}</p>}
            {view.trayError && <p role="status">{view.trayError}</p>}
            {view.warnings.length > 0 && (
              <details>
                <summary>
                  {uiText("Source availability (")}
                  {view.warnings.length})
                </summary>
                {[...new Set(view.warnings)].map((w) => (
                  <p key={w}>{w}</p>
                ))}
              </details>
            )}
            {!view.preferences.enabled && (
              <p className="attention-empty">
                {uiText(
                  "Monitoring is paused. Choose sources below, then enable monitoring. Existing history is kept.",
                )}
              </p>
            )}
            {history ? (
              <ul className="attention-list">
                {[...view.items].sort((a, b) => b.updated - a.updated).map(row)}
              </ul>
            ) : (
              <>
                <ul className="attention-list">{needs.map(row)}</ul>
                {view.preferences.enabled && !needs.length && (
                  <p className="attention-empty">
                    {uiText(
                      "This view has no open items. Check source availability and history for older observations.",
                    )}
                  </p>
                )}
                <h3>{uiText("Running agents")}</h3>
                {view.running.length ? (
                  <ul className="attention-list">
                    {view.running.map((i) => (
                      <li key={i.id} className="attention-row">
                        <div className="attention-row-body">
                          <strong>{i.title}</strong>
                          <p>{i.detail}</p>
                        </div>
                        <button
                          disabled={pending}
                          onClick={() => void act(() => inbox.open(i.id))}
                        >
                          {uiText("Show agent")}
                        </button>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p>
                    {uiText(
                      "No current agent activity reported. Enable agent detection in Agents and review to observe sessions.",
                    )}
                  </p>
                )}
                <h3>{uiText("Workspaces and repositories")}</h3>
                <ul className="attention-repositories">
                  {view.repositories.map((r) => (
                    <li key={r.tab}>
                      <strong>{r.workspaceName}</strong>
                      <span>{r.label}</span>
                      <small>
                        {preferences.localTabs.includes(r.tab)
                          ? uiText("Conflict monitoring selected")
                          : uiText("Not monitored for conflicts")}
                        {r.active ? uiText(" · Current review") : ""}
                      </small>
                    </li>
                  ))}
                </ul>
              </>
            )}
            <details className="attention-preferences">
              <summary>{uiText("Monitoring and notifications")}</summary>
              <p>
                {uiText(
                  "One monitor is shared by all windows. Only selected local tabs and configured remote repositories are polled. Paused sources keep their history.",
                )}
              </p>
              <label>
                <input
                  type="checkbox"
                  checked={preferences.enabled}
                  onChange={(e) => update("enabled", e.target.checked)}
                />
                {uiText("Enable monitoring")}
              </label>
              <div className="attention-options">
                {(["requested", "checks", "conflicts", "agents"] as const).map(
                  (key) => (
                    <label key={key}>
                      <input
                        type="checkbox"
                        checked={preferences[key]}
                        onChange={(e) => update(key, e.target.checked)}
                      />
                      {sourceNames[key]}
                    </label>
                  ),
                )}
              </div>
              <h4>{uiText("Local conflict monitoring")}</h4>
              {view.repositories
                .filter((r) => r.root)
                .map((r) => (
                  <label key={r.tab}>
                    <input
                      type="checkbox"
                      checked={preferences.localTabs.includes(r.tab)}
                      onChange={(e) =>
                        update(
                          "localTabs",
                          e.target.checked
                            ? [...preferences.localTabs, r.tab]
                            : preferences.localTabs.filter(
                                (id) => id !== r.tab,
                              ),
                        )
                      }
                    />
                    {r.workspaceName}: {r.label}
                  </label>
                ))}
              <h4>{uiText("Remote repositories")}</h4>
              <p>
                {uiText(
                  "Connections supplies host-bound credentials. Authentication discovery runs once per host after refresh or connection changes, never on each poll.",
                )}
              </p>
              {preferences.repositories.map((r, n) => (
                <div
                  className="attention-remote"
                  key={`${r.site.provider}:${r.site.host}:${r.repository}`}
                >
                  <span>
                    {r.site.provider} · {r.site.host}/{r.repository}
                  </span>
                  <button
                    onClick={() =>
                      update(
                        "repositories",
                        preferences.repositories.filter((_, i) => i !== n),
                      )
                    }
                  >
                    {uiText("Remove")}
                  </button>
                </div>
              ))}
              <div className="attention-add">
                <label>
                  {uiText("Provider")}
                  <select
                    value={provider}
                    onChange={(e) => {
                      const v = e.target.value as "github" | "gitlab";
                      setProvider(v);
                      setHost(v === "github" ? "github.com" : "gitlab.com");
                    }}
                  >
                    <option value="github">{uiText("GitHub")}</option>
                    <option value="gitlab">{uiText("GitLab")}</option>
                  </select>
                </label>
                <label>
                  {uiText("Host")}
                  <input
                    value={host}
                    onChange={(e) => setHost(e.target.value)}
                  />
                </label>
                <label>
                  {uiText("Repository")}
                  <input
                    placeholder={uiText("owner/repository")}
                    value={repository}
                    onChange={(e) => setRepository(e.target.value)}
                  />
                </label>
                <button
                  onClick={() => {
                    if (!repository.trim()) return;
                    update("repositories", [
                      ...preferences.repositories,
                      {
                        site: { provider, host: host.trim() },
                        repository: repository.trim(),
                      },
                    ]);
                    setRepository("");
                  }}
                >
                  {uiText("Add repository")}
                </button>
              </div>
              <h4>{uiText("Notifications")}</h4>
              <label>
                <input
                  type="checkbox"
                  checked={preferences.notifications}
                  onChange={(e) => update("notifications", e.target.checked)}
                />
                {uiText("Send optional OS notifications")}
              </label>
              <p>
                {uiText(
                  "Alerts contain generic text. Open an alert to inspect its recorded destination. OS permission settings and notification service availability still apply.",
                )}
              </p>
              <div className="attention-options">
                {(
                  [
                    "notifyRequested",
                    "notifyChecks",
                    "notifyConflicts",
                    "notifyAgents",
                  ] as const
                ).map((key, n) => (
                  <label key={key}>
                    <input
                      type="checkbox"
                      checked={preferences[key]}
                      onChange={(e) => update(key, e.target.checked)}
                    />
                    {
                      [
                        "Review requests",
                        "Failed checks",
                        "Conflicts",
                        "Agent input",
                      ][n]
                    }
                  </label>
                ))}
              </div>
              <label>
                <input
                  type="checkbox"
                  checked={preferences.tray}
                  onChange={(e) => {
                    dirty.current = true;
                    setPreferences({
                      ...preferences,
                      tray: e.target.checked,
                      keepInTray: e.target.checked && preferences.keepInTray,
                    });
                  }}
                />
                {uiText("Show menu bar / tray entry")}
              </label>
              <label>
                <input
                  type="checkbox"
                  disabled={!preferences.tray}
                  checked={preferences.keepInTray}
                  onChange={(e) => update("keepInTray", e.target.checked)}
                />
                {uiText("Keep windows in the menu bar / tray when closed")}
              </label>
              <p>
                {uiText(
                  "When enabled, closing hides the window and keeps its reviews, terminals and monitoring running. Quit always saves and stops the app through the normal confirmation flow. Without a working tray, closing uses the normal close flow.",
                )}
              </p>
              <div className="attention-toolbar">
                <button
                  disabled={pending}
                  onClick={() =>
                    void act(async () => {
                      const result = await inbox.save(preferences);
                      dirty.current = false;
                      accept(result);
                    })
                  }
                >
                  {uiText("Save inbox preferences")}
                </button>
                <button
                  disabled={pending}
                  onClick={() =>
                    void act(async () => {
                      dirty.current = false;
                      accept(await inbox.view());
                    })
                  }
                >
                  {uiText("Reload inbox preferences")}
                </button>
              </div>
              <p aria-label={uiText("Monitoring usage this run")}>
                {uiText("Activity this run: ")}
                {view.metrics.localReads}
                {uiText(" local checks ·")} {view.metrics.remoteJobs}
                {uiText(" hosting refreshes ·")} {view.metrics.notifications}
                {uiText(" alert attempts")}
              </p>
              <small>
                {uiText(
                  "At most one local read and one remote summary job per 15-second cycle; each repository backs off between reads. History keeps up to 200 items. Notifications are rate limited and capped per app run.",
                )}
              </small>
            </details>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
