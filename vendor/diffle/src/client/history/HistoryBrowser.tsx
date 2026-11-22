import { uiText, formatDate } from '@/client/i18n/runtime.js';
import { useCommands } from '@/client/commands/CommandProvider.js';
import { useEffect, useMemo, useRef, useState } from 'react';
import { GitCommitHorizontal, ChevronLeft, ChevronRight, GitBranch, RefreshCw } from 'lucide-react';
import { Button } from '@/client/ui/Button.js';
import { translate } from '@/client/i18n/runtime.js';
import type {
  EnterRequest,
  HistoryDetail,
  HistoryInfo,
  HistoryPage,
  HistoryRow,
  HistoryTree,
  HistoryQuery,
} from '@/shared/protocol.js';
import { historyApi, type HistoryTransport } from './api.js';
import { historyDate } from './dates.js';
import { historyWindow } from './window.js';
import { mergeFolds } from './folds.js';
import { historyMessages as m } from './messages.js';
const EMPTY: HistoryQuery = { root: 'HEAD', search: '', author: '', since: null, until: null };
const ROW = 36;
export default function HistoryBrowser({
  transport,
  initialInfo,
  active = true,
  onReview,
  onAgentReview,
  onClose,
  onChooseRepository,
  agentCommits = {},
}: {
  agentCommits?: Record<string, string>;
  transport?: HistoryTransport;
  initialInfo?: HistoryInfo;
  active?: boolean;
  onReview(request: EnterRequest, root: string): Promise<unknown>;
  onAgentReview?(request: EnterRequest, root: string): Promise<unknown>;
  onClose(): void;
  onChooseRepository?(): void;
}) {
  const registry = useCommands();
  useEffect(() => (active ? registry?.suspendReview() : undefined), [registry, active]);
  const api = useMemo(() => historyApi(transport), [transport]);
  const [info, setInfo] = useState<HistoryInfo | null>(initialInfo ?? null),
    [query, setQuery] = useState(EMPTY),
    [draft, setDraft] = useState(EMPTY);
  const [pages, setPages] = useState<HistoryPage[]>([]),
    [selected, setSelected] = useState<string[]>([]),
    [detail, setDetail] = useState<HistoryDetail | null>(null),
    [parent, setParent] = useState('');
  const [error, setError] = useState(''),
    [busy, setBusy] = useState(false),
    [changed, setChanged] = useState(false),
    [expired, setExpired] = useState(false);
  const [scroll, setScroll] = useState(0),
    [height, setHeight] = useState(500),
    [lines, setLines] = useState(true),
    [zoom, setZoom] = useState(10),
    [color, setColor] = useState(''),
    [fold, setFold] = useState(false),
    [expanded, setExpanded] = useState<Set<string>>(new Set());
  const infoLatest = useRef(info);
  infoLatest.current = info;
  const generation = useRef<string | null>(null);
  const viewport = useRef<HTMLDivElement>(null),
    sequence = useRef(0),
    pageJob = useRef<AbortController | null>(null),
    mounted = useRef(true);
  const [highlightAuthor, setHighlightAuthor] = useState('');
  const [reload, setReload] = useState(0);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      pageJob.current?.abort();
      if (generation.current) void api.close(generation.current).catch(() => {});
    };
  }, [api]);
  useEffect(() => {
    const el = viewport.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setHeight(el.clientHeight));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (!active) {
      pageJob.current?.abort();
      if (generation.current) void api.close(generation.current).catch(() => {});
      setExpired(true);
      setBusy(false);
      return;
    }
    const abort = new AbortController();
    void api
      .info(abort.signal)
      .then((value) => {
        if (infoLatest.current && infoLatest.current.stamp !== value.stamp) setChanged(true);
        setInfo((previous) => previous ?? value);
      })
      .catch((e) => {
        if (!abort.signal.aborted) setError(String(e));
      });
    return () => abort.abort();
  }, [api, active]);
  useEffect(() => {
    const abort = new AbortController();
    const id = ++sequence.current;
    pageJob.current?.abort();
    pageJob.current = abort;
    setBusy(true);
    setError('');
    setExpired(false);
    setPages([]);
    setSelected([]);
    setDetail(null);
    setScroll(0);
    setExpanded(new Set());
    if (viewport.current) viewport.current.scrollTop = 0;
    const token = crypto.randomUUID();
    generation.current = token;
    void api
      .begin(query, abort.signal, token)
      .then((page) => {
        if (!abort.signal.aborted && id === sequence.current) setPages([page]);
      })
      .catch((e) => {
        if (!abort.signal.aborted) setError(String(e));
      })
      .finally(() => {
        if (!abort.signal.aborted) setBusy(false);
      });
    return () => {
      abort.abort();
      void api.close(token).catch(() => {});
    };
  }, [api, query, reload]);
  const selectedSha = selected.at(-1);
  useEffect(() => {
    setDetail(null);
    if (!selectedSha) return;
    const abort = new AbortController();
    void api
      .detail(selectedSha, abort.signal)
      .then((value) => {
        if (!abort.signal.aborted) {
          setDetail(value);
          setParent(value.parents[0] ?? '');
        }
      })
      .catch((e) => {
        if (!abort.signal.aborted) setError(String(e));
      });
    return () => abort.abort();
  }, [api, selectedSha]);
  const raw = useMemo(() => pages.flatMap((page) => page.rows), [pages]);
  const filtered = !!(query.search || query.author || query.since != null || query.until != null);
  const folds = useMemo(
    () =>
      fold && !filtered
        ? mergeFolds(raw, pages[0]?.page === 0 && !info?.refsTruncated && !pages.some((page) => page.notice))
        : new Map<string, Set<string>>(),
    [raw, fold, filtered, pages, info],
  );
  const hidden = useMemo(
    () => new Set([...folds].flatMap(([sha, ids]) => (expanded.has(sha) ? [] : [...ids]))),
    [folds, expanded],
  );
  const rows = raw.filter((row) => !hidden.has(row.sha));
  const from = Math.max(0, Math.floor(scroll / ROW) - 8),
    to = Math.min(rows.length, Math.ceil((scroll + height) / ROW) + 8);
  const load = async (direction: -1 | 1) => {
    if (busy || !pages.length) return;
    const edge = direction === 1 ? pages.at(-1)! : pages[0]!;
    const number = direction === 1 ? edge.next : edge.page - 1;
    if (number == null || number < 0) return;
    const abort = new AbortController();
    pageJob.current?.abort();
    pageJob.current = abort;
    const id = sequence.current;
    setBusy(true);
    setError('');
    try {
      const page = await api.page(edge.generation, number, abort.signal);
      if (abort.signal.aborted || id !== sequence.current || page.generation !== edge.generation) return;
      const window = historyWindow(direction === 1 ? [...pages, page] : [page, ...pages], direction);
      const evictedRows = pages
        .filter((old) => old.page < window[0]!.page)
        .reduce((sum, page) => sum + page.rows.length, 0);
      setPages(window);
      const evicted = evictedRows > 0;
      if (viewport.current) {
        const target =
          direction === -1
            ? 0
            : evicted
              ? Math.max(0, viewport.current.scrollTop - evictedRows * ROW)
              : viewport.current.scrollTop;
        viewport.current.scrollTop = target;
        setScroll(target);
      }
    } catch (e) {
      if (!abort.signal.aborted) {
        setError(String(e));
        setExpired(true);
      }
    } finally {
      if (!abort.signal.aborted && mounted.current) setBusy(false);
    }
  };
  const refresh = async () => {
    try {
      const value = await api.info();
      setInfo(value);
      setChanged(false);
      setReload((value) => value + 1);
    } catch (e) {
      setError(String(e));
    }
  };
  const compare = async (kind: string, agent = false) => {
    if (!info || !detail || busy) return;
    setBusy(true);
    setError('');
    const abort = new AbortController();
    pageJob.current = abort;
    try {
      const commits =
        kind === 'commit'
          ? [detail.sha]
          : kind === 'series'
            ? raw.filter((row) => selected.includes(row.sha)).map((row) => row.sha)
            : selected;
      const request = await api.compare(kind, commits, parent || null, abort.signal);
      if (!abort.signal.aborted) await (agent && onAgentReview ? onAgentReview : onReview)(request, info.root);
    } catch (e) {
      if (!abort.signal.aborted && !(e instanceof DOMException && e.name === 'AbortError')) setError(String(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };
  const pick = (row: HistoryRow, add: boolean) => {
    setSelected((current) =>
      add
        ? current.includes(row.sha)
          ? current.filter((sha) => sha !== row.sha)
          : [...current, row.sha].slice(-250)
        : [row.sha],
    );
  };
  return (
    <section className="history-browser" data-history-surface aria-label={translate(m.title)}>
      <header className="history-toolbar">
        <Button onClick={onClose}>
          <ChevronLeft size={15} />
          {translate(m.changes)}
        </Button>
        {onChooseRepository && (
          <Button onClick={onChooseRepository} disabled={busy}>
            {uiText('Browse another repository')}
          </Button>
        )}
        <strong>
          <GitBranch size={16} />
          {translate(m.title)}
        </strong>
        <span className="history-root" title={info?.root}>
          {info?.root.split('/').pop()}
        </span>
        <Button onClick={() => void refresh()} disabled={busy}>
          <RefreshCw size={14} />
          {translate(m.refresh)}
        </Button>
      </header>
      <form
        className="history-filters"
        onSubmit={(e) => {
          e.preventDefault();
          setQuery({ ...draft });
        }}
      >
        <label>
          {uiText('Root')}
          <select
            aria-label={uiText('History root')}
            value={draft.root}
            onChange={(e) => setDraft({ ...draft, root: e.target.value })}
          >
            <option value="HEAD">{uiText('HEAD')}</option>
            <option value="all">{uiText('All visible branches and tags')}</option>
            {info?.refs.map((ref) => (
              <option key={ref.name} value={ref.name}>
                {ref.name.replace(/^refs\//, '')}
              </option>
            ))}
          </select>
        </label>
        <input
          aria-label={translate(m.search)}
          placeholder={translate(m.search)}
          maxLength={128}
          value={draft.search}
          onChange={(e) => setDraft({ ...draft, search: e.target.value })}
        />
        <input
          aria-label={translate(m.author)}
          placeholder={translate(m.author)}
          maxLength={128}
          value={draft.author}
          onChange={(e) => setDraft({ ...draft, author: e.target.value })}
        />
        <label>
          {uiText('Since')}
          <input
            type="date"
            onChange={(e) =>
              setDraft({
                ...draft,
                since: historyDate(e.target.value),
              })
            }
          />
        </label>
        <label>
          {uiText('Until')}
          <input
            type="date"
            onChange={(e) =>
              setDraft({
                ...draft,
                until: historyDate(e.target.value, true),
              })
            }
          />
        </label>
        <Button type="submit" disabled={busy}>
          {translate(m.apply)}
        </Button>
      </form>
      <div className="history-options">
        <label>
          <input type="checkbox" checked={lines} onChange={(e) => setLines(e.target.checked)} />
          {uiText('Ancestry lines')}
        </label>
        <label>
          {uiText('Lane spacing')}
          <input
            aria-label={uiText('Lane spacing')}
            type="range"
            min="8"
            max="20"
            value={zoom}
            onChange={(e) => setZoom(Number(e.target.value))}
          />
        </label>
        <label>
          {uiText('Line color')}
          <input
            aria-label={uiText('Ancestry line color')}
            type="color"
            value={color || '#4d79a5'}
            onChange={(e) => setColor(e.target.value)}
          />
        </label>
        <Button onClick={() => setColor('')}>{uiText('Theme colors')}</Button>
        <label>
          <input type="checkbox" checked={fold} onChange={(e) => setFold(e.target.checked)} />
          {uiText('Fold verified merge branches')}
        </label>
      </div>
      <p className="history-notice" role="status">
        {changed ? translate(m.changed) : translate(m.info)}{' '}
        {info?.shallow && 'Shallow clone: parents may be unavailable.'}{' '}
        {info?.partial && 'Partial clone: missing objects are not fetched automatically.'}{' '}
        {(filtered || hidden.size > 0) && translate(m.filtered)} {pages.at(-1)?.notice}{' '}
        {pages[0]?.page ? uiText('Earlier ancestry is outside this window; use Earlier page.') : ''}{' '}
        {pages.at(-1)?.next != null ? uiText('Ancestry continues beyond the loaded rows.') : ''}
      </p>
      {error && (
        <p className="history-error" role="alert">
          {error} <Button onClick={() => void refresh()}>{uiText('Reload history')}</Button>
        </p>
      )}
      <div className="history-body">
        <div className="history-list-column">
          <div className="history-paging">
            <Button disabled={busy || expired || !pages[0]?.page} onClick={() => void load(-1)}>
              <ChevronLeft size={14} />
              {uiText('Earlier page')}
            </Button>
            <span>
              {raw.length}
              {uiText(' retained · ')}
              {pages.at(-1)?.scanned ?? 0}
              {uiText(' examined')}
            </span>
            <Button disabled={busy || expired || pages.at(-1)?.next == null} onClick={() => void load(1)}>
              {uiText('Continue')}
              <ChevronRight size={14} />
            </Button>
            {expired && <Button onClick={() => void refresh()}>{uiText('Reload to continue')}</Button>}
          </div>
          <div
            className="history-list"
            ref={viewport}
            role="listbox"
            aria-label={uiText('Commits')}
            aria-multiselectable="true"
            aria-busy={busy}
            onScroll={(e) => setScroll(e.currentTarget.scrollTop)}
          >
            <div style={{ height: rows.length * ROW, position: 'relative' }}>
              {rows.slice(from, to).map((row, index) => {
                const at = from + index;
                const graphWidth =
                  Math.min(33, Math.max(2, ...row.edges.flatMap((e) => [e.from + 2, e.to + 2]), (row.lane ?? 32) + 2)) *
                  zoom;
                return (
                  <div
                    key={row.sha}
                    role="option"
                    aria-selected={selected.includes(row.sha)}
                    tabIndex={selectedSha === row.sha || (!selectedSha && at === 0) ? 0 : -1}
                    className="history-row"
                    data-author-highlight={row.author === highlightAuthor}
                    style={{ top: at * ROW, height: ROW }}
                    onClick={(e) => pick(row, e.metaKey || e.ctrlKey)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        pick(row, e.metaKey || e.ctrlKey);
                      }
                      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
                        e.preventDefault();
                        const target = rows[at + (e.key === 'ArrowDown' ? 1 : -1)];
                        if (target) {
                          pick(target, false);
                          const top = (at + (e.key === 'ArrowDown' ? 1 : -1)) * ROW;
                          viewport.current?.scrollTo({ top: Math.max(0, top - height / 2) });
                        }
                      }
                    }}
                  >
                    <svg
                      width={graphWidth}
                      height={ROW}
                      aria-label={
                        row.boundary
                          ? uiText('Ancestry continues outside the visible graph')
                          : uiText('Commit ancestry')
                      }
                    >
                      {lines &&
                        !filtered &&
                        !hidden.size &&
                        row.edges.map((edge, i) => (
                          <path
                            key={i}
                            d={`M ${(edge.from + 1) * zoom} ${edge.from === row.lane ? ROW / 2 : 0} L ${(edge.to + 1) * zoom} ${ROW}`}
                            stroke={color || `var(--history-lane-${String(edge.color % 6)})`}
                            fill="none"
                            strokeWidth="1.5"
                          />
                        ))}
                      {lines && !filtered && !hidden.size && row.incoming && (
                        <path
                          d={`M ${((row.lane ?? 0) + 1) * zoom} 0 V ${ROW / 2}`}
                          stroke={color || 'var(--accent)'}
                          fill="none"
                          strokeWidth="1.5"
                        />
                      )}
                      <circle cx={((row.lane ?? 32) + 1) * zoom} cy={ROW / 2} r="3" fill={color || 'var(--accent)'} />
                      {row.boundary && (
                        <text x={((row.lane ?? 32) + 1) * zoom - 3} y="28">
                          ⋮
                        </text>
                      )}
                    </svg>
                    <span className="history-subject">
                      <strong>{row.subject || uiText('(no subject)')}</strong>
                      {row.refs.map((ref) => (
                        <small key={ref}>{ref.replace(/^refs\/(heads|remotes|tags)\//, '')}</small>
                      ))}
                      {folds.has(row.sha) && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            setExpanded((value) => {
                              const next = new Set(value);
                              if (next.has(row.sha)) next.delete(row.sha);
                              else next.add(row.sha);
                              return next;
                            });
                          }}
                        >
                          {expanded.has(row.sha)
                            ? uiText('Fold')
                            : uiText('Expand {value0} branch commits', { value0: String(folds.get(row.sha)!.size) })}
                        </button>
                      )}
                    </span>
                    <span
                      className="history-author"
                      title={uiText('Click to highlight this author; double-click to filter')}
                      onClick={(e) => {
                        e.stopPropagation();
                        setHighlightAuthor(row.author);
                      }}
                      onDoubleClick={() => {
                        setDraft({ ...draft, author: row.author });
                        setQuery({ ...query, author: row.author });
                      }}
                    >
                      {row.author}
                    </span>
                    <time
                      dateTime={
                        Number.isFinite(new Date(row.date * 1000).getTime())
                          ? new Date(row.date * 1000).toISOString()
                          : undefined
                      }
                    >
                      {formatDate(new Date(row.date * 1000))}
                    </time>
                    <code>{row.sha.slice(0, 8)}</code>
                  </div>
                );
              })}
            </div>
            {!rows.length && <p className="history-empty">{busy ? uiText('Loading history…') : translate(m.empty)}</p>}
          </div>
        </div>
        <aside className="history-detail" aria-label={uiText('Commit details')}>
          {detail ? (
            <>
              <h2>{detail.message.split('\n', 1)[0] || uiText('(no subject)')}</h2>
              <code>{detail.sha}</code>
              {agentCommits[detail.sha] && <p className="history-agent-badge">{agentCommits[detail.sha]}</p>}
              <p>
                {detail.author} ·{' '}
                {formatDate(new Date(detail.date * 1000), { dateStyle: 'medium', timeStyle: 'short' })}
              </p>
              <pre>{detail.message}</pre>
              <label>
                {uiText('Compare with parent')}
                <select aria-label={uiText('Commit parent')} value={parent} onChange={(e) => setParent(e.target.value)}>
                  {!detail.parents.length && <option value="">{uiText('Empty tree (root commit)')}</option>}
                  {detail.parents.map((sha, i) => (
                    <option key={sha} value={sha}>
                      {uiText('Parent ')}
                      {i + 1}: {sha.slice(0, 12)}
                    </option>
                  ))}
                </select>
              </label>
              <Button disabled={busy} onClick={() => void compare('commit')}>
                <GitCommitHorizontal size={15} />
                {detail.parents.length > 1 ? uiText('Compare merge with parent') : uiText('Review this commit')}
              </Button>
              {onAgentReview && (
                <Button disabled={busy} onClick={() => void compare('commit', true)}>
                  {uiText('Review commit with agent')}
                </Button>
              )}
              <p>
                {selected.length}
                {uiText(' selected. Ctrl/Command-click adds commits; selection order sets the two endpoints.')}
              </p>
              <Button disabled={busy || selected.length !== 2} onClick={() => void compare('direct')}>
                {uiText('Compare first vs second')}
              </Button>
              <Button disabled={busy || selected.length !== 2} onClick={() => void compare('merge-base')}>
                {uiText('Compare from merge base')}
              </Button>
              <Button
                disabled={busy || selected.length < 2 || selected.some((sha) => !raw.some((row) => row.sha === sha))}
                onClick={() => void compare('series')}
              >
                {uiText('Review contiguous first-parent series')}
              </Button>
              <p>
                {uiText(
                  'Endpoint comparison includes changes between two trees. A series includes its oldest selected commit; noncontiguous selections are rejected.',
                )}
              </p>
              <h3>{uiText('Files at this commit')}</h3>
              <Tree api={api} oid={detail.tree} />
            </>
          ) : (
            <p>{uiText('Select a commit to read its message, parents and files.')}</p>
          )}
        </aside>
      </div>
    </section>
  );
}
function Tree({ api, oid }: { api: ReturnType<typeof historyApi>; oid: string }) {
  const [tree, setTree] = useState<HistoryTree | null>(null),
    [error, setError] = useState('');
  const [open, setOpen] = useState<Set<string>>(new Set());
  useEffect(() => {
    const abort = new AbortController();
    setTree(null);
    setError('');
    setOpen(new Set());
    void api
      .tree(oid, abort.signal)
      .then((value) => {
        if (!abort.signal.aborted) setTree(value);
      })
      .catch((e) => {
        if (!abort.signal.aborted) setError(String(e));
      });
    return () => abort.abort();
  }, [api, oid]);
  return (
    <ul className="history-tree">
      {error && <li role="alert">{error}</li>}
      {tree?.entries.map((entry) => (
        <li key={entry.name}>
          {entry.kind === 'tree' ? (
            <>
              <button
                aria-expanded={open.has(entry.oid)}
                onClick={() =>
                  setOpen((current) => {
                    const next = new Set(current);
                    if (next.has(entry.oid)) next.delete(entry.oid);
                    else next.add(entry.oid);
                    return next;
                  })
                }
              >
                {open.has(entry.oid) ? '▾' : '▸'} {entry.name}
              </button>
              {open.has(entry.oid) && <Tree api={api} oid={entry.oid} />}
            </>
          ) : (
            <span>
              {entry.name}
              {entry.kind === 'commit' ? uiText(' (submodule)') : ''}
            </span>
          )}
        </li>
      ))}
      {tree?.truncated && <li>{uiText('Only the first 512 entries are shown.')}</li>}
    </ul>
  );
}
