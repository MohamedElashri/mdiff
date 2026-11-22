import { workspaceMessages } from "./messages";
import { uiText, translate } from "../../vendor/diffle/src/client/i18n/runtime";
import { useState, type CSSProperties } from "react";
import {
  Plus,
  X,
  ChevronDown,
  MoreHorizontal,
  Folder,
  RotateCcw,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
} from "../../vendor/diffle/src/client/ui/dialog";
import type { Document, Operation, Tab, WorkspaceWindow } from "./types";
export function WorkspaceBar({
  document,
  windows,
  onWindow,
  onFocusWindow,
  active,
  busy,
  onOperation,
  onActivate,
  onNew,
  onRefresh,
  onUnload,
  onDrafts,
  agentStatus = {},
}: {
  agentStatus?: Record<string, string>;
  document: Document;
  windows: WorkspaceWindow[];
  onWindow: (
    action: "open" | "duplicate" | "move",
    tab?: string,
    target?: string,
  ) => Promise<void>;
  onFocusWindow: (target: string) => void;
  active: string | null;
  busy: boolean;
  onOperation: (operation: Operation) => Promise<void>;
  onActivate: (tab: string) => void;
  onNew: () => void;
  onRefresh: (tab: string) => void;
  onUnload: (tab: string) => void;
  onDrafts: (tab: string) => void;
}) {
  const workspace = document.workspaces.find(
    (w) => w.id === document.activeWorkspace,
  )!;
  const [manage, setManage] = useState(false),
    [tabOptions, setTabOptions] = useState<Tab | null>(null);
  const [name, setName] = useState(""),
    [groupName, setGroupName] = useState(""),
    [color, setColor] = useState("blue");
  const [dragged, setDragged] = useState<string | null>(null);
  const [failure, setFailure] = useState("");
  const run = async (op: Operation) => {
    setFailure("");
    try {
      await onOperation(op);
      return true;
    } catch (e) {
      setFailure(String(e));
      return false;
    }
  };
  const visibleTabs = [
    ...workspace.tabs.filter((t) => !t.group),
    ...workspace.groups.flatMap((g) =>
      g.collapsed ? [] : workspace.tabs.filter((t) => t.group === g.id),
    ),
  ];
  const focusTab =
    visibleTabs.find((t) => t.id === active)?.id ?? visibleTabs[0]?.id;
  const renderTab = (tab: Tab) => (
    <div
      key={tab.id}
      className={`workspace-tab ${String(active === tab.id ? "is-active" : "")}`}
      draggable={!busy}
      onDragStart={(e) => {
        setDragged(tab.id);
        e.dataTransfer.setData("text/plain", tab.id);
        e.dataTransfer.effectAllowed = "move";
      }}
      onDragEnd={() => setDragged(null)}
      onDragOver={(e) => {
        if (dragged) e.preventDefault();
      }}
      onDrop={(e) => {
        e.preventDefault();
        if (dragged) void run({ kind: "moveTab", id: dragged, before: tab.id });
        setDragged(null);
      }}
    >
      <button
        role="tab"
        aria-selected={active === tab.id}
        id={`tab-${String(tab.id)}`}
        aria-controls="workspace-review-panel"
        tabIndex={tab.id === focusTab ? 0 : -1}
        title={`${tab.source.label}\n${tab.source.path}${tab.drafts ? "\nUnsent drafts are kept when closed" : ""}`}
        disabled={busy}
        onClick={() => onActivate(tab.id)}
        onKeyDown={(e) => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key))
            return;
          e.preventDefault();
          const visible = visibleTabs;
          const index = visible.findIndex((t) => t.id === tab.id);
          const next =
            e.key === "Home"
              ? 0
              : e.key === "End"
                ? visible.length - 1
                : (index + (e.key === "ArrowRight" ? 1 : -1) + visible.length) %
                  visible.length;
          const target = visible[next];
          if (target) {
            globalThis.document.getElementById(`tab-${target.id}`)?.focus();
            onActivate(target.id);
          }
        }}
      >
        <Folder size={13} />
        <span>
          {tab.source.path.split("/").filter(Boolean).pop() || tab.source.label}
        </span>
        <small>{tab.source.label}</small>
        {agentStatus[tab.id] && (
          <span className="agent-badge" title={agentStatus[tab.id]}>
            {uiText("Agent: ")}
            {agentStatus[tab.id]}
          </span>
        )}
        {tab.drafts > 0 && (
          <span
            className="tab-draft"
            aria-label={translate(workspaceMessages.unsentDrafts, {
              count: tab.drafts,
            })}
          >
            •
          </span>
        )}
      </button>
      <button
        className="tab-options"
        aria-label={uiText("Options for {value0}", {
          value0: String(tab.source.label),
        })}
        disabled={busy}
        onClick={() => {
          setFailure("");
          setName(tab.source.label);
          setTabOptions(tab);
        }}
      >
        <MoreHorizontal size={13} />
      </button>
      <button
        className="tab-close"
        aria-label={uiText("Close {value0}", {
          value0: String(tab.source.label),
        })}
        title={uiText("Close tab; drafts are kept")}
        disabled={busy}
        onClick={() => void run({ kind: "closeTab", id: tab.id })}
      >
        <X size={13} />
      </button>
    </div>
  );
  return (
    <>
      <nav
        className="workspace-bar"
        aria-label={uiText("Workspaces and reviews")}
        aria-busy={busy}
      >
        <div className="workspace-selector">
          <select
            aria-label={uiText("Workspace")}
            value={workspace.id}
            disabled={busy}
            onChange={(e) => {
              const existing = windows.find(
                (w) => !w.current && w.workspace === e.target.value,
              );
              if (existing) onFocusWindow(existing.label);
              else void run({ kind: "selectWorkspace", id: e.target.value });
            }}
          >
            {document.workspaces.map((w) => (
              <option key={w.id} value={w.id}>
                {w.name}
                {windows.some(
                  (other) => !other.current && other.workspace === w.id,
                )
                  ? uiText(" (other window)")
                  : ""}
              </option>
            ))}
          </select>
          <button
            aria-label={uiText("Manage workspaces")}
            title={uiText("Manage workspaces and groups")}
            disabled={busy}
            onClick={() => {
              setFailure("");
              setName(workspace.name);
              setManage(true);
            }}
          >
            <MoreHorizontal size={16} />
          </button>
        </div>
        <div
          className="workspace-tabs"
          role={visibleTabs.length ? "tablist" : undefined}
          aria-label={uiText("Review tabs")}
        >
          {workspace.tabs.filter((t) => !t.group).map(renderTab)}
          {workspace.groups.map((group) => (
            <div
              className="tab-group"
              key={group.id}
              style={
                {
                  "--group-color": `var(--workspace-${group.color})`,
                } as CSSProperties
              }
            >
              <button
                className="group-toggle"
                aria-label={`${group.collapsed ? "Expand" : "Collapse"} ${group.name}`}
                aria-expanded={!group.collapsed}
                disabled={busy}
                onClick={() =>
                  void run({
                    kind: "editGroup",
                    id: group.id,
                    collapsed: !group.collapsed,
                  })
                }
              >
                <ChevronDown size={12} />
                {group.name}
                <small>
                  {workspace.tabs.filter((t) => t.group === group.id).length}
                </small>
              </button>
              {!group.collapsed &&
                workspace.tabs
                  .filter((t) => t.group === group.id)
                  .map(renderTab)}
            </div>
          ))}
        </div>
        <button
          aria-label={uiText("New review tab")}
          title={uiText("New review tab")}
          disabled={busy}
          onClick={onNew}
        >
          <Plus size={16} />
        </button>
        <button
          aria-label={uiText("Reopen closed tab")}
          title={uiText("Reopen closed tab")}
          disabled={busy || !document.closed.length}
          onClick={() => void run({ kind: "reopenTab" })}
        >
          <RotateCcw size={15} />
        </button>
      </nav>
      {failure && !manage && !tabOptions && (
        <div role="alert" className="workspace-error">
          {failure}
        </div>
      )}
      {manage && (
        <Dialog open onOpenChange={setManage}>
          <DialogContent
            className="workspace-dialog"
            aria-labelledby="manage-workspaces-title"
          >
            <h2 id="manage-workspaces-title">{uiText("Workspaces")}</h2>
            <p>
              {uiText(
                "Keep review tabs and groups together. Saved review data belongs to its repository.",
              )}
            </p>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void run({ kind: "renameWorkspace", id: workspace.id, name });
              }}
            >
              <label>
                {uiText("Workspace name")}
                <input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  maxLength={100}
                />
              </label>
              <button type="submit">{uiText("Rename workspace")}</button>
              <button
                type="button"
                onClick={() => {
                  void run({
                    kind: "createWorkspace",
                    name: name || "New workspace",
                  }).then((ok) => {
                    if (ok) setManage(false);
                  });
                }}
              >
                {uiText("Create workspace")}
              </button>
            </form>
            <div className="tab-dialog-actions">
              <button
                disabled={busy || windows.length >= 2}
                onClick={() => {
                  void onWindow("open");
                  setManage(false);
                }}
              >
                {uiText("Move workspace to new window")}
              </button>
              {windows
                .filter((w) => !w.current)
                .map((other) => (
                  <button
                    key={other.label}
                    onClick={() => onFocusWindow(other.label)}
                  >
                    {uiText("Focus")}{" "}
                    {document.workspaces.find((w) => w.id === other.workspace)
                      ?.name ?? uiText("other workspace")}
                  </button>
                ))}
            </div>
            <p>
              {uiText(
                "Up to two workspace windows can be open. Each workspace belongs to one window.",
              )}
            </p>
            <h3>{uiText("Tab groups")}</h3>
            {workspace.groups.map((g) => (
              <div className="group-editor" key={g.id}>
                <input
                  aria-label={uiText("Name for {value0}", {
                    value0: String(g.name),
                  })}
                  defaultValue={g.name}
                  maxLength={100}
                  onBlur={(e) => {
                    if (e.target.value !== g.name)
                      void run({
                        kind: "editGroup",
                        id: g.id,
                        name: e.target.value,
                      });
                  }}
                />
                <select
                  aria-label={uiText("Color for {value0}", {
                    value0: String(g.name),
                  })}
                  value={g.color}
                  onChange={(e) =>
                    void run({
                      kind: "editGroup",
                      id: g.id,
                      color: e.target.value,
                    })
                  }
                >
                  {["blue", "green", "amber", "purple", "rose", "gray"].map(
                    (c) => (
                      <option key={c}>{c}</option>
                    ),
                  )}
                </select>
                <button
                  onClick={() => void run({ kind: "deleteGroup", id: g.id })}
                >
                  {uiText("Ungroup tabs")}
                </button>
              </div>
            ))}
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void run({
                  kind: "createGroup",
                  workspace: workspace.id,
                  name: groupName,
                  color,
                }).then((ok) => {
                  if (ok) setGroupName("");
                });
              }}
            >
              <label>
                {uiText("New group")}
                <input
                  value={groupName}
                  onChange={(e) => setGroupName(e.target.value)}
                  maxLength={100}
                />
              </label>
              <select
                aria-label={uiText("New group color")}
                value={color}
                onChange={(e) => setColor(e.target.value)}
              >
                {["blue", "green", "amber", "purple", "rose", "gray"].map(
                  (c) => (
                    <option key={c}>{c}</option>
                  ),
                )}
              </select>
              <button disabled={!groupName.trim()}>
                {uiText("Add group")}
              </button>
            </form>
            <details>
              <summary>{uiText("Delete this workspace")}</summary>
              <p>
                {uiText(
                  "Removes its tabs and groups. Repository files, saved comments and downloaded review archives remain.",
                )}
              </p>
              <button
                disabled={document.workspaces.length < 2}
                onClick={() =>
                  void run({ kind: "deleteWorkspace", id: workspace.id }).then(
                    (ok) => {
                      if (ok) setManage(false);
                    },
                  )
                }
              >
                {uiText("Delete workspace")}
              </button>
            </details>
            {failure && <p role="alert">{failure}</p>}
            <button onClick={() => setManage(false)}>{uiText("Done")}</button>
          </DialogContent>
        </Dialog>
      )}
      {tabOptions && (
        <Dialog
          open
          onOpenChange={(open) => {
            if (!open) setTabOptions(null);
          }}
        >
          <DialogContent
            className="workspace-dialog"
            aria-labelledby="review-tab-title"
          >
            <h2 id="review-tab-title">{uiText("Review tab")}</h2>
            <p className="tab-source">{tabOptions.source.path}</p>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void run({
                  kind: "renameTab",
                  id: tabOptions.id,
                  label: name,
                }).then((ok) => {
                  if (ok) setTabOptions(null);
                });
              }}
            >
              <label>
                {uiText("Tab label")}
                <input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  maxLength={100}
                />
              </label>
              <button>{uiText("Rename tab")}</button>
            </form>
            <label>
              {uiText("Group")}
              <select
                value={
                  workspace.tabs.find((t) => t.id === tabOptions.id)?.group ??
                  ""
                }
                onChange={(e) =>
                  void run({
                    kind: "setGroup",
                    id: tabOptions.id,
                    group: e.target.value || null,
                  })
                }
              >
                <option value="">{uiText("Ungrouped")}</option>
                {workspace.groups.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              {uiText("Move to workspace")}
              <select
                value={workspace.id}
                onChange={(e) =>
                  void run({
                    kind: "moveWorkspace",
                    id: tabOptions.id,
                    workspace: e.target.value,
                  }).then((ok) => {
                    if (ok) setTabOptions(null);
                  })
                }
              >
                {document.workspaces.map((w) => (
                  <option key={w.id} value={w.id}>
                    {w.name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              {uiText("Revisions")}
              <select
                value={
                  workspace.tabs.find((t) => t.id === tabOptions.id)?.policy ??
                  "follow"
                }
                onChange={(e) =>
                  void run({
                    kind: "setPolicy",
                    id: tabOptions.id,
                    policy: e.target.value as "follow" | "pinned",
                  }).then((ok) => {
                    if (ok) setTabOptions(null);
                  })
                }
              >
                <option value="follow">
                  {uiText("Follow local source refs")}
                </option>
                <option value="pinned">
                  {uiText("Keep pinned revisions")}
                </option>
              </select>
            </label>
            <p>
              {uiText(
                "Working changes remain live. Remote reviews keep their saved revision until you refresh the source. Reconstructed reviews require fresh code intelligence trust.",
              )}
            </p>
            <p>
              {uiText(
                "Duplicating copies the source revision into an independent tab. Unsent drafts stay here. Moving keeps this tab's saved drafts.",
              )}
            </p>
            <div className="tab-dialog-actions">
              <button
                disabled={busy || windows.length >= 2}
                onClick={() => {
                  void onWindow("duplicate", tabOptions.id);
                  setTabOptions(null);
                }}
              >
                {uiText("Duplicate source in new window")}
              </button>
              {windows
                .filter((w) => !w.current && w.workspace)
                .map((other) => (
                  <button
                    key={other.label}
                    disabled={busy}
                    onClick={() => {
                      void onWindow("move", tabOptions.id, other.label);
                      setTabOptions(null);
                    }}
                  >
                    {uiText("Move tab to")}{" "}
                    {document.workspaces.find((w) => w.id === other.workspace)
                      ?.name ?? uiText("other window")}
                  </button>
                ))}
              <button
                onClick={() => {
                  onRefresh(tabOptions.id);
                  setTabOptions(null);
                }}
              >
                {uiText("Refresh source")}
              </button>
              <button
                onClick={() => {
                  onDrafts(tabOptions.id);
                  setTabOptions(null);
                }}
              >
                {uiText("View unsent drafts")}
              </button>
              <button
                onClick={() => {
                  onUnload(tabOptions.id);
                  setTabOptions(null);
                }}
              >
                {uiText("Unload review")}
              </button>
              <button
                onClick={() => {
                  const tabs = workspace.tabs;
                  const at = tabs.findIndex((t) => t.id === tabOptions.id);
                  if (at > 0)
                    void run({
                      kind: "moveTab",
                      id: tabOptions.id,
                      before: tabs[at - 1]!.id,
                    });
                }}
              >
                {uiText("Move left")}
              </button>
              <button
                onClick={() => {
                  const tabs = workspace.tabs;
                  const at = tabs.findIndex((t) => t.id === tabOptions.id);
                  if (at < tabs.length - 1)
                    void run({
                      kind: "moveTab",
                      id: tabOptions.id,
                      before: tabs[at + 2]?.id ?? null,
                    });
                }}
              >
                {uiText("Move right")}
              </button>
            </div>
            {failure && <p role="alert">{failure}</p>}
            <button onClick={() => setTabOptions(null)}>
              {uiText("Done")}
            </button>
          </DialogContent>
        </Dialog>
      )}
    </>
  );
}
