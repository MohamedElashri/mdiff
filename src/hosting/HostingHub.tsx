import { uiText } from "../../vendor/diffle/src/client/i18n/runtime";
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeft, ExternalLink, RefreshCw, X } from "lucide-react";
import {
  Dialog,
  DialogContent,
} from "../../vendor/diffle/src/client/ui/dialog";
import {
  hosting,
  sameReview,
  draftKey,
  type Site,
  type Context,
  type ReviewRef,
  type Details,
  type Summary,
  type Page,
  type Check,
  type Thread,
  type Note,
  type Job,
  type Action,
  type Receipt,
  type Write,
} from "./api";
import "./hosting.css";
const HostingMarkdown = lazy(() => import("./HostingMarkdown"));
function Body({ text }: { text: string }) {
  return (
    <Suspense fallback={<p className="hosting-prose">{text}</p>}>
      <HostingMarkdown text={text} />
    </Suspense>
  );
}
type Props = {
  id?: string;
  getDraft: (key: string) => string;
  setDraft: (key: string, value: string) => void;
  onOpen: (review: ReviewRef, title: string) => Promise<unknown>;
  onDiscover?: (items: Summary[]) => void;
  onClose: () => void;
  onConnections: () => void;
};
function ProviderLink({
  url,
  children = "Open on provider",
}: {
  url: string | null;
  children?: React.ReactNode;
}) {
  if (!url || !/^https:\/\//.test(url)) return null;
  return (
    <a
      href={url}
      onClick={(event) => {
        event.preventDefault();
        void openUrl(url);
      }}
    >
      {children}
      <ExternalLink size={12} />
    </a>
  );
}
export default function HostingHub(props: Props) {
  const [context, setContext] = useState<Context | null>(null);
  const [site, setSite] = useState<Site | null>(null);
  const [selected, setSelected] = useState<ReviewRef | null>(null);
  const [scope, setScope] = useState("requested");
  const [page, setPage] = useState(1);
  const [list, setList] = useState<Page<Summary> | null>(null);
  const [error, setError] = useState("");
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    let active = true;
    let generation = 0;
    const load = () => {
      const requestGeneration = ++generation;
      setContext(null);
      setSelected(null);
      setList(null);
      setError("");
      void hosting
        .context(props.id)
        .then((value) => {
          if (!active || requestGeneration !== generation) return;
          setContext(value);
          setSite(value.current?.review.site ?? value.sites[0] ?? null);
          setSelected(value.current?.review ?? null);
        })
        .catch(
          (e) =>
            active && requestGeneration === generation && setError(String(e)),
        );
    };
    load();
    const listener = listen("desktop-settings", load);
    return () => {
      active = false;
      void listener.then((stop) => stop());
    };
  }, [props.id]);
  const siteKey = site ? `${site.provider}:${site.host}` : "";
  useEffect(() => {
    if (!context || !site || selected) return;
    let active = true;
    const cancel = new AbortController();
    setList(null);
    setError("");
    void hosting
      .read<Page<Summary>>(
        site,
        { kind: "discover", scope, page },
        cancel.signal,
      )
      .then((v) => {
        if (active) {
          setList(v);
          props.onDiscover?.(v.items);
        }
      })
      .catch((e) => active && setError(String(e)));
    return () => {
      active = false;
      cancel.abort();
    };
  }, [siteKey, scope, page, selected, refresh, context?.epoch]);
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
    >
      <DialogContent className="hosting-hub" aria-labelledby="hosting-title">
        <header>
          <div>
            <h2 id="hosting-title">{uiText("Hosting reviews")}</h2>
            <p>{uiText("GitHub · GitLab")}</p>
          </div>
          <button
            aria-label={uiText("Close hosting reviews")}
            onClick={props.onClose}
          >
            <X size={18} />
          </button>
        </header>
        {error && <p role="alert">{error}</p>}
        {!context && !error && (
          <p role="status">{uiText("Loading hosting context…")}</p>
        )}
        {context && selected ? (
          <Detail
            key={JSON.stringify(selected)}
            {...props}
            review={selected}
            current={
              sameReview(context.current?.review, selected)
                ? context.current
                : null
            }
            onBack={() => setSelected(null)}
          />
        ) : (
          context && (
            <>
              <div className="hosting-controls">
                <label>
                  {uiText("Host")}
                  <select
                    aria-label={uiText("Hosting host")}
                    value={siteKey}
                    onChange={(e) => {
                      setSite(
                        context.sites.find(
                          (s) => `${s.provider}:${s.host}` === e.target.value,
                        ) ?? null,
                      );
                      setPage(1);
                    }}
                  >
                    {context.sites.map((s) => (
                      <option
                        key={`${s.provider}:${s.host}`}
                        value={`${s.provider}:${s.host}`}
                      >
                        {s.host} ({s.provider})
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  {uiText("Reviews")}
                  <select
                    value={scope}
                    onChange={(e) => {
                      setScope(e.target.value);
                      setPage(1);
                    }}
                  >
                    <option value="requested">
                      {uiText("Requested from me")}
                    </option>
                    <option value="authored">{uiText("Authored by me")}</option>
                  </select>
                </label>
                <button
                  aria-label={uiText("Refresh review list")}
                  onClick={() => setRefresh((n) => n + 1)}
                >
                  <RefreshCw size={15} />
                </button>
                <button onClick={props.onConnections}>
                  {uiText("Connections")}
                </button>
              </div>
              <div className="hosting-scroll">
                {list?.items.map((item) => (
                  <button
                    className="hosting-review-row"
                    key={JSON.stringify(item.review)}
                    onClick={() => setSelected(item.review)}
                  >
                    <span>{item.title}</span>
                    <small>
                      {item.review.repository} #{item.review.number} ·{" "}
                      {item.author}
                    </small>
                  </button>
                ))}
                {list?.items.length === 0 && (
                  <p>{uiText("No open reviews in this list.")}</p>
                )}
                {!list && !error && (
                  <p role="status">{uiText("Loading reviews…")}</p>
                )}
                {list?.incomplete && (
                  <p>{uiText("Provider search returned a partial result.")}</p>
                )}
              </div>
              <Pager page={page} next={list?.next ?? null} change={setPage} />
            </>
          )
        )}
      </DialogContent>
    </Dialog>
  );
}
function Pager({
  page,
  next,
  change,
}: {
  page: number;
  next: number | null;
  change: (page: number) => void;
}) {
  return (
    <nav aria-label={uiText("Hosting pagination")}>
      <button disabled={page <= 1} onClick={() => change(page - 1)}>
        {uiText("Previous")}
      </button>
      <span>
        {uiText("Page ")}
        {page}
      </span>
      <button
        disabled={!next || next > 100}
        onClick={() => next && change(next)}
      >
        {uiText("Next")}
      </button>
    </nav>
  );
}
function Detail({
  review,
  current,
  onBack,
  ...props
}: Props & {
  review: ReviewRef;
  current: Context["current"];
  onBack: () => void;
}) {
  const [details, setDetails] = useState<Details | null>(null);
  const [tab, setTab] = useState("overview");
  const [page, setPage] = useState(1);
  const [cursor, setCursor] = useState<(string | null)[]>([null]);
  const pageIdentity = useRef("");
  const [data, setData] = useState<Page<Check | Note | Thread> | null>(null);
  const [error, setError] = useState("");
  const [tick, setTick] = useState(0);
  const [revision, setRevision] = useState("");
  const [pending, setPending] = useState<Action | null>(null);
  const [busy, setBusy] = useState(false);
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const [verdict, setVerdict] = useState("comment");
  const [method, setMethod] = useState("");
  const key = draftKey(review);
  const [body, setBody] = useState(() => props.getDraft(key));
  const operationKey = draftKey(review, "operation");
  const [operation, setOperation] = useState<string>(() =>
    props.getDraft(operationKey),
  );
  const latest = useRef({ body, live: true });
  latest.current.body = body;
  useEffect(() => {
    latest.current.live = true;
    return () => {
      latest.current.live = false;
    };
  }, []);
  // A visible detail polls only when this native window has focus. No list/history-row polling.
  useEffect(() => {
    const timer = setInterval(() => {
      if (document.visibilityState === "visible" && document.hasFocus())
        setTick((n) => n + 1);
    }, 30000);
    return () => clearInterval(timer);
  }, []);
  useEffect(() => {
    let live = true;
    const cancel = new AbortController();
    void hosting
      .read<Details>(review.site, { kind: "details", review }, cancel.signal)
      .then((v) => {
        if (live) {
          setDetails(v);
          setError("");
        }
      })
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
      cancel.abort();
    };
  }, [review, tick]);
  const sha = revision || current?.sha || details?.head || "";
  useEffect(() => {
    const identity = `${sha}:${tab}:${page}`;
    if (pageIdentity.current !== identity) {
      setData(null);
      pageIdentity.current = identity;
    }
    if (!sha || tab === "overview") return;
    let live = true;
    const cancel = new AbortController();
    const kind =
      tab === "checks"
        ? "checks"
        : tab === "threads"
          ? "threads"
          : "conversation";
    void hosting
      .read<Page<Check | Note | Thread>>(
        review.site,
        {
          kind,
          review,
          ...(kind === "checks" ? { sha } : {}),
          page,
          ...(kind === "threads" ? { cursor: cursor[page - 1] ?? null } : {}),
        },
        cancel.signal,
      )
      .then((v) => {
        if (live) setData(v);
      })
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
      cancel.abort();
    };
  }, [review, sha, tab, page, tick]);
  const drift = !!current && !!details && current.sha !== details.head;
  const writable =
    !!details?.capabilities.comment &&
    !!current &&
    !!props.id &&
    !!details &&
    !drift &&
    ["open", "opened"].includes(details.state) &&
    !busy &&
    !operation;
  const send = async () => {
    if (!pending || !props.id || !current || !writable) return;
    const request: Write = {
      operation: crypto.randomUUID(),
      review,
      expectedHead: current.sha,
      action: pending,
    };
    setPending(null);
    setBusy(true);
    setError("");
    setOperation(request.operation);
    props.setDraft(operationKey, request.operation);
    try {
      const result = await hosting.write(props.id, request);
      if (!latest.current.live) return;
      setReceipt(result);
      if (result.status === "pending" || result.status === "unknown") {
        setOperation(result.operation);
        props.setDraft(operationKey, result.operation);
      }
      if (["succeeded", "rejected"].includes(result.status)) {
        setOperation("");
        props.setDraft(operationKey, "");
      }
      if (
        result.status === "succeeded" &&
        !(
          review.site.provider === "gitlab" &&
          request.action.kind === "submit" &&
          request.action.verdict === "approve"
        ) &&
        "body" in request.action &&
        request.action.body === latest.current.body &&
        request.action.kind !== "reply"
      ) {
        setBody("");
        props.setDraft(key, "");
      }
      if (result.status === "succeeded" && request.action.kind === "reply") {
        const replyKey = draftKey(review, `thread:${request.action.thread}`);
        if (props.getDraft(replyKey) === request.action.body)
          props.setDraft(replyKey, "");
      }
      setTick((n) => n + 1);
    } catch (e) {
      if (latest.current.live)
        setError(`${String(e)}. Reconcile this operation before trying again.`);
    } finally {
      if (latest.current.live) setBusy(false);
    }
  };
  const reconcile = async () => {
    setBusy(true);
    setError("");
    try {
      const result = await hosting.read<Receipt>(review.site, {
        kind: "reconcile",
        operation,
      });
      setReceipt(result);
      if (["succeeded", "rejected"].includes(result.status)) {
        setOperation("");
        props.setDraft(operationKey, "");
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const changeTab = (value: string) => {
    setTab(value);
    setPage(1);
    setCursor([null]);
    setData(null);
    setError("");
  };
  return (
    <>
      <div className="hosting-controls">
        <button onClick={onBack} disabled={busy}>
          <ArrowLeft size={14} />
          {uiText(" Reviews")}
        </button>
        <strong>
          {review.repository} #{review.number}
        </strong>
        <ProviderLink url={details?.url ?? null} />
        <button
          aria-label={uiText("Refresh hosting details")}
          disabled={busy}
          onClick={() => setTick((n) => n + 1)}
        >
          <RefreshCw size={14} />
        </button>
      </div>
      {error && <p role="alert">{error}</p>}
      {details ? (
        <>
          <div className="hosting-heading">
            <h3>{details.title}</h3>
            <span>
              {details.state}
              {details.draft ? uiText(" · Draft") : ""} · {details.author}
            </span>
            <code title={details.head ?? ""}>
              Provider head: {details.head?.slice(0, 12)}
            </code>
            {current && (
              <code title={current.sha}>
                Recorded review: {current.sha.slice(0, 12)}
              </code>
            )}
          </div>
          {drift && (
            <p role="alert">
              {uiText(
                "The provider head changed. Open the updated review and inspect it before submitting.",
              )}
            </p>
          )}
          {!current && (
            <button
              className="primary"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setError("");
                try {
                  await props.onOpen(review, details.title);
                } catch (e) {
                  setError(String(e));
                } finally {
                  if (latest.current.live) setBusy(false);
                }
              }}
            >
              {busy
                ? uiText("Opening review…")
                : uiText("Open review in a tab")}
            </button>
          )}
          <div
            className="hosting-tabs"
            role="tablist"
            aria-label={uiText("Hosting detail")}
          >
            {["overview", "checks", "conversation", "threads"].map((value) => (
              <button
                role="tab"
                aria-selected={tab === value}
                key={value}
                onClick={() => changeTab(value)}
              >
                {value[0]!.toUpperCase() + value.slice(1)}
              </button>
            ))}
          </div>
          <div className="hosting-scroll" role="tabpanel" aria-label={tab}>
            {tab === "overview" ? (
              <>
                <Body text={details.body || "No description."} />
                <p>
                  {uiText("Merge status:")}{" "}
                  {details.mergeStatus ||
                    uiText("Unknown — inspect on provider")}
                </p>
                <p>
                  {uiText(
                    "Protections, required checks, and approval eligibility are enforced by the provider.",
                  )}
                </p>
                {current && (
                  <section className="hosting-compose">
                    <label>
                      {uiText("Review message")}
                      <textarea
                        aria-label={uiText("Review message")}
                        value={body}
                        maxLength={32000}
                        disabled={busy}
                        onChange={(e) => {
                          setBody(e.target.value);
                          props.setDraft(key, e.target.value);
                        }}
                      />
                    </label>
                    <label>
                      {uiText("Submit as")}
                      <select
                        aria-label={uiText("Review verdict")}
                        value={verdict}
                        onChange={(e) => setVerdict(e.target.value)}
                      >
                        {details.capabilities.review.map((v) => (
                          <option key={v} value={v}>
                            {v === "requestChanges"
                              ? uiText("Request changes")
                              : v === "approve"
                                ? uiText("Approve")
                                : uiText("Comment")}
                          </option>
                        ))}
                      </select>
                    </label>
                    {review.site.provider === "gitlab" && (
                      <small>
                        {uiText(
                          "GitLab approval sends an approval for this SHA; its API does not attach the message. Choose Comment to publish the text.",
                        )}
                      </small>
                    )}
                    <button
                      disabled={!writable || !body.trim()}
                      onClick={() =>
                        setPending({ kind: "submit", verdict, body })
                      }
                    >
                      {uiText("Review submission…")}
                    </button>
                    <label>
                      {uiText("Merge method")}
                      <select
                        aria-label={uiText("Merge method")}
                        value={method}
                        onChange={(e) => setMethod(e.target.value)}
                      >
                        <option value="">{uiText("Select method")}</option>
                        {details.capabilities.mergeMethods.map((v) => (
                          <option key={v} value={v}>
                            {review.site.provider === "gitlab" && v === "merge"
                              ? uiText("Project default")
                              : v}
                          </option>
                        ))}
                      </select>
                    </label>
                    {!details.capabilities.mergeMethods.length && (
                      <small>
                        {uiText(
                          "No supported merge method is available for this account/project.",
                        )}
                      </small>
                    )}
                    <button
                      disabled={!writable || !method || details.draft}
                      onClick={() => setPending({ kind: "merge", method })}
                    >
                      {uiText("Review merge…")}
                    </button>
                  </section>
                )}
              </>
            ) : tab === "checks" ? (
              <>
                <label>
                  {uiText("CI revision")}
                  <select
                    aria-label={uiText("CI revision")}
                    value={sha}
                    onChange={(e) => {
                      setRevision(e.target.value);
                      setPage(1);
                    }}
                  >
                    <option value={current?.sha || details.head || ""}>
                      {uiText("Recorded/head")}{" "}
                      {(current?.sha || details.head)?.slice(0, 12)}
                    </option>
                    {details.mergeRevision &&
                      details.mergeRevision !==
                        (current?.sha || details.head) && (
                        <option value={details.mergeRevision}>
                          {uiText("Provider merge/pipeline revision")}{" "}
                          {details.mergeRevision.slice(0, 12)}
                        </option>
                      )}
                  </select>
                </label>
                <p>
                  {uiText("Exact SHA: ")}
                  <code>{sha}</code>
                  {uiText(
                    ". Paged observations; this is not a combined mergeability verdict.",
                  )}
                </p>
                {data?.warnings?.map((w, i) => (
                  <p role="status" key={i}>
                    {w}
                  </p>
                ))}
                {data?.items.map((value) => (
                  <CheckRow
                    key={(value as Check).id}
                    check={value as Check}
                    review={review}
                    sha={sha}
                  />
                ))}
              </>
            ) : tab === "conversation" ? (
              data?.items.map((note) => (
                <NoteRow key={(note as Note).id} note={note as Note} />
              ))
            ) : (
              data?.items.map((value) => (
                <ThreadRow
                  key={(value as Thread).id}
                  thread={value as Thread}
                  review={review}
                  writable={
                    writable && details.capabilities.reply && page <= 10
                  }
                  getDraft={props.getDraft}
                  setDraft={props.setDraft}
                  prepare={setPending}
                />
              ))
            )}
            {tab !== "overview" && !data && !error && (
              <p role="status">
                {uiText("Loading ")}
                {tab}…
              </p>
            )}
            {data?.items.length === 0 && (
              <p>
                {uiText(
                  "No items on this page. Use the provider link for unavailable CI or older discussions.",
                )}
              </p>
            )}
          </div>
          {tab !== "overview" && (
            <Pager
              page={page}
              next={data?.next ?? null}
              change={(n) => {
                if (n > page)
                  setCursor((old) => [
                    ...old.slice(0, page),
                    data?.cursor ?? null,
                  ]);
                setPage(n);
              }}
            />
          )}
          {operation && (
            <div role="status">
              <p>
                {uiText("Operation ")}
                {operation}
                {uiText(
                  " has a pending or unknown result. Reconcile before another write.",
                )}
              </p>
              <button disabled={busy} onClick={() => void reconcile()}>
                {uiText("Reconcile operation")}
              </button>
              <ProviderLink url={details.url} />
            </div>
          )}
          {receipt && (
            <div className="hosting-receipt" role="status">
              <strong>{receipt.status}</strong>
              <p>{receipt.message}</p>
              <code>Reviewed SHA: {receipt.reviewedSha}</code>
              {receipt.resultSha && (
                <code>Provider result SHA: {receipt.resultSha}</code>
              )}
              <ProviderLink url={receipt.url} />
            </div>
          )}
          {pending && (
            <div
              className="hosting-confirm"
              role="alertdialog"
              aria-labelledby="hosting-confirm-title"
            >
              <h3 id="hosting-confirm-title">
                {uiText("Confirm")}{" "}
                {pending.kind === "submit" ? pending.verdict : pending.kind}
              </h3>
              <p>
                {review.site.host}/{review.repository} #{review.number}
              </p>
              <code>Reviewed SHA: {current?.sha}</code>
              {"body" in pending && (
                <p className="hosting-prose">{pending.body}</p>
              )}
              {(pending.kind === "reply" || pending.kind === "resolve") && (
                <p>
                  {uiText("Thread: ")}
                  {pending.thread}
                  {pending.kind === "resolve"
                    ? ` · ${pending.resolved ? "Resolve" : "Reopen"}`
                    : uiText(" · Reply")}
                </p>
              )}
              {pending.kind === "merge" && (
                <p>
                  {uiText("Merge method: ")}
                  {pending.method}
                  {uiText(". The provider must accept the expected head.")}
                </p>
              )}
              {review.site.provider === "github" &&
                pending.kind === "submit" && (
                  <p>
                    {uiText(
                      "GitHub pins the review to this commit. A concurrent push can still happen after the final head check.",
                    )}
                  </p>
                )}
              <button onClick={() => setPending(null)}>
                {uiText("Cancel")}
              </button>
              <button
                className="primary"
                disabled={!writable}
                onClick={() => void send()}
              >
                {uiText("Confirm and submit")}
              </button>
            </div>
          )}
        </>
      ) : (
        !error && <p role="status">{uiText("Loading review details…")}</p>
      )}
    </>
  );
}
function NoteRow({ note }: { note: Note }) {
  const [expanded, setExpanded] = useState(false);
  return (
    <article className="hosting-note">
      <strong>{note.author}</strong>
      {note.sha && <code>{note.sha.slice(0, 12)}</code>}
      <Body text={expanded ? note.body : note.body.slice(0, 4000)} />
      {note.body.length > 4000 && (
        <button onClick={() => setExpanded((v) => !v)}>
          {expanded ? uiText("Collapse comment") : uiText("Read full comment")}
        </button>
      )}
      <ProviderLink url={note.url} />
    </article>
  );
}
function ThreadRow({
  thread,
  review,
  writable,
  getDraft,
  setDraft,
  prepare,
}: {
  thread: Thread;
  review: ReviewRef;
  writable: boolean;
  getDraft: Props["getDraft"];
  setDraft: Props["setDraft"];
  prepare: (a: Action) => void;
}) {
  const key = draftKey(review, `thread:${thread.id}`);
  const [body, setBody] = useState(() => getDraft(key));
  const [expanded, setExpanded] = useState(false);
  return (
    <article className="hosting-thread">
      <header>
        <code>
          {thread.path ?? "Discussion"}
          {thread.line ? `:${thread.line}` : ""}
        </code>
        <span>
          {thread.resolved ? uiText("Resolved") : uiText("Open")}
          {thread.outdated ? uiText(" · Outdated") : ""}
        </span>
      </header>
      {(thread.oldPath || thread.newPath) && (
        <small>
          {uiText("Original: ")}
          {thread.oldPath}:{thread.oldLine ?? "—"}
          {uiText(" · Revised:")} {thread.newPath}:{thread.newLine ?? "—"}
        </small>
      )}
      {thread.originalLine != null && (
        <small>
          {uiText("Original line: ")}
          {thread.originalLine} · {thread.side}
        </small>
      )}
      {thread.positionSha && (
        <code>Position revision: {thread.positionSha}</code>
      )}
      {(expanded ? thread.comments : thread.comments.slice(0, 1)).map(
        (note) => (
          <NoteRow key={note.id} note={note} />
        ),
      )}
      {thread.comments.length > 1 && (
        <button onClick={() => setExpanded((v) => !v)}>
          {expanded
            ? uiText("Hide replies")
            : uiText("Show {value0} replies", {
                value0: String(thread.comments.length - 1),
              })}
        </button>
      )}
      {thread.truncated && (
        <p>{uiText("More comments are available on the provider.")}</p>
      )}
      <label>
        {uiText("Reply")}
        <textarea
          aria-label={uiText("Reply to {value0}", {
            value0: String(thread.path ?? thread.id),
          })}
          value={body}
          maxLength={32000}
          onChange={(e) => {
            setBody(e.target.value);
            setDraft(key, e.target.value);
          }}
        />
      </label>
      <button
        disabled={!writable || !body.trim()}
        onClick={() =>
          prepare({
            kind: "reply",
            thread: thread.id,
            comment:
              thread.comments[0]?.id != null
                ? String(thread.comments[0].id)
                : null,
            body,
          })
        }
      >
        {uiText("Review reply…")}
      </button>
      {thread.canResolve && (
        <button
          disabled={!writable}
          onClick={() =>
            prepare({
              kind: "resolve",
              thread: thread.id,
              resolved: !thread.resolved,
            })
          }
        >
          {uiText("Review ")}
          {thread.resolved ? uiText("reopen") : uiText("resolution")}…
        </button>
      )}
    </article>
  );
}
function CheckRow({
  check,
  review,
  sha,
}: {
  check: Check;
  review: ReviewRef;
  sha: string;
}) {
  const [cancel] = useState(() => new AbortController());
  useEffect(() => () => cancel.abort(), [cancel]);
  const [jobs, setJobs] = useState<Page<Job> | null>(null);
  const [page, setPage] = useState(1);
  const [log, setLog] = useState<{ text: string; truncated: boolean } | null>(
    null,
  );
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const load = useCallback(
    async (n: number) => {
      setLoading(true);
      setError("");
      try {
        setJobs(
          await hosting.read<Page<Job>>(
            review.site,
            {
              kind: "jobs",
              review,
              sha,
              run: check.run,
              attempt: check.attempt,
              page: n,
            },
            cancel.signal,
          ),
        );
        setPage(n);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [review, sha, check.run, check.attempt],
  );
  return (
    <article className="hosting-check">
      <header>
        <strong>{check.name}</strong>
        <span>{check.status}</span>
      </header>
      <small>
        {check.kind}
        {check.attempt
          ? uiText(" · Attempt {value0}", { value0: String(check.attempt) })
          : ""}{" "}
        · {sha.slice(0, 12)}
      </small>
      <ProviderLink url={check.url} />
      {check.run && (
        <button disabled={loading} onClick={() => void load(1)}>
          {uiText("Load jobs")}
        </button>
      )}
      {error && <p role="alert">{error}</p>}
      {jobs?.items.map((job) => (
        <div className="hosting-job" key={job.id}>
          <strong>
            {job.name} · {job.status}
          </strong>
          <ProviderLink url={job.url} />
          {job.steps.map((s, i) => (
            <p key={i}>
              {s.name} · {s.status}
            </p>
          ))}
          <button
            disabled={loading}
            onClick={async () => {
              setLoading(true);
              try {
                setLog(
                  await hosting.read(
                    review.site,
                    {
                      kind: "log",
                      review,
                      sha,
                      job: job.id,
                    },
                    cancel.signal,
                  ),
                );
              } catch (e) {
                setError(String(e));
              } finally {
                setLoading(false);
              }
            }}
          >
            {uiText("Load log excerpt")}
          </button>
        </div>
      ))}
      {jobs && (
        <Pager page={page} next={jobs.next} change={(n) => void load(n)} />
      )}
      {log && (
        <>
          <p>
            {uiText("Log excerpt, at most 128 KiB")}
            {log.truncated ? uiText(" · Truncated") : ""}
          </p>
          <pre>{log.text}</pre>
        </>
      )}
    </article>
  );
}
