import { uiText } from "../../vendor/diffle/src/client/i18n/runtime";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { X, RefreshCw, GitCommitHorizontal } from "lucide-react";
import {
  Dialog,
  DialogContent,
} from "../../vendor/diffle/src/client/ui/dialog";
import {
  gitApi,
  type Action,
  type Catalog,
  type Job,
  type Patch,
  type Preview,
} from "./api";
import { useCommitDraft } from "./draft";
import AdvancedOperations from "./AdvancedOperations";
import "./operations.css";
type Section =
  | "Advanced and recovery"
  | "Changes"
  | "Refs and remotes"
  | "Network"
  | "Worktrees"
  | "Stashes"
  | "New repository"
  | "Operations";
const sections: Section[] = [
  "Changes",
  "Advanced and recovery",
  "Refs and remotes",
  "Network",
  "Worktrees",
  "Stashes",
  "New repository",
  "Operations",
];
type Props = {
  initialSection?: Section;
  context: string | null;
  onClose: () => void;
  onChoose: () => Promise<unknown>;
  onTools?: () => Promise<unknown>;
  onOpen: (path: string) => Promise<unknown>;
  flushRef: { current: () => Promise<unknown> };
};
export default function GitOperations(props: Props) {
  const [section, setSection] = useState<Section>(
    props.initialSection ?? (props.context ? "Changes" : "New repository"),
  );
  const [catalog, setCatalog] = useState<Catalog | null>(null),
    [jobs, setJobs] = useState<Job[]>([]);
  const [preview, setPreview] = useState<Preview | null>(null),
    [patch, setPatch] = useState<Patch | null>(null);
  const [tick, setTick] = useState(0),
    [jobTick, setJobTick] = useState(0),
    [error, setError] = useState(""),
    [message, setMessage] = useState(""),
    [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState(""),
    [staged, setStaged] = useState(false),
    [search, setSearch] = useState(""),
    [page, setPage] = useState(0);
  const [form, setForm] = useState<Record<string, string>>({
    start: "HEAD",
    branch: "",
    remote: "",
    networkBranch: "",
    lockReason: "Keep for development",
    stashMessage: "Work in progress",
    newBranch: "main",
    newKind: "init",
  });
  const [upstream, setUpstream] = useState<{
    upstream: string | null;
    ahead: number | null;
    behind: number | null;
  } | null>(null);
  const [amend, setAmend] = useState(false),
    [untracked, setUntracked] = useState(false);
  const [evidence, setEvidence] = useState<{
    operation: string;
    text: string;
  } | null>(null);
  const pending = useRef(false),
    live = useRef(true),
    actionRead = useRef<AbortController | null>(null);
  const commitFlush = useRef<() => Promise<unknown>>(async () => {});
  const resolutionFlush = useRef<() => Promise<unknown>>(async () => {});
  const contextRef = useRef(props.context);
  contextRef.current = props.context;
  const draft = useCommitDraft(props.context, commitFlush);
  const flushAll = async () => {
    await resolutionFlush.current();
    await commitFlush.current();
  };
  useEffect(() => {
    props.flushRef.current = flushAll;
    return () => {
      props.flushRef.current = async () => {};
    };
  });
  useEffect(() => {
    actionRead.current?.abort();
    setPreview(null);
    setSelected("");
    setError("");
    setMessage("");
    setForm((form) => ({
      ...form,
      remote: "",
      networkBranch: "",
      existingBranch: "",
      existingTag: "",
      worktreeBranch: "",
      worktree: "",
      stash: "",
    }));
  }, [props.context]);
  const put = (name: string, value: string) =>
    setForm((form) => ({ ...form, [name]: value }));
  useEffect(
    () => () => {
      live.current = false;
      actionRead.current?.abort();
    },
    [],
  );
  useEffect(() => {
    let current = true;
    const controller = new AbortController();
    setCatalog(null);
    setPatch(null);
    if (props.context)
      void gitApi
        .catalog(props.context, controller.signal)
        .then((value) => {
          if (!current) return;
          setCatalog(value);
          setForm((form) => ({
            ...form,
            remote: form.remote || value.remotes[0] || "",
            networkBranch:
              form.networkBranch ||
              value.state.branch?.replace(/^refs\/heads\//, "") ||
              "main",
            existingBranch: form.existingBranch || value.branches[0] || "",
            existingTag: form.existingTag || value.tags[0] || "",
            worktreeBranch: form.worktreeBranch || value.branches[0] || "",
            worktree:
              form.worktree ||
              value.worktrees.find((w) => w.path !== value.state.identity.root)
                ?.path ||
              "",
            stash: form.stash || value.stashes[0]?.oid || "",
          }));
        })
        .catch((error) => current && setError(String(error)));
    return () => {
      current = false;
      controller.abort();
    };
  }, [props.context, tick]);
  useEffect(() => {
    let current = true;
    void gitApi
      .jobs()
      .then((jobs) => current && setJobs(jobs))
      .catch((error) => current && setError(String(error)));
    return () => {
      current = false;
    };
  }, [jobTick]);
  useEffect(() => {
    if (!jobs.some((job) => job.active)) return;
    const timer = setTimeout(() => setJobTick((tick) => tick + 1), 1000);
    return () => clearTimeout(timer);
  }, [jobs]);
  useEffect(() => {
    let current = true;
    const event = listen("git-operations-changed", () => {
      if (current) {
        setJobTick((n) => n + 1);
        setTick((n) => n + 1);
      }
    });
    void event.catch((e) => current && setError(String(e)));
    return () => {
      current = false;
      void event.then((stop) => stop()).catch(() => {});
    };
  }, []);
  useEffect(() => {
    let current = true;
    const controller = new AbortController();
    setUpstream(null);
    if (props.context && section === "Network")
      void gitApi
        .upstream(props.context, controller.signal)
        .then((value) => current && setUpstream(value))
        .catch((error) => current && setError(String(error)));
    return () => {
      current = false;
      controller.abort();
    };
  }, [props.context, section, tick]);
  useEffect(() => {
    let current = true;
    const controller = new AbortController();
    setPatch(null);
    if (props.context && selected && section === "Changes")
      void gitApi
        .patch(props.context, selected, staged, controller.signal)
        .then((patch) => current && setPatch(patch))
        .catch((error) => current && setError(String(error)));
    return () => {
      current = false;
      controller.abort();
    };
  }, [props.context, selected, staged, section, tick]);
  useEffect(
    () => () => {
      if (preview) void gitApi.forget(preview.id).catch(() => {});
    },
    [preview?.id],
  );
  async function perform(task: () => Promise<unknown>) {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError("");
    setMessage("");
    try {
      await task();
    } catch (error) {
      if (live.current) setError(String(error));
    } finally {
      pending.current = false;
      if (live.current) setBusy(false);
    }
  }
  const prepare = (action: Action) =>
    void perform(async () => {
      await flushAll();
      if (contextRef.current !== props.context) return;
      const controller = new AbortController();
      actionRead.current = controller;
      const prepared = await gitApi.preview(
        props.context,
        action,
        controller.signal,
      );
      if (
        live.current &&
        !controller.signal.aborted &&
        contextRef.current === props.context
      )
        setPreview(prepared);
      else void gitApi.forget(prepared.id).catch(() => {});
    });
  async function close() {
    actionRead.current?.abort();
    try {
      await flushAll();
      props.onClose();
    } catch (error) {
      setError(
        `Git draft was not saved; the dialog stayed open. ${String(error)}`,
      );
    }
  }
  const choose = () =>
    void perform(async () => {
      await flushAll();
      await props.onChoose();
    });
  const field = (name: string, label: string, placeholder = "") => (
    <label>
      {label}
      <input
        value={form[name] ?? ""}
        placeholder={placeholder}
        onChange={(event) => put(name, event.target.value)}
      />
    </label>
  );
  const select = (name: string, label: string, values: string[]) => (
    <label>
      {label}
      <select
        value={form[name] ?? ""}
        onChange={(event) => put(name, event.target.value)}
      >
        <option value="">{uiText("Choose…")}</option>
        {values.map((value) => (
          <option key={value} value={value}>
            {value}
          </option>
        ))}
      </select>
    </label>
  );
  const files = (catalog?.files ?? []).filter((file) =>
    file.path.toLocaleLowerCase().includes(search.toLocaleLowerCase()),
  );
  const file = catalog?.files.find((file) => file.path === selected);
  const recovery =
    !!catalog &&
    (catalog.state.conflicts || catalog.state.operations.length > 0);
  const selectedWorktree = catalog?.worktrees.find(
    (row) => row.path === form.worktree,
  );
  const jobNames: Record<string, string> = {
    pending: "Running",
    completed: "Completed",
    noChanges: "No changes",
    needsInspection: "Inspect result",
    observed: "State inspected",
    rejected: "Not started",
  };
  return (
    <Dialog open onOpenChange={(open) => !open && void close()}>
      <DialogContent
        className="git-operations"
        aria-labelledby="git-operations-title"
      >
        <header>
          <div>
            <h2 id="git-operations-title">{uiText("Git operations")}</h2>
            <p>
              {preview?.root ??
                preview?.before?.identity.root ??
                catalog?.state.identity.root ??
                (props.context
                  ? "Loading checkout…"
                  : "Choose a repository or create a new one.")}
            </p>
          </div>
          <button
            disabled={busy}
            onClick={choose}
            aria-label={uiText("Change Git repository")}
          >
            {uiText("Choose repository")}
          </button>
          <button
            aria-label={uiText("Close Git operations")}
            onClick={() => void close()}
          >
            <X size={17} />
          </button>
        </header>
        {preview ? (
          <div className="git-body git-confirm">
            <h3>{preview.label}</h3>
            {preview.before && (
              <small>
                {preview.before.branch?.replace(/^refs\/heads\//, "") ??
                  uiText("Detached or unborn HEAD")}{" "}
                · {preview.before.head ?? uiText("No commit yet")}
              </small>
            )}
            <pre>{preview.text}</pre>
            <p>
              {uiText(
                "Git state is checked again before execution. Closing this dialog after acceptance keeps the job running; quit waits for it.",
              )}
            </p>
            <div className="git-actions">
              <button disabled={busy} onClick={() => setPreview(null)}>
                {uiText("Back to action")}
              </button>
              <button
                className="primary"
                disabled={busy}
                onClick={() =>
                  void perform(async () => {
                    await gitApi.execute(preview.id, props.context);
                    setPreview(null);
                    setSection("Operations");
                    setJobTick((tick) => tick + 1);
                  })
                }
              >
                {busy ? uiText("Starting…") : preview.label}
              </button>
            </div>
          </div>
        ) : (
          <>
            <nav
              className="git-tabs"
              role="tablist"
              aria-label={uiText("Git operation categories")}
            >
              {sections.map((name) => (
                <button
                  key={name}
                  role="tab"
                  aria-selected={section === name}
                  disabled={busy}
                  onClick={() =>
                    void perform(async () => {
                      await resolutionFlush.current();
                      setSection(name);
                    })
                  }
                >
                  {name}
                  {name === "Operations" && jobs.some((job) => job.active)
                    ? ` (${jobs.filter((job) => job.active).length})`
                    : ""}
                </button>
              ))}
              <button
                aria-label={uiText("Refresh Git operations")}
                disabled={busy}
                onClick={() => {
                  setError("");
                  setTick((n) => n + 1);
                  setJobTick((n) => n + 1);
                }}
              >
                <RefreshCw size={15} />
              </button>
            </nav>
            {recovery && (
              <p role="alert">
                {uiText("Git has")}{" "}
                {catalog?.state.conflicts
                  ? uiText("unresolved conflicts")
                  : catalog?.state.operations.join(", ")}
                {uiText(
                  ". Open Advanced and recovery to inspect the index and available recovery actions. New operations are blocked.",
                )}
              </p>
            )}
            {!!catalog?.locks.length && (
              <p role="alert">
                {uiText("Git lock detected: ")}
                {catalog.locks.join(", ")}
                {uiText(
                  ". Check for a running Git process before recovering a stale lock externally.",
                )}
              </p>
            )}
            <fieldset disabled={busy} className="git-body">
              {!props.context &&
              section !== "New repository" &&
              section !== "Operations" ? (
                <div className="git-form">
                  <p>
                    {uiText(
                      "Choose a local repository to inspect its actual checkout and index.",
                    )}
                  </p>
                  <button onClick={choose}>
                    {uiText("Choose repository for Git operations")}
                  </button>
                </div>
              ) : section !== "New repository" &&
                section !== "Operations" &&
                !catalog ? (
                <p role="status">{uiText("Reading checkout…")}</p>
              ) : null}
              {catalog &&
                props.context &&
                section === "Advanced and recovery" && (
                  <AdvancedOperations
                    key={props.context}
                    context={props.context}
                    catalog={catalog}
                    prepare={prepare}
                    perform={perform}
                    flushRef={resolutionFlush}
                    onTools={
                      props.onTools
                        ? async () => {
                            await flushAll();
                            await props.onTools?.();
                          }
                        : undefined
                    }
                  />
                )}
              {catalog && section === "Changes" && (
                <div className="git-changes">
                  <nav
                    className="git-files"
                    aria-label={uiText("Changed files")}
                  >
                    <input
                      aria-label={uiText("Filter changed files")}
                      placeholder={uiText("Filter files…")}
                      value={search}
                      onChange={(event) => {
                        setSearch(event.target.value);
                        setPage(0);
                      }}
                    />
                    <small>
                      {files.length}
                      {uiText(" changed paths")}
                    </small>
                    {files.slice(page * 100, (page + 1) * 100).map((row) => (
                      <button
                        key={row.path}
                        aria-current={
                          selected === row.path ? "true" : undefined
                        }
                        onClick={() => {
                          setSelected(row.path);
                          setError("");
                        }}
                      >
                        {row.untracked ? "?" : `${row.index}${row.worktree}`}{" "}
                        {row.path}
                      </button>
                    ))}
                    <div className="git-actions">
                      <button
                        disabled={page === 0}
                        onClick={() => setPage((n) => n - 1)}
                      >
                        {uiText("Previous files")}
                      </button>
                      <button
                        disabled={(page + 1) * 100 >= files.length}
                        onClick={() => setPage((n) => n + 1)}
                      >
                        {uiText("Next files")}
                      </button>
                    </div>
                    <button
                      onClick={() => {
                        setSelected("");
                        setPatch(null);
                      }}
                    >
                      {uiText("Commit staged changes")}
                    </button>
                  </nav>
                  <div className="git-file-detail">
                    {file ? (
                      <>
                        <h3>{file.path}</h3>
                        <div className="git-actions">
                          <button
                            aria-pressed={!staged}
                            onClick={() => setStaged(false)}
                          >
                            {uiText("Unstaged diff")}
                          </button>
                          <button
                            aria-pressed={staged}
                            onClick={() => setStaged(true)}
                          >
                            {uiText("Staged diff")}
                          </button>
                          <button
                            disabled={recovery}
                            onClick={() =>
                              prepare({ kind: "stage", path: file.path })
                            }
                          >
                            {uiText("Preview stage file")}
                          </button>
                          <button
                            disabled={recovery || file.untracked}
                            onClick={() =>
                              prepare({ kind: "unstage", path: file.path })
                            }
                          >
                            {uiText("Preview unstage file")}
                          </button>
                          <button
                            className="danger"
                            disabled={recovery}
                            onClick={() =>
                              prepare({
                                kind: file.untracked ? "discard" : "restore",
                                path: file.path,
                              })
                            }
                          >
                            {file.untracked
                              ? uiText("Preview delete untracked file")
                              : uiText("Preview restore from index")}
                          </button>
                        </div>
                        {patch ? (
                          <>
                            <pre>
                              {patch.text ||
                                (file.untracked
                                  ? "Untracked file. Use whole-file staging; no index diff exists yet."
                                  : "No changes on this side.")}
                            </pre>
                            {patch.hunks.map((hunk, i) => (
                              <div key={i}>
                                <small>
                                  {hunk
                                    .split("\n")
                                    .find((line) => line.startsWith("@@"))}
                                </small>
                                <button
                                  disabled={recovery}
                                  onClick={() =>
                                    prepare({
                                      kind: "hunk",
                                      path: file.path,
                                      staged,
                                      hunk: i,
                                    })
                                  }
                                >
                                  {staged
                                    ? uiText("Preview unstage")
                                    : uiText("Preview stage")}{" "}
                                  {uiText("hunk ")}
                                  {i + 1}
                                </button>
                              </div>
                            ))}
                            {!patch.partial && (
                              <small>
                                {uiText(
                                  "Partial staging is unavailable for binary, add/delete, mode, symlink, non-UTF-8 or large patches. Use the whole-file action.",
                                )}
                              </small>
                            )}
                          </>
                        ) : (
                          <p role="status">
                            {uiText("Reading canonical diff…")}
                          </p>
                        )}
                      </>
                    ) : (
                      <div className="git-form">
                        <h3>{uiText("Commit staged changes")}</h3>
                        <small>
                          {
                            catalog.files.filter(
                              (f) => f.index !== " " && !f.untracked,
                            ).length
                          }{" "}
                          {uiText("staged paths on")}{" "}
                          {catalog.state.branch?.replace(
                            /^refs\/heads\//,
                            "",
                          ) ?? uiText("detached HEAD")}
                        </small>
                        <label>
                          {uiText("Commit message")}
                          <textarea
                            aria-label={uiText("Commit message")}
                            value={draft.message}
                            disabled={!draft.loaded}
                            onChange={(event) =>
                              draft.change(event.target.value)
                            }
                          />
                        </label>
                        <label>
                          <input
                            type="checkbox"
                            checked={amend}
                            onChange={(event) => setAmend(event.target.checked)}
                          />
                          {uiText("Amend the current commit")}
                        </label>
                        <div className="git-actions">
                          <button
                            onClick={() =>
                              void perform(async () => {
                                await flushAll();
                                setMessage(
                                  "Commit draft saved for this checkout.",
                                );
                              })
                            }
                          >
                            {uiText("Save commit draft")}
                          </button>
                          <button
                            disabled={!draft.loaded || !draft.message}
                            onClick={() =>
                              void perform(() => writeText(draft.message))
                            }
                          >
                            {uiText("Copy commit draft")}
                          </button>
                          <button
                            className="primary"
                            disabled={
                              recovery || !draft.loaded || !draft.message.trim()
                            }
                            onClick={() =>
                              prepare({
                                kind: "commit",
                                message: draft.message,
                                amend,
                              })
                            }
                          >
                            <GitCommitHorizontal size={15} />
                            {amend
                              ? uiText("Preview amend")
                              : uiText("Preview commit")}
                          </button>
                        </div>
                        <p>
                          {uiText(
                            "Drafts stay with this checkout through navigation and errors. Hooks and signing use your Git configuration; terminal prompts need an external client.",
                          )}
                        </p>
                        {draft.error && <p role="alert">{draft.error}</p>}
                        <button onClick={() => void perform(draft.load)}>
                          {uiText(
                            "Reload saved commit draft (replaces this message)",
                          )}
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              )}
              {catalog && section === "Refs and remotes" && (
                <div className="git-form">
                  <section>
                    <h3>{uiText("Branches")}</h3>
                    {field("branch", "New branch name")}
                    {field("start", "Starting revision", "HEAD")}
                    <button
                      disabled={recovery}
                      onClick={() =>
                        prepare({
                          kind: "branchCreate",
                          name: form.branch ?? "",
                          start: form.start ?? "HEAD",
                        })
                      }
                    >
                      {uiText("Preview create branch")}
                    </button>
                    {select(
                      "existingBranch",
                      "Existing branch",
                      catalog.branches,
                    )}
                    <div className="git-actions">
                      <button
                        disabled={recovery}
                        onClick={() =>
                          prepare({
                            kind: "branchSwitch",
                            name: form.existingBranch ?? "",
                          })
                        }
                      >
                        {uiText("Preview switch branch")}
                      </button>
                      <button
                        className="danger"
                        disabled={recovery}
                        onClick={() =>
                          prepare({
                            kind: "branchDelete",
                            name: form.existingBranch ?? "",
                          })
                        }
                      >
                        {uiText("Preview delete merged branch")}
                      </button>
                    </div>
                  </section>
                  <section>
                    <h3>{uiText("Lightweight tags")}</h3>
                    {field("tag", "New tag name")}
                    {field("tagStart", "Tag revision", "HEAD")}
                    <button
                      disabled={recovery}
                      onClick={() =>
                        prepare({
                          kind: "tagCreate",
                          name: form.tag ?? "",
                          start: form.tagStart || "HEAD",
                        })
                      }
                    >
                      {uiText("Preview create tag")}
                    </button>
                    {select("existingTag", "Existing tag", catalog.tags)}
                    <button
                      className="danger"
                      disabled={recovery}
                      onClick={() =>
                        prepare({
                          kind: "tagDelete",
                          name: form.existingTag ?? "",
                        })
                      }
                    >
                      {uiText("Preview delete tag")}
                    </button>
                  </section>
                  <section>
                    <h3>{uiText("Remotes")}</h3>
                    {field("remoteName", "New remote name", "origin")}
                    {field(
                      "remoteUrl",
                      "Remote URL",
                      "https://host/team/repository.git",
                    )}
                    <button
                      disabled={recovery}
                      onClick={() =>
                        prepare({
                          kind: "remoteAdd",
                          name: form.remoteName ?? "",
                          url: form.remoteUrl ?? "",
                        })
                      }
                    >
                      {uiText("Preview add remote")}
                    </button>
                    {select("remote", "Existing remote", catalog.remotes)}
                    <button
                      className="danger"
                      disabled={recovery}
                      onClick={() =>
                        prepare({
                          kind: "remoteRemove",
                          name: form.remote ?? "",
                        })
                      }
                    >
                      {uiText("Preview remove remote")}
                    </button>
                  </section>
                </div>
              )}
              {catalog && section === "Network" && (
                <div className="git-form">
                  <h3>{uiText("Fetch, fast-forward pull and push")}</h3>
                  {upstream && (
                    <p>
                      {upstream.upstream
                        ? uiText(
                            "{value0}: ahead {value1}, behind {value2} (local refs)",
                            {
                              value0: String(upstream.upstream),
                              value1: String(upstream.ahead ?? "?"),
                              value2: String(upstream.behind ?? "?"),
                            },
                          )
                        : uiText("No local upstream configured.")}
                    </p>
                  )}
                  {select("remote", "Remote", catalog.remotes)}
                  {field("networkBranch", "Remote branch")}
                  {!!catalog.state.head && (
                    <small>
                      {uiText("Local HEAD: ")}
                      {catalog.state.head}
                    </small>
                  )}
                  <p>
                    {uiText(
                      "Uses installed Git credentials and helpers. Push always checks an exact remote ref lease; a normal push also requires a fast-forward. Fetch first if the remote history is unavailable locally.",
                    )}
                  </p>
                  <div className="git-actions">
                    <button
                      disabled={recovery}
                      onClick={() =>
                        prepare({ kind: "fetch", remote: form.remote ?? "" })
                      }
                    >
                      {uiText("Preview fetch")}
                    </button>
                    <button
                      disabled={recovery}
                      onClick={() =>
                        prepare({
                          kind: "pull",
                          remote: form.remote ?? "",
                          branch: form.networkBranch ?? "",
                        })
                      }
                    >
                      {uiText("Preview fast-forward pull")}
                    </button>
                    <button
                      disabled={recovery}
                      onClick={() =>
                        prepare({
                          kind: "push",
                          remote: form.remote ?? "",
                          branch: form.networkBranch ?? "",
                          force: false,
                        })
                      }
                    >
                      {uiText("Preview push")}
                    </button>
                    <button
                      className="danger"
                      disabled={recovery}
                      onClick={() =>
                        prepare({
                          kind: "push",
                          remote: form.remote ?? "",
                          branch: form.networkBranch ?? "",
                          force: true,
                        })
                      }
                    >
                      {uiText("Preview push with lease")}
                    </button>
                  </div>
                </div>
              )}
              {catalog && section === "Worktrees" && (
                <div className="git-form">
                  <section>
                    <h3>{uiText("Create a linked checkout")}</h3>
                    {field(
                      "worktreePath",
                      "New worktree folder",
                      "/absolute/path/to/new-folder",
                    )}
                    {select(
                      "worktreeBranch",
                      "Branch for new worktree",
                      catalog.branches,
                    )}
                    <button
                      disabled={recovery}
                      onClick={() =>
                        prepare({
                          kind: "worktreeCreate",
                          path: form.worktreePath ?? "",
                          branch: form.worktreeBranch ?? "",
                        })
                      }
                    >
                      {uiText("Preview create worktree")}
                    </button>
                    <small>
                      {uiText(
                        "Create a branch in Refs and remotes first if needed. Existing folders are never reused.",
                      )}
                    </small>
                  </section>
                  <section>
                    <h3>{uiText("Manage existing worktrees")}</h3>
                    {select(
                      "worktree",
                      "Linked worktree",
                      catalog.worktrees
                        .filter((w) => !w.bare && w.canModify && !w.prunable)
                        .map((w) => w.path),
                    )}
                    {selectedWorktree && (
                      <p>
                        {selectedWorktree.branch ?? uiText("Detached HEAD")}
                        {selectedWorktree.locked !== null
                          ? uiText(" · Locked: {value0}", {
                              value0: String(selectedWorktree.locked),
                            })
                          : ""}
                      </p>
                    )}
                    {field("lockReason", "Worktree lock reason")}
                    <div className="git-actions">
                      <button
                        disabled={recovery}
                        onClick={() =>
                          prepare({
                            kind: "worktreeLock",
                            path: form.worktree ?? "",
                            reason: form.lockReason ?? "",
                          })
                        }
                      >
                        {uiText("Preview lock worktree")}
                      </button>
                      <button
                        disabled={recovery}
                        onClick={() =>
                          prepare({
                            kind: "worktreeUnlock",
                            path: form.worktree ?? "",
                          })
                        }
                      >
                        {uiText("Preview unlock worktree")}
                      </button>
                      <button
                        className="danger"
                        disabled={recovery}
                        onClick={() =>
                          prepare({
                            kind: "worktreeRemove",
                            path: form.worktree ?? "",
                          })
                        }
                      >
                        {uiText("Preview remove clean worktree")}
                      </button>
                      <button
                        disabled={recovery}
                        onClick={() => prepare({ kind: "worktreePrune" })}
                      >
                        {uiText("Preview prune missing registrations")}
                      </button>
                    </div>
                    <p>
                      {uiText(
                        "Removal refuses dirty, ignored-file, submodule, locked, managed or in-use checkouts. Folders opened in external tools remain protected across restarts; remove those externally after closing their tools.",
                      )}
                    </p>
                  </section>
                </div>
              )}
              {catalog && section === "Stashes" && (
                <div className="git-form">
                  <section>
                    <h3>{uiText("Save changes")}</h3>
                    {field("stashMessage", "Stash message")}
                    <label>
                      <input
                        type="checkbox"
                        checked={untracked}
                        onChange={(event) => setUntracked(event.target.checked)}
                      />
                      {uiText("Include untracked files")}
                    </label>
                    <button
                      disabled={recovery}
                      onClick={() =>
                        prepare({
                          kind: "stashSave",
                          message: form.stashMessage ?? "",
                          untracked,
                        })
                      }
                    >
                      {uiText("Preview save stash")}
                    </button>
                  </section>
                  <section>
                    <h3>{uiText("Saved stashes")}</h3>
                    <label>
                      {uiText("Stash")}
                      <select
                        value={form.stash ?? ""}
                        onChange={(event) => put("stash", event.target.value)}
                      >
                        <option value="">{uiText("Choose…")}</option>
                        {catalog.stashes.map((stash) => (
                          <option key={stash.oid} value={stash.oid}>
                            {stash.label} · {stash.oid.slice(0, 10)}
                          </option>
                        ))}
                      </select>
                    </label>
                    <div className="git-actions">
                      <button
                        disabled={recovery}
                        onClick={() =>
                          prepare({ kind: "stashApply", oid: form.stash ?? "" })
                        }
                      >
                        {uiText("Preview apply stash")}
                      </button>
                      <button
                        disabled={
                          recovery || form.stash !== catalog.stashes[0]?.oid
                        }
                        onClick={() =>
                          prepare({ kind: "stashPop", oid: form.stash ?? "" })
                        }
                      >
                        {uiText("Preview pop latest stash")}
                      </button>
                      <button
                        className="danger"
                        disabled={
                          recovery || form.stash !== catalog.stashes[0]?.oid
                        }
                        onClick={() =>
                          prepare({ kind: "stashDrop", oid: form.stash ?? "" })
                        }
                      >
                        {uiText("Preview drop latest stash")}
                      </button>
                    </div>
                    <p>
                      {uiText(
                        "Apply restores the index and keeps the stash. Pop only drops after a successful apply; conflicts retain it for external recovery. Pop/drop currently targets the latest stash only.",
                      )}
                    </p>
                  </section>
                </div>
              )}
              {section === "New repository" && (
                <div className="git-form">
                  <h3>{uiText("Create a user-owned repository")}</h3>
                  <label>
                    {uiText("Operation")}
                    <select
                      value={form.newKind}
                      onChange={(event) => put("newKind", event.target.value)}
                    >
                      <option value="init">
                        {uiText("Initialize empty repository")}
                      </option>
                      <option value="clone">
                        {uiText("Clone repository")}
                      </option>
                    </select>
                  </label>
                  <div className="git-actions">
                    <button
                      onClick={() =>
                        void perform(async () => {
                          const parent = await open({
                            directory: true,
                            multiple: false,
                            title: uiText("Parent folder for new repository"),
                          });
                          if (parent) put("newParent", parent);
                        })
                      }
                    >
                      {uiText("Choose parent folder")}
                    </button>
                  </div>
                  {field(
                    "newParent",
                    "Parent folder",
                    "/absolute/parent/folder",
                  )}
                  {field("newName", "New folder name")}
                  {form.newKind === "clone"
                    ? field(
                        "cloneUrl",
                        "Clone URL",
                        "https://host/team/repository.git",
                      )
                    : field("newBranch", "Initial branch")}
                  <button
                    className="primary"
                    onClick={() => {
                      const name = form.newName ?? "";
                      if (
                        !name ||
                        name === "." ||
                        name === ".." ||
                        name.includes("/")
                      ) {
                        setError(
                          "Enter one new folder name, without path separators.",
                        );
                        return;
                      }
                      const path = `${form.newParent ?? ""}/${name}`;
                      prepare(
                        form.newKind === "clone"
                          ? { kind: "clone", path, url: form.cloneUrl ?? "" }
                          : {
                              kind: "init",
                              path,
                              branch: form.newBranch || "main",
                            },
                      );
                    }}
                  >
                    {form.newKind === "clone"
                      ? uiText("Preview clone")
                      : uiText("Preview initialize")}
                  </button>
                  <p>
                    {uiText(
                      "Existing folders are never replaced. Failed or cancelled clones keep their partial checkout for inspection. Open the result explicitly from Operations.",
                    )}
                  </p>
                </div>
              )}
              {section === "Operations" && (
                <div className="git-jobs">
                  <p>
                    {uiText(
                      "Accepted jobs survive closing this dialog. Interrupted or failed results need inspection; no operation is automatically retried or rolled back.",
                    )}
                  </p>
                  {jobs.length === 0 && (
                    <p>{uiText("No Git operations recorded.")}</p>
                  )}
                  {jobs.map((job) => (
                    <article key={job.receipt.operation} className="git-job">
                      <h3>
                        {job.receipt.label} —{" "}
                        {jobNames[job.receipt.status] ?? job.receipt.status}
                      </h3>
                      <small>{job.receipt.root}</small>
                      {job.receipt.targetRoot && (
                        <small>
                          {uiText("Destination: ")}
                          {job.receipt.targetRoot}
                        </small>
                      )}
                      {job.receipt.intent &&
                        (!job.receipt.intent.path ||
                          (job.receipt.intent.path !== job.receipt.root &&
                            job.receipt.intent.path !==
                              job.receipt.targetRoot)) && (
                          <small>
                            {job.receipt.intent.path ??
                              job.receipt.intent.name ??
                              job.receipt.intent.oid ??
                              (job.receipt.intent.remote
                                ? `${job.receipt.intent.remote}/${job.receipt.intent.branch ?? ""}`
                                : "")}
                          </small>
                        )}
                      <p>{job.receipt.message}</p>
                      <div className="git-actions">
                        {job.canCancel && (
                          <button
                            onClick={() =>
                              void perform(async () => {
                                await gitApi.cancel(job.receipt.operation);
                                setMessage(
                                  "Cancellation requested. Inspect the final result before retrying.",
                                );
                                setJobTick((n) => n + 1);
                              })
                            }
                          >
                            {uiText("Cancel running operation")}
                          </button>
                        )}
                        {!job.active && (
                          <button
                            onClick={() =>
                              void perform(async () => {
                                const result = await gitApi.inspect(
                                  job.receipt.operation,
                                );
                                setEvidence({
                                  operation: job.receipt.operation,
                                  text: `Branch: ${result.current.state.branch ?? "detached / unborn"}\nHEAD: ${result.current.state.head ?? "none"}\nChanged paths: ${result.current.files.length}\nConflicts: ${result.current.state.conflicts ? "yes" : "no"}\nActive Git operation: ${result.current.state.operations.join(", ") || "none"}${result.remoteHead ? `\nCurrent remote head: ${result.remoteHead}` : ""}\n${result.receipt.message}`,
                                });
                                setJobTick((n) => n + 1);
                              })
                            }
                          >
                            {uiText("Inspect current state")}
                          </button>
                        )}
                        {!job.active && job.receipt.status === "completed" && (
                          <button
                            onClick={() =>
                              void perform(async () => {
                                await flushAll();
                                await props.onOpen(
                                  job.receipt.targetRoot ?? job.receipt.root,
                                );
                                props.onClose();
                              })
                            }
                          >
                            {uiText("Open working changes")}
                          </button>
                        )}
                        <button
                          onClick={() =>
                            void perform(() =>
                              writeText(
                                job.receipt.targetRoot ?? job.receipt.root,
                              ),
                            )
                          }
                        >
                          {uiText("Copy folder path")}
                        </button>
                      </div>
                      {evidence?.operation === job.receipt.operation && (
                        <pre>{evidence.text}</pre>
                      )}
                    </article>
                  ))}
                </div>
              )}
            </fieldset>
          </>
        )}
        {busy && <p role="status">{uiText("Preparing or saving…")}</p>}
        {message && <p role="status">{message}</p>}
        {error && <p role="alert">{error}</p>}
      </DialogContent>
    </Dialog>
  );
}
