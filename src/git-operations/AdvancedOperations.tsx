import { uiText } from "../../vendor/diffle/src/client/i18n/runtime";
import { useEffect, useRef, useState } from "react";
import {
  gitApi,
  type Action,
  type Catalog,
  type Conflict,
  type RebaseCommit,
  type RebaseItem,
} from "./api";
import { useCommitDraft } from "./draft";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

type Flush = { current: () => Promise<unknown> };
type Props = {
  context: string;
  catalog: Catalog;
  prepare: (action: Action) => void;
  perform: (task: () => Promise<unknown>) => Promise<void>;
  flushRef: Flush;
  onTools?: () => Promise<unknown>;
};
export type ConflictChunk = {
  start: number;
  end: number;
  current: string;
  incoming: string;
};
// Choices replace one current marker block, preserving manual edits outside it.
export function conflictChunks(text: string): ConflictChunk[] {
  const lines = text.match(/[^\n]*\n|[^\n]+$/g) ?? [];
  const chunks: ConflictChunk[] = [];
  let offset = 0,
    start = -1,
    current = "",
    incoming = "",
    side = 0;
  for (const line of lines) {
    if (/^<{7,}(?: |\r?$)/.test(line)) {
      if (start >= 0) return [];
      start = offset;
      current = "";
      incoming = "";
      side = 1;
    } else if (start >= 0 && /^\|{7,}(?: |\r?$)/.test(line)) side = 2;
    else if (start >= 0 && /^={7,}\r?\n?$/.test(line)) side = 3;
    else if (start >= 0 && /^>{7,}(?: |\r?$)/.test(line)) {
      if (side !== 3) return [];
      chunks.push({ start, end: offset + line.length, current, incoming });
      start = -1;
      side = 0;
    } else if (side === 1) current += line;
    else if (side === 3) incoming += line;
    offset += line.length;
  }
  return start < 0 ? chunks : [];
}
function Resolution({
  view,
  context,
  flushRef,
  prepare,
  perform,
}: {
  view: Conflict;
  context: string;
  flushRef: Flush;
  prepare: Props["prepare"];
  perform: Props["perform"];
}) {
  const draft = useCommitDraft(
    context,
    flushRef,
    `resolution/${JSON.stringify([view.path, view.draftKey])}`,
  );
  const initialized = useRef(false);
  useEffect(() => {
    if (draft.loaded && !initialized.current) {
      initialized.current = true;
      if (draft.revision() === 0) draft.change(view.result);
    }
  }, [draft.loaded, view.result]);
  const chunks = conflictChunks(draft.message);
  return (
    <div className="git-resolution">
      {draft.loaded && draft.revision() > 0 && (
        <details>
          <summary>
            {uiText("Saved result restored; compare current working file")}
          </summary>
          <pre>{view.result}</pre>
        </details>
      )}
      <div className="git-conflict-sides">
        {view.contents.map((text, index) => (
          <section key={index}>
            <h4>{view.labels[index]}</h4>
            <pre>{text || "(empty file)"}</pre>
          </section>
        ))}
      </div>
      <label>
        {uiText("Editable result")}
        <textarea
          aria-label={uiText("Conflict result")}
          spellCheck={false}
          disabled={!draft.loaded}
          value={draft.message}
          onChange={(e) => draft.change(e.target.value)}
        />
      </label>
      {chunks.map((chunk, index) => (
        <div className="git-actions" key={chunk.start}>
          <small>
            {uiText("Change ")}
            {index + 1}
          </small>
          {["current", "incoming", "both"].map((choice) => (
            <button
              key={choice}
              onClick={() =>
                draft.change(
                  draft.message.slice(0, chunk.start) +
                    (choice === "current"
                      ? chunk.current
                      : choice === "incoming"
                        ? chunk.incoming
                        : chunk.current + chunk.incoming) +
                    draft.message.slice(chunk.end),
                )
              }
            >
              {choice === "current"
                ? uiText("Use {value0}", {
                    value0: String(view.labels[1]!.split(" (")[0]),
                  })
                : choice === "incoming"
                  ? uiText("Use {value0}", {
                      value0: String(view.labels[2]!.split(" (")[0]),
                    })
                  : uiText("Use both")}
            </button>
          ))}
        </div>
      ))}
      <div className="git-actions">
        <button onClick={() => void perform(() => writeText(draft.message))}>
          {uiText("Copy result")}
        </button>
        <button
          disabled={!draft.loaded}
          onClick={() => void perform(() => draft.flush())}
        >
          {uiText("Save resolution draft")}
        </button>
        <button
          className="primary"
          disabled={
            !draft.loaded ||
            new TextEncoder().encode(draft.message).length > 65536 ||
            /^(?:<{7,}|={7,}|>{7,}|\|{7,})/m.test(draft.message)
          }
          onClick={() =>
            prepare({
              kind: "resolveText",
              path: view.path,
              expected: view.expected,
              content: draft.message,
            })
          }
        >
          {uiText("Preview save and stage resolution")}
        </button>
      </div>
      <small>
        {uiText(
          "Saving a draft does not change Git. Saving and staging replaces this file and resolves its index stages; continue the operation separately. Drafts are bound to these index stages and operation. The current working file is checked again before staging.",
        )}
      </small>
      {draft.loaded && draft.message === draft.baseline() && (
        <small role="status">{uiText("Resolution draft saved.")}</small>
      )}
      {draft.error && <p role="alert">{draft.error}</p>}
    </div>
  );
}
export default function AdvancedOperations({
  context,
  catalog,
  prepare,
  perform,
  flushRef,
  onTools,
}: Props) {
  const state = catalog.recovery;
  const [revision, setRevision] = useState(""),
    [mainline, setMainline] = useState(""),
    [upstream, setUpstream] = useState("");
  const [commits, setCommits] = useState<RebaseCommit[]>([]),
    [todo, setTodo] = useState<RebaseItem[]>([]),
    [plannedUpstream, setPlannedUpstream] = useState("");
  const [selected, setSelected] = useState(""),
    [view, setView] = useState<Conflict | null>(null),
    [loading, setLoading] = useState(false);
  const [pathFilter, setPathFilter] = useState("");
  const [pathPage, setPathPage] = useState(0);
  const filteredPaths = (state?.conflicts ?? []).filter((row) =>
    row.path.toLocaleLowerCase().includes(pathFilter.toLocaleLowerCase()),
  );
  const controller = useRef<AbortController | null>(null);
  useEffect(() => () => controller.current?.abort(), []);
  const stopped =
    !!state?.operation ||
    catalog.state.conflicts ||
    catalog.state.operations.length > 0;
  const readConflict = (path: string) =>
    void perform(async () => {
      await flushRef.current();
      controller.current?.abort();
      const request = new AbortController();
      controller.current = request;
      setLoading(true);
      try {
        const next = await gitApi.conflict(context, path, request.signal);
        if (!request.signal.aborted) {
          setSelected(path);
          setView(next);
        }
      } finally {
        if (!request.signal.aborted) setLoading(false);
      }
    });
  const move = (index: number, delta: number) =>
    setTodo((old) => {
      const next = [...old];
      const a = next[index],
        b = next[index + delta];
      if (!a || !b) return old;
      [next[index], next[index + delta]] = [b, a];
      return next;
    });
  return (
    <div className="git-advanced git-form">
      <section>
        <h3>
          {state?.operation
            ? uiText("Recover {value0}", { value0: String(state.operation) })
            : catalog.state.conflicts
              ? uiText("Resolve unmerged index")
              : uiText("Operation state")}
        </h3>
        <p>
          {state?.explanation ??
            uiText("Refresh to inspect actual Git operation state.")}
        </p>
        {state?.currentCommit && (
          <small>
            {uiText("Current operation commit: ")}
            {state.currentCommit}
          </small>
        )}
        {state?.onto && (
          <small>
            {uiText("Rebase destination: ")}
            {state.onto}
          </small>
        )}
        {state?.branch && (
          <small>
            {uiText("Original branch: ")}
            {state.branch}
          </small>
        )}
        <div className="git-actions">
          {state?.operation &&
            state.commands.map((command) => (
              <button
                className={
                  command === "continue"
                    ? "primary"
                    : command === "abort"
                      ? "danger"
                      : ""
                }
                key={command}
                onClick={() =>
                  prepare({
                    kind: "recover",
                    operation: state.operation!,
                    command,
                  })
                }
              >
                {uiText("Preview ")}
                {command}
              </button>
            ))}
          {onTools && (
            <button
              onClick={() =>
                void perform(async () => {
                  await flushRef.current();
                  await onTools();
                })
              }
            >
              {uiText("Open external recovery tools")}
            </button>
          )}
        </div>
        {stopped && !state?.commands.length && (
          <small>
            {uiText(
              "There is no supported sequencer command available. Resolve the index or recover with external Git, then refresh.",
            )}
          </small>
        )}
      </section>
      {!!state?.conflicts.length && (
        <section>
          <h3>{uiText("Unmerged paths")}</h3>
          <input
            aria-label={uiText("Filter unmerged paths")}
            placeholder={uiText("Filter conflicts…")}
            value={pathFilter}
            onChange={(event) => {
              setPathFilter(event.target.value);
              setPathPage(0);
            }}
          />
          <small>
            {filteredPaths.length}
            {uiText(" unmerged paths")}
          </small>
          <div className="git-actions">
            {filteredPaths
              .slice(pathPage * 100, pathPage * 100 + 100)
              .map((row) => (
                <button
                  key={row.path}
                  aria-current={selected === row.path}
                  onClick={() => readConflict(row.path)}
                >
                  {row.path}
                </button>
              ))}
          </div>
          <div className="git-actions">
            <button
              disabled={pathPage === 0}
              onClick={() => setPathPage((n) => n - 1)}
            >
              {uiText("Previous conflicts")}
            </button>
            <button
              disabled={(pathPage + 1) * 100 >= filteredPaths.length}
              onClick={() => setPathPage((n) => n + 1)}
            >
              {uiText("Next conflicts")}
            </button>
          </div>
          {loading && <p role="status">{uiText("Reading index stages…")}</p>}
          {view && (
            <>
              <h4>{view.path}</h4>
              {view.supported ? (
                <Resolution
                  key={view.expected}
                  view={view}
                  context={context}
                  flushRef={flushRef}
                  prepare={prepare}
                  perform={perform}
                />
              ) : (
                <p role="alert">{view.reason}</p>
              )}
            </>
          )}
        </section>
      )}
      {!stopped && (
        <>
          <section>
            <h3>{uiText("Merge, cherry-pick or revert")}</h3>
            <label>
              {uiText("Revision")}
              <input
                value={revision}
                onChange={(e) => setRevision(e.target.value)}
                placeholder={uiText("Branch or commit")}
              />
            </label>
            <label>
              {uiText("Mainline parent for a merge commit")}
              <input
                type="number"
                min="1"
                value={mainline}
                onChange={(e) => setMainline(e.target.value)}
                placeholder={uiText("Leave empty for ordinary commits")}
              />
            </label>
            <div className="git-actions">
              <button
                disabled={!revision}
                onClick={() => prepare({ kind: "merge", revision })}
              >
                {uiText("Preview merge")}
              </button>
              {(["cherryPick", "revert"] as const).map((kind) => (
                <button
                  key={kind}
                  disabled={!revision}
                  onClick={() =>
                    prepare({
                      kind,
                      revision,
                      mainline: mainline ? Number(mainline) : null,
                    })
                  }
                >
                  {kind === "cherryPick"
                    ? uiText("Preview cherry-pick")
                    : uiText("Preview revert")}
                </button>
              ))}
            </div>
          </section>
          <section>
            <h3>{uiText("Rebase current branch")}</h3>
            <p>
              {uiText(
                "Rebase rewrites local commit IDs. Start clean; other branches remain unchanged. Merge-containing ranges require external Git.",
              )}
            </p>
            <label>
              {uiText("Upstream revision")}
              <input
                value={upstream}
                onChange={(e) => setUpstream(e.target.value)}
                placeholder={uiText("main")}
              />
            </label>
            <div className="git-actions">
              <button
                disabled={!upstream}
                onClick={() => prepare({ kind: "rebase", upstream })}
              >
                {uiText("Preview rebase")}
              </button>
              <button
                disabled={!upstream}
                onClick={() =>
                  void perform(async () => {
                    controller.current?.abort();
                    const request = new AbortController();
                    controller.current = request;
                    const next = await gitApi.rebaseCommits(
                      context,
                      upstream,
                      request.signal,
                    );
                    if (!request.signal.aborted) {
                      setCommits(next);
                      setTodo(
                        next.map((c) => ({ oid: c.oid, command: "pick" })),
                      );
                      setPlannedUpstream(upstream);
                    }
                  })
                }
              >
                {uiText("Load interactive plan")}
              </button>
            </div>
            {!!todo.length && (
              <>
                <ol className="git-rebase-todo">
                  {todo.map((item, index) => (
                    <li key={item.oid}>
                      <div>
                        <code>{item.oid.slice(0, 10)}</code>{" "}
                        {commits.find((c) => c.oid === item.oid)?.subject}
                      </div>
                      <div className="git-actions">
                        <select
                          aria-label={uiText("Action for commit {value0}", {
                            value0: String(index + 1),
                          })}
                          value={item.command}
                          onChange={(e) =>
                            setTodo((old) =>
                              old.map((v, i) =>
                                i === index
                                  ? {
                                      ...v,
                                      command: e.target
                                        .value as RebaseItem["command"],
                                    }
                                  : v,
                              ),
                            )
                          }
                        >
                          {["pick", "drop", "fixup", "squash"].map(
                            (command) => (
                              <option key={command}>{command}</option>
                            ),
                          )}
                        </select>
                        <button
                          aria-label={uiText("Move commit {value0} earlier", {
                            value0: String(index + 1),
                          })}
                          disabled={index === 0}
                          onClick={() => move(index, -1)}
                        >
                          {uiText("Earlier")}
                        </button>
                        <button
                          aria-label={uiText("Move commit {value0} later", {
                            value0: String(index + 1),
                          })}
                          disabled={index === todo.length - 1}
                          onClick={() => move(index, 1)}
                        >
                          {uiText("Later")}
                        </button>
                      </div>
                    </li>
                  ))}
                </ol>
                <button
                  className="primary"
                  disabled={upstream !== plannedUpstream}
                  onClick={() =>
                    prepare({ kind: "rebaseInteractive", upstream, todo })
                  }
                >
                  {uiText("Preview interactive rebase")}
                </button>
                <small>
                  {uiText(
                    "Fixup/squash require an earlier kept commit. Squash keeps Git’s combined default message; reword/edit/exec and merge-topology editing use external Git. Reload after changing upstream.",
                  )}
                </small>
              </>
            )}
          </section>
        </>
      )}
    </div>
  );
}
