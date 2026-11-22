import { Bot, RefreshCw, Search } from "lucide-react";
import {
  uiText,
  formatDate,
} from "../../vendor/diffle/src/client/i18n/runtime";
import type { Activity, View } from "./api";
export const isCurrentActivity = (row: Activity) =>
  !["unavailable", "ended"].includes(row.state);
const name = (tool: string) =>
  tool === "claude" ? "Claude Code" : tool === "codex" ? "Codex" : tool;
function state(row: Activity) {
  switch (row.state) {
    case "working":
      return uiText("Working");
    case "waiting":
      return uiText("Needs input");
    case "turn completed":
      return uiText("Turn complete");
    case "started":
      return uiText("Session started");
    case "launched":
      return uiText("Launched");
    case "ended":
      return uiText("Ended");
    case "unavailable":
      return uiText("No longer observable");
    default:
      return uiText("State unknown");
  }
}
export function AgentActivity({
  view,
  context,
  loading,
  busy,
  onScan,
  onEnable,
  onSettings,
  onTerminal,
}: {
  view: View | null;
  context: string | null;
  loading: boolean;
  busy: string;
  onScan: () => void;
  onEnable: () => void;
  onSettings: () => void;
  onTerminal: (id: string) => void;
}) {
  const scanning = !!view?.scan.running || busy === "scan";
  const current = view?.activities.filter(isCurrentActivity) ?? [];
  const recent =
    view?.activities.filter((row) => !isCurrentActivity(row)) ?? [];
  function rows(activities: Activity[]) {
    return (
      <ul className="agent-activity">
        {activities.map((row) => (
          <li key={row.id}>
            <Bot size={17} aria-hidden="true" />
            <div>
              <div className="agent-row-heading">
                <strong>
                  {name(row.tool)}
                  {row.parent ? uiText(" subagent") : ""}
                </strong>
                <span className="agent-state" data-state={row.state}>
                  {state(row)}
                </span>
              </div>
              <p className="agent-row-path" title={row.root}>
                {row.root}
              </p>
              <small>
                {row.provenance === "detected"
                  ? uiText("Process detected; state is unknown")
                  : row.provenance === "hook"
                    ? uiText("Local hook report")
                    : uiText("Explicit mdiff launch")}
                {" · "}
                {uiText("PID {pid}", { pid: row.pid })}
                {row.commit ? ` · ${row.commit.slice(0, 12)}` : ""}
              </small>
            </div>
            {row.terminal && isCurrentActivity(row) && (
              <button onClick={() => onTerminal(row.terminal!)}>
                {uiText("Show terminal")}
              </button>
            )}
          </li>
        ))}
      </ul>
    );
  }
  return (
    <section
      className="agent-activity-view"
      aria-label={uiText("Repository agent activity")}
      aria-busy={loading || scanning}
    >
      <div className="agent-section-heading">
        <h3>{uiText("Activity in this repository")}</h3>
        <button
          disabled={
            !!busy ||
            loading ||
            scanning ||
            !context ||
            !view?.preferences.enabled
          }
          onClick={onScan}
        >
          <RefreshCw
            size={14}
            className={scanning ? "agent-spinning" : ""}
            aria-hidden="true"
          />
          {scanning ? uiText("Scanning…") : uiText("Scan now")}
        </button>
      </div>
      {loading ? (
        <div className="agent-empty" role="status">
          {uiText("Loading agent activity…")}
        </div>
      ) : !view ? (
        <div className="agent-empty">
          <p>{uiText("Agent activity could not be loaded.")}</p>
          <button onClick={onSettings}>{uiText("Agent preferences")}</button>
        </div>
      ) : !view.preferences.enabled ? (
        <div className="agent-empty">
          <Search size={22} aria-hidden="true" />
          <strong>{uiText("Detection is off")}</strong>
          <p>
            {uiText(
              "Look for Codex and Claude Code in the selected repository and its worktrees. No conversations or command lines are read.",
            )}
          </p>
          <button disabled={!!busy} onClick={onEnable}>
            {uiText("Enable detection")}
          </button>
        </div>
      ) : !context ? (
        <div className="agent-empty">
          <strong>{uiText("Select a repository to scan")}</strong>
          <p>
            {uiText(
              "Open a repository review or History to scan. Check executable paths in Preferences → Agents.",
            )}
          </p>
          <button onClick={onSettings}>
            {uiText("Check installed tools")}
          </button>
        </div>
      ) : (
        <>
          <div className="agent-scan-status" role="status" aria-live="polite">
            <span className="agent-status-dot" data-scanning={scanning} />
            {scanning
              ? uiText("Scanning local processes…")
              : view.scan.completed
                ? uiText("Last scan at {time}", {
                    time: formatDate(new Date(view.scan.completed * 1000), {
                      timeStyle: "medium",
                    }),
                  })
                : uiText("Waiting for the first scan…")}
          </div>
          {view.scan.error && (
            <p role="alert" className="agent-error">
              {view.scan.error}
            </p>
          )}
          {view.scan.completed &&
            view.scan.readable === 0 &&
            view.scan.total > 0 && (
              <p className="agent-warning">
                {uiText(
                  "No process metadata was accessible. Check OS permissions and try Scan now.",
                )}
              </p>
            )}
          {current.length ? (
            rows(current)
          ) : (
            <div className="agent-empty">
              <Bot size={22} aria-hidden="true" />
              <strong>
                {scanning
                  ? uiText("Looking for agents…")
                  : uiText("No current agents found")}
              </strong>
              <p>
                {uiText(
                  "Start a supported CLI in this repository, then scan again. Script wrappers or restricted process access may prevent detection.",
                )}
              </p>
              <button onClick={onSettings}>
                {uiText("Check installed tools")}
              </button>
            </div>
          )}
          <details className="agent-scan-details">
            <summary>{uiText("Scan details")}</summary>
            <p>
              {uiText(
                "Checked {checked} of {total} processes; {readable} readable.",
                {
                  checked: view.scan.checked,
                  total: view.scan.total,
                  readable: view.scan.readable,
                },
              )}
            </p>
            {view.scan.limited && (
              <p>
                {uiText(
                  "This pass reached its scan budget. Automatic scans continue through remaining processes; Scan now uses a longer budget.",
                )}
              </p>
            )}
            <p>
              {uiText(
                "Protected or exited processes may be unreadable. Process detection alone does not establish task state.",
              )}
            </p>
            <ul className="agent-paths">
              {view.roots.map((root) => (
                <li key={root}>{root}</li>
              ))}
            </ul>
          </details>
          {recent.length > 0 && (
            <details className="agent-recent">
              <summary>
                {uiText("Recent activity ({count})", { count: recent.length })}
              </summary>
              {rows(recent)}
            </details>
          )}
        </>
      )}
    </section>
  );
}
