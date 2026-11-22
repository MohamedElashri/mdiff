import { uiText } from "../../vendor/diffle/src/client/i18n/runtime";
import type { Activity } from "../agents/api";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { GitFork, Terminal, Code2, RefreshCw, X } from "lucide-react";
import {
  Dialog,
  DialogContent,
} from "../../vendor/diffle/src/client/ui/dialog";
import {
  worktreeApi,
  type Inventory,
  type Worktree,
  type Status,
  type Tools,
  type Kind,
} from "./api";
import "./worktrees.css";
type Props = {
  id: string;
  initial: "worktrees" | Kind;
  onClose: () => void;
  onPreferences?: () => void;
  onOpen: (path: string) => Promise<unknown>;
  agentActivity?: Activity[];
  onEmbedded?: (worktree: string) => Promise<unknown>;
  onInventory: (rows: Worktree[]) => void;
};
export default function WorktreesHub(props: Props) {
  const [inventory, setInventory] = useState<Inventory | null>(null),
    [selected, setSelected] = useState<string | null>(null);
  const [tab, setTab] = useState<"worktrees" | Kind>(props.initial),
    [status, setStatus] = useState<Status | null>(null),
    [tools, setTools] = useState<Tools | null>(null);
  const [terminal, setTerminal] = useState(""),
    [tick, setTick] = useState(0),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [message, setMessage] = useState("");
  const generation = useRef(0),
    pending = useRef(false),
    savingPreference = useRef(false),
    terminalRef = useRef(terminal),
    toolsRef = useRef(tools);
  terminalRef.current = terminal;
  toolsRef.current = tools;
  const row = inventory?.items.find((row) => row.id === selected) ?? null;
  useEffect(
    () => () => {
      generation.current++;
    },
    [],
  );
  useEffect(() => {
    if (tab !== "worktrees") return;
    let live = true;
    const cancel = new AbortController();
    setError("");
    void worktreeApi
      .read<Inventory>(props.id, { kind: "list" }, cancel.signal)
      .then((value) => {
        if (!live) return;
        setInventory(value);
        setSelected((old) =>
          value.items.some((row) => row.id === old)
            ? old
            : (value.items.find((row) => row.current)?.id ??
              value.items[0]?.id ??
              null),
        );
        props.onInventory(value.items);
      })
      .catch((error) => live && setError(String(error)));
    return () => {
      live = false;
      cancel.abort();
    };
  }, [props.id, tab === "worktrees", tick]);
  useEffect(() => {
    setStatus(null);
    if (tab !== "worktrees" || !row?.canInspect) return;
    let live = true;
    const cancel = new AbortController();
    void worktreeApi
      .read<Status>(
        props.id,
        { kind: "status", worktree: row.id },
        cancel.signal,
      )
      .then((value) => live && setStatus(value))
      .catch((error) => live && setError(String(error)));
    return () => {
      live = false;
      cancel.abort();
    };
  }, [props.id, selected, tab === "worktrees", tick, row?.canInspect]);
  useEffect(() => {
    if (tab === "worktrees") return;
    let live = true;
    setTools(null);
    setError("");
    void worktreeApi
      .tools(props.id, selected)
      .then((value) => {
        if (live) {
          setTools(value);
          setTerminal(
            value.preferences.terminal ?? value.terminals[0]?.id ?? "",
          );
        }
      })
      .catch((error) => live && setError(String(error)));
    return () => {
      live = false;
    };
  }, [props.id, selected, tab === "worktrees", tick]);
  useEffect(() => {
    let live = true;
    const listener = listen<number>("external-tool-preferences", (event) => {
      if (savingPreference.current) return;
      const current = toolsRef.current;
      if (!live || !current || event.payload <= current.preferences.revision)
        return;
      if (
        terminalRef.current !==
        (current.preferences.terminal ?? current.terminals[0]?.id ?? "")
      ) {
        setError(
          "Terminal preference changed in another window. Refresh tools before saving.",
        );
        return;
      }
      setTick((tick) => tick + 1);
    });
    return () => {
      live = false;
      void listener.then((stop) => stop());
    };
  }, []);
  async function act(task: () => Promise<unknown>, success: string) {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError("");
    setMessage("");
    const ticket = generation.current;
    try {
      await task();
      if (ticket === generation.current) setMessage(success);
    } catch (error) {
      if (ticket === generation.current) setError(String(error));
    } finally {
      if (ticket === generation.current) {
        pending.current = false;
        setBusy(false);
      }
    }
  }
  const launch = (kind: Kind, toolId: string, name: string) =>
    act(
      () => worktreeApi.launch(props.id, selected, kind, toolId),
      `${name} launch accepted for the displayed folder.`,
    );
  const open = () =>
    row &&
    act(async () => {
      const result = await worktreeApi.read<{ path: string }>(props.id, {
        kind: "open",
        worktree: row.id,
      });
      await props.onOpen(result.path);
    }, "Review opened.");
  const save = (value: string | null) =>
    tools &&
    act(async () => {
      savingPreference.current = true;
      try {
        const preferences = await worktreeApi.save(
          tools.preferences.revision,
          value,
        );
        setTools({ ...tools, preferences });
      } finally {
        savingPreference.current = false;
      }
    }, "Default terminal saved.");
  function changeTab(tab: "worktrees" | Kind) {
    if (busy) return;
    setTab(tab);
    setError("");
    setMessage("");
  }
  return (
    <Dialog open onOpenChange={(open) => !open && props.onClose()}>
      <DialogContent className="worktree-hub" aria-labelledby="worktree-title">
        <header className="worktree-heading">
          <div>
            <h2 id="worktree-title">{uiText("Worktrees and tools")}</h2>
            <p>
              {inventory?.root ?? tools?.root ?? uiText("Loading repository…")}
            </p>
          </div>
          <button
            aria-label={uiText("Close worktrees and tools")}
            onClick={props.onClose}
          >
            <X size={17} />
          </button>
        </header>
        <div
          className="worktree-tabs"
          role="tablist"
          aria-label={uiText("Repository tools")}
        >
          <button
            role="tab"
            aria-selected={tab === "worktrees"}
            disabled={busy}
            onClick={() => changeTab("worktrees")}
          >
            <GitFork size={15} />
            {uiText("Worktrees")}
          </button>
          <button
            role="tab"
            aria-selected={tab !== "worktrees"}
            disabled={busy || (!!row && !row.canInspect)}
            onClick={() => changeTab("terminal")}
          >
            <Terminal size={15} />
            {uiText("External tools")}
          </button>
          <button
            aria-label={uiText("Refresh worktrees and tools")}
            disabled={busy}
            onClick={() => {
              setError("");
              setTick((tick) => tick + 1);
            }}
          >
            <RefreshCw size={15} />
          </button>
        </div>
        {error && (
          <p className="worktree-error" role="alert">
            {error}
          </p>
        )}
        {tab === "worktrees" ? (
          <div className="worktree-columns">
            <nav
              aria-label={uiText("Linked worktrees")}
              className="worktree-list"
            >
              {inventory?.items.map((item) => (
                <button
                  key={item.id}
                  aria-current={selected === item.id ? "true" : undefined}
                  disabled={busy}
                  onClick={() => {
                    setSelected(item.id);
                    setError("");
                    setMessage("");
                  }}
                >
                  <strong>
                    {item.branch ??
                      (item.bare
                        ? "Bare repository"
                        : item.detached
                          ? "Detached HEAD"
                          : "Unborn worktree")}
                    {item.current && <small>{uiText("Current folder")}</small>}
                  </strong>
                  <code>{item.path}</code>
                  <span>
                    {item.head?.slice(0, 10)}
                    {item.locked !== null ? uiText(" · Locked") : ""}
                    {item.prunable !== null ? uiText(" · Unavailable") : ""}
                  </span>
                </button>
              ))}
              {!inventory && !error && (
                <p role="status">{uiText("Reading worktree registrations…")}</p>
              )}
              {inventory?.items.length === 0 && (
                <p>
                  {uiText(
                    "No worktrees found. Refresh after making changes in your Git client.",
                  )}
                </p>
              )}
            </nav>
            <section
              className="worktree-details"
              aria-label={uiText("Selected worktree")}
            >
              {row ? (
                <>
                  <h3>
                    {row.branch ??
                      (row.detached ? "Detached HEAD" : "Working folder")}
                  </h3>
                  <code className="worktree-path">{row.path}</code>
                  {row.reason && <p>{row.reason}</p>}
                  {row.locked !== null && (
                    <p>
                      {uiText("Locked: ")}
                      {row.locked || uiText("No reason supplied")}
                    </p>
                  )}
                  {row.prunable !== null && (
                    <p>
                      {uiText("Prunable: ")}
                      {row.prunable || uiText("Working folder missing")}
                    </p>
                  )}
                  <div className="worktree-actions">
                    <button
                      className="primary"
                      disabled={busy || !row.canOpen}
                      onClick={() => void open()}
                    >
                      {busy
                        ? uiText("Opening…")
                        : uiText("Open working changes")}
                    </button>
                    <button
                      disabled={busy || !row.canInspect}
                      onClick={() => changeTab("editor")}
                    >
                      <Code2 size={14} />
                      {uiText("Open in editor")}
                    </button>
                    <button
                      disabled={busy || !row.canInspect}
                      onClick={() => changeTab("terminal")}
                    >
                      <Terminal size={14} />
                      {uiText("Open in terminal")}
                    </button>
                  </div>
                  {row.canInspect &&
                    (status ? (
                      <>
                        <dl className="worktree-status">
                          <div>
                            <dt>{uiText("Index changes")}</dt>
                            <dd>{status.staged}</dd>
                          </div>
                          <div>
                            <dt>{uiText("Working changes")}</dt>
                            <dd>{status.unstaged}</dd>
                          </div>
                          <div>
                            <dt>{uiText("Untracked entries")}</dt>
                            <dd>{status.untracked}</dd>
                          </div>
                          <div>
                            <dt>{uiText("Conflicts")}</dt>
                            <dd>{status.conflicts}</dd>
                          </div>
                        </dl>
                        <p>
                          {uiText("Observed branch:")}{" "}
                          {status.branch ??
                            (status.head ? "Detached HEAD" : "Unborn")}
                        </p>
                        <code>HEAD: {status.head ?? "No commit yet"}</code>
                        {status.upstream ? (
                          <p>
                            {uiText("Local upstream ")}
                            {status.upstream}: {status.ahead ?? "?"}
                            {uiText(" ahead, ")}
                            {status.behind ?? "?"} {uiText("behind.")}
                          </p>
                        ) : (
                          <p>{uiText("No local upstream configured.")}</p>
                        )}
                        {status.shallow && (
                          <p>
                            {uiText(
                              "Shallow history: ahead/behind counts reflect locally available history.",
                            )}
                          </p>
                        )}
                        <small>
                          {uiText(
                            "Status is read on selection or refresh. Remotes are not fetched.",
                          )}
                        </small>
                      </>
                    ) : (
                      !error && (
                        <p role="status">
                          {uiText("Reading selected worktree status…")}
                        </p>
                      )
                    ))}
                  {props.agentActivity
                    ?.filter(
                      (a) =>
                        a.root === row.path &&
                        !["unavailable", "ended"].includes(a.state),
                    )
                    .map((a) => (
                      <p className="agent-badge" key={a.id}>
                        {a.tool}: {a.state} ({a.provenance})
                      </p>
                    ))}
                  {props.onEmbedded && (
                    <button
                      disabled={busy || !row.canInspect}
                      onClick={() =>
                        void act(
                          async () => props.onEmbedded?.(row.id),
                          "Shell started in the selected checkout.",
                        )
                      }
                    >
                      {uiText("Start embedded terminal in this worktree")}
                    </button>
                  )}
                  {row.reviews.length > 0 && (
                    <section>
                      <h4>{uiText("Known review tabs")}</h4>
                      {row.reviews.map((review) => (
                        <p key={review.tab}>
                          {review.workspaceName}: {review.label}
                        </p>
                      ))}
                    </section>
                  )}
                </>
              ) : (
                <p>{uiText("Select a worktree to inspect its folder.")}</p>
              )}
            </section>
          </div>
        ) : (
          <section
            className="worktree-external"
            aria-label={uiText("External tools")}
          >
            {tools ? (
              <>
                <code className="worktree-path">{tools.root}</code>
                {tools.temporary && (
                  <p>
                    {uiText(
                      "Downloaded review checkout. Launching a tool retains this folder for external edits and recovery.",
                    )}
                  </p>
                )}
                <section
                  className={
                    tab === "terminal"
                      ? "worktree-tool-section selected"
                      : "worktree-tool-section"
                  }
                >
                  <h3>{uiText("Terminal")}</h3>
                  <label>
                    {uiText("Terminal application")}
                    <select
                      aria-label={uiText("Terminal application")}
                      disabled={busy}
                      value={terminal}
                      onChange={(event) => setTerminal(event.target.value)}
                    >
                      <option value="">{uiText("Choose a terminal")}</option>
                      {tools.preferences.terminal &&
                        !tools.terminals.some(
                          (tool) => tool.id === tools.preferences.terminal,
                        ) && (
                          <option value={tools.preferences.terminal}>
                            {uiText("Missing: ")}
                            {tools.preferences.terminal}
                          </option>
                        )}
                      {tools.terminals.map((tool) => (
                        <option key={tool.id} value={tool.id}>
                          {tool.name}
                        </option>
                      ))}
                    </select>
                  </label>
                  {tools.terminals.length === 0 && (
                    <p>
                      {uiText(
                        "No supported terminals found. Install a supported terminal or enable its command-line launcher, then refresh.",
                      )}
                    </p>
                  )}
                  <div className="worktree-actions">
                    <button
                      disabled={
                        busy ||
                        !tools.terminals.some((tool) => tool.id === terminal)
                      }
                      onClick={() => {
                        const tool = tools.terminals.find(
                          (tool) => tool.id === terminal,
                        )!;
                        void launch("terminal", tool.id, tool.name);
                      }}
                    >
                      {uiText("Open terminal")}
                    </button>
                    {props.onPreferences ? (
                      <button onClick={props.onPreferences}>
                        {uiText("Terminal preferences")}
                      </button>
                    ) : (
                      <>
                        <button
                          disabled={
                            busy ||
                            !terminal ||
                            terminal === tools.preferences.terminal ||
                            !tools.terminals.some(
                              (tool) => tool.id === terminal,
                            )
                          }
                          onClick={() => void save(terminal)}
                        >
                          {uiText("Use as default")}
                        </button>
                        <button
                          disabled={busy || !tools.preferences.terminal}
                          onClick={() => void save(null)}
                        >
                          {uiText("Clear default")}
                        </button>
                      </>
                    )}
                  </div>
                  <small>
                    {uiText(
                      "Only Open terminal starts a shell. The default is shared across workspace windows.",
                    )}
                  </small>
                </section>
                <section
                  className={
                    tab === "editor"
                      ? "worktree-tool-section selected"
                      : "worktree-tool-section"
                  }
                >
                  <h3>{uiText("Editor")}</h3>
                  <div className="worktree-actions">
                    {tools.editors.map((tool) => (
                      <button
                        key={tool.id}
                        disabled={busy}
                        onClick={() =>
                          void launch("editor", tool.id, tool.name)
                        }
                      >
                        {tool.name}
                      </button>
                    ))}
                  </div>
                  {tools.editors.length === 0 && (
                    <p>
                      {uiText(
                        "No supported editors found. Install an editor or enable its launcher, then refresh.",
                      )}
                    </p>
                  )}
                </section>
              </>
            ) : (
              !error && (
                <p role="status">{uiText("Finding installed tools…")}</p>
              )
            )}
          </section>
        )}
        {message && <p role="status">{message}</p>}
      </DialogContent>
    </Dialog>
  );
}
