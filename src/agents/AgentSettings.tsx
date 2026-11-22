import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, FolderOpen } from "lucide-react";
import {
  uiText,
  formatDate,
} from "../../vendor/diffle/src/client/i18n/runtime";
import { agentApi, type Preferences, type Tool, type View } from "./api";
export type AgentAction = (
  label: string,
  task: () => Promise<unknown>,
) => Promise<void>;
export function AgentSettings({
  view,
  busy,
  act,
  accept,
  reload,
  notify,
  onDirty,
}: {
  view: View;
  busy: string;
  act: AgentAction;
  accept: (view: View) => void;
  reload: () => Promise<void>;
  notify: (message: string) => void;
  onDirty: (dirty: boolean) => void;
}) {
  const [draft, setDraft] = useState(view.preferences);
  const [dirty, setDirty] = useState(false);
  const [checked, setChecked] = useState<Tool[] | null>(null);
  const edit = useRef(0);
  useEffect(() => onDirty(dirty), [dirty, onDirty]);
  useEffect(() => {
    if (
      dirty &&
      (["enabled", "hooks", "codex", "claude", "focus"] as const).every(
        (key) => draft[key] === view.preferences[key],
      )
    )
      setDirty(false);
  }, [draft, view.preferences, dirty]);
  useEffect(() => {
    if (!dirty) {
      setDraft(view.preferences);
      setChecked(null);
    }
  }, [view.preferences.revision, dirty]);
  function change(patch: Partial<Preferences>) {
    edit.current++;
    const next = { ...draft, ...patch };
    setDraft(next);
    setDirty(
      (["enabled", "hooks", "codex", "claude", "focus"] as const).some(
        (key) => next[key] !== view.preferences[key],
      ),
    );
    notify("");
    setChecked(null);
  }
  const tools = checked ?? view.tools;
  return (
    <section
      className="agent-settings"
      aria-label={uiText("Agent preferences")}
    >
      <div className="agent-section-heading">
        <h3>{uiText("Detection and reports")}</h3>
        {dirty && (
          <span className="agent-unsaved">{uiText("Unsaved changes")}</span>
        )}
      </div>
      <div className="agent-save-bar">
        <button
          className="agent-primary"
          disabled={!!busy || !dirty}
          onClick={() =>
            void act("save", async () => {
              const value = await agentApi.save(draft);
              edit.current++;
              setDraft(value.preferences);
              setDirty(false);
              setChecked(null);
              accept(value);
              notify(uiText("Agent preferences saved."));
            })
          }
        >
          {busy === "save" ? uiText("Saving…") : uiText("Save changes")}
        </button>
        <button
          disabled={!!busy || !dirty}
          onClick={() => {
            edit.current++;
            setDraft(view.preferences);
            setDirty(false);
            setChecked(null);
            notify(uiText("Unsaved changes discarded."));
          }}
        >
          {uiText("Discard changes")}
        </button>
        <button
          disabled={!!busy}
          onClick={() =>
            void act("reload", async () => {
              await reload();
              setDirty(false);
              setChecked(null);
              notify(uiText("Agent preferences reloaded."));
            })
          }
        >
          {busy === "reload"
            ? uiText("Reloading…")
            : dirty
              ? uiText("Discard and reload")
              : uiText("Reload saved preferences")}
        </button>
        {dirty && draft.revision !== view.preferences.revision && (
          <p role="alert" className="agent-warning">
            {uiText(
              "Preferences changed in another window. Discard and reload to use the saved values.",
            )}
          </p>
        )}
      </div>
      <label className="agent-toggle">
        <input
          type="checkbox"
          disabled={!!busy}
          checked={draft.enabled}
          onChange={(e) => change({ enabled: e.target.checked })}
        />
        {uiText("Detect agents in the selected repository")}
      </label>
      <p>
        {uiText(
          "Observe process metadata in this repository and its worktrees. Detection never reads conversations, command lines or environments.",
        )}
      </p>
      <label className="agent-toggle">
        <input
          type="checkbox"
          disabled={!!busy}
          checked={draft.hooks}
          onChange={(e) => change({ hooks: e.target.checked })}
        />
        {uiText("Accept local state hooks")}
      </label>
      <p>
        {uiText(
          "Optional hooks report working, waiting and completed states. Detection must also be enabled. Existing sessions need their own hook setup.",
        )}
      </p>
      {view.hookError && (
        <p role="alert" className="agent-error">
          {view.hookError}
        </p>
      )}
      <h3>{uiText("Installed tools")}</h3>
      <p>
        {uiText(
          "Use automatic discovery or choose the CLI executable. Checking a path does not run the tool.",
        )}
      </p>
      {(["codex", "claude"] as const).map((id) => {
        const tool = tools.find((t) => t.id === id);
        const changed = draft[id] !== view.preferences[id] && !checked;
        return (
          <div className="agent-tool" key={id}>
            <div className="agent-section-heading">
              <strong>
                {id === "codex" ? uiText("Codex") : uiText("Claude Code")}
              </strong>
              <span
                className="agent-tool-status"
                data-ready={!changed && !tool?.error && !!tool?.path}
              >
                {changed ? (
                  uiText("Not checked")
                ) : tool?.error || !tool?.path ? (
                  uiText("Unavailable")
                ) : (
                  <>
                    <Check size={13} aria-hidden="true" />
                    {uiText("Available")}
                  </>
                )}
              </span>
            </div>
            <label className="agent-tool-path">
              {id === "codex"
                ? uiText("Codex executable")
                : uiText("Claude Code executable")}
              <input
                disabled={!!busy}
                value={draft[id] ?? ""}
                placeholder={uiText("Automatic discovery")}
                spellCheck={false}
                onChange={(e) => change({ [id]: e.target.value || null })}
              />
            </label>
            <div className="agent-actions">
              <button
                disabled={!!busy}
                onClick={() =>
                  void act("browse", async () => {
                    const ticket = edit.current;
                    const path = await open({
                      directory: false,
                      multiple: false,
                      title: uiText("Choose agent executable"),
                    });
                    if (typeof path === "string" && ticket === edit.current)
                      change({ [id]: path });
                  })
                }
              >
                <FolderOpen size={14} aria-hidden="true" />
                {uiText("Browse…")}
              </button>
              <button
                disabled={!!busy || !draft[id]}
                onClick={() => change({ [id]: null })}
              >
                {uiText("Use automatic discovery")}
              </button>
            </div>
            {!changed &&
              (tool?.error ? (
                <p className="agent-error">{tool.error}</p>
              ) : (
                <p className="agent-paths">{tool?.path}</p>
              ))}
          </div>
        );
      })}
      <button
        disabled={!!busy}
        onClick={() =>
          void act("tools", async () => {
            const ticket = edit.current;
            const result = await agentApi.tools(draft);
            if (ticket === edit.current) {
              setChecked(result);
              notify(uiText("Tool check complete."));
            }
          })
        }
      >
        {busy === "tools"
          ? uiText("Checking tools…")
          : uiText("Check installed tools")}
      </button>
      <label className="agent-focus">
        {uiText("Additional review focus")}
        <textarea
          disabled={!!busy}
          value={draft.focus}
          maxLength={4000}
          onChange={(e) => change({ focus: e.target.value })}
        />
      </label>

      <details className="agent-hook-setup">
        <summary>{uiText("Set up local state hooks")}</summary>
        <p>
          {uiText(
            "Generate the configuration, enable detection and local state hooks, then save. New mdiff review launches use it automatically; merge it into your CLI settings for existing sessions using the CLI’s normal trust and reload flow.",
          )}
        </p>
        <div className="agent-actions">
          <button
            disabled={!!busy}
            onClick={() =>
              void act("hooks", async () => {
                accept(await agentApi.hooks(true));
                notify(
                  uiText(
                    "Hook configuration generated. Existing agent settings were not changed.",
                  ),
                );
              })
            }
          >
            {busy === "hooks"
              ? uiText("Generating…")
              : uiText("Generate hook bundle")}
          </button>
          <button
            disabled={!!busy || !view.hooks}
            onClick={() =>
              void act("uninstall", async () => {
                const value = await agentApi.hooks(false);
                accept(value);
                setDraft((p) => ({
                  ...p,
                  hooks: false,
                  revision: value.preferences.revision,
                }));
                notify(
                  uiText(
                    "Hooks disabled. Remove manually added references from your CLI settings; the helper remains inert for old sessions.",
                  ),
                );
              })
            }
          >
            {uiText("Uninstall hook bundle")}
          </button>
        </div>
        {view.hooks && (
          <>
            <p>
              {!view.preferences.enabled || !view.preferences.hooks
                ? uiText("Local hook reports are disabled.")
                : view.hookReady
                  ? uiText("Local hook listener is ready.")
                  : uiText(
                      "Local hook listener is not ready. Restart mdiff if this persists.",
                    )}
            </p>
            <p>
              {view.lastHook
                ? uiText("Last accepted report at {time}", {
                    time: formatDate(new Date(view.lastHook * 1000), {
                      timeStyle: "medium",
                    }),
                  })
                : uiText(
                    "No hook reports received yet. Existing CLI sessions need the generated configuration.",
                  )}
            </p>
            <div className="agent-actions">
              {(["codex", "claude"] as const).map((tool) => (
                <button
                  disabled={!!busy}
                  key={tool}
                  onClick={() =>
                    void act("copy", async () => {
                      await writeText(await agentApi.hookConfig(tool));
                      notify(uiText("Hook configuration copied."));
                    })
                  }
                >
                  {tool === "codex"
                    ? uiText("Copy Codex configuration")
                    : uiText("Copy Claude configuration")}
                </button>
              ))}
            </div>
            <p className="agent-paths">{view.hooks.helper}</p>
          </>
        )}
      </details>
    </section>
  );
}
