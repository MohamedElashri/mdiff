import { uiText } from '@/client/i18n/runtime.js';
import { useReviewGraph } from '@/client/graph/state.js';
import { FlaskConical, FolderOpen, LayersArrowUp, LoaderCircle, RefreshCw, SquareFunction, X } from 'lucide-react';
import type { ReactNode } from 'react';
import { Button, TOGGLE_ON } from '@/client/ui/Button.js';
import { useStore } from '@/client/store.js';
import { GraphCanvas } from './GraphCanvas.js';
import { type Filter } from './state.js';

/** The change graph in the diff's place: what it holds, the view filters, and the canvas. */
export function GraphView() {
  const useGraph = useReviewGraph();

  /** Shows the side panel's list of what the graph left out: it stands where a selection's details do. */
  function revealCut() {
    useGraph.getState().deselect();
    useStore.getState().setPanel('graph', { visible: true });
  }

  const load = useGraph((s) => s.load);
  const view = useGraph((s) => s.view);
  const filters = useGraph((s) => s.filters);
  const scope = useGraph((s) => s.scope);
  const expanding = useGraph((s) => s.expanding);
  const selected = useGraph((s) => s.selected);
  const graph = view?.graph;
  const changed = graph?.nodes.filter((n) => n.status !== 'consumer').length ?? 0;
  const full = load.state === 'ready' ? load.graph : null;
  const loading = load.state === 'loading';
  return (
    <section className="flex h-full min-h-0 flex-col bg-canvas" aria-label={uiText('Change graph view')}>
      <div className="flex flex-wrap items-center gap-2.5 border-b border-border bg-surface px-3 py-1.5 text-[0.8125rem]">
        {graph && (
          <span className="text-muted">
            {changed}
            {uiText(' changed · ')}
            {graph.nodes.length - changed}
            {uiText(' using them · ')}
            {graph.edges.length}
            {uiText(' links')}
          </span>
        )}
        {scope && (
          <Button
            variant="ghost"
            className={TOGGLE_ON}
            onClick={() => useGraph.getState().showScope(null)}
            title={uiText('Graph every change again')}
          >
            <FolderOpen size="0.8125rem" /> {scope}/ <X size="0.75rem" />
          </Button>
        )}
        {full?.cut.length ? (
          <button
            type="button"
            className="cursor-pointer border-0 bg-transparent p-0 text-warn hover:underline"
            onClick={revealCut}
            title={uiText('List them in the side panel')}
          >
            {full.cut.length}
            {uiText(' changed symbols not shown')}
          </button>
        ) : null}
        {full?.inconclusive && (
          <span className="text-warn" title={full.inconclusive}>
            {uiText('may be incomplete')}
          </span>
        )}
        <span className="flex-1" />
        {selected && graph?.nodes.some((node) => node.id === selected && node.status !== 'consumer') && (
          <Button
            variant="ghost"
            disabled={expanding}
            onClick={() => void useGraph.getState().expand(selected)}
            title={uiText('Find code using this symbol')}
          >
            {expanding ? uiText('Finding uses…') : uiText('Find uses')}
          </Button>
        )}
        {expanding && !selected && (
          <span className="text-muted" role="status">
            {uiText('Finding dependents…')}
          </span>
        )}
        <div className="flex items-center gap-1" role="group" aria-label={uiText('Show in the graph')}>
          <FilterChip filter="symbols" title={uiText('One box per symbol instead of per file (s)')}>
            <SquareFunction size="0.8125rem" />
            {uiText(' Symbols')}
          </FilterChip>
          <FilterChip filter="consumers" title={uiText('Unchanged code using the change (d)')}>
            <LayersArrowUp size="0.8125rem" />
            {uiText(' Dependents')}
          </FilterChip>
          <FilterChip filter="tests" title={uiText('Tests (T)')}>
            <FlaskConical size="0.8125rem" />
            {uiText(' Tests')}
          </FilterChip>
        </div>
        <Button
          variant="ghost"
          icon
          disabled={loading}
          onClick={() => void useGraph.getState().reload()}
          title={loading ? uiText('Loading') : uiText('Reload')}
          aria-label={uiText('Reload')}
        >
          {loading ? (
            <LoaderCircle size="0.875rem" className="animate-spin text-accent" />
          ) : (
            <RefreshCw size="0.875rem" />
          )}
        </Button>
      </div>
      <div className="min-h-0 flex-1">
        {loading && !view && (
          <p className="flex items-center gap-2 p-4 text-muted">
            <LoaderCircle size="0.875rem" className="animate-spin text-accent" />
            {uiText(' Loading')}
          </p>
        )}
        {load.state === 'error' && <p className="p-4 text-danger">{load.message}</p>}
        {view && graph!.nodes.length === 0 && (
          <p className="p-4 text-muted">{uiText('No changed functions, classes or types in this diff.')}</p>
        )}
        {view && graph!.nodes.length > 0 && <GraphCanvas view={view} clustered={filters.symbols} />}
      </div>
    </section>
  );
}

/**
 * One on/off filter, as the search bar's toggles are: on is accent-tinted and outlined, off is
 * muted text. Each stands alone, unlike the either/or segmented controls beside it.
 */
function FilterChip({ filter, title, children }: { filter: Filter; title: string; children: ReactNode }) {
  const useGraph = useReviewGraph();

  const on = useGraph((s) => s.filters[filter]);
  return (
    <Button
      variant="ghost"
      aria-pressed={on}
      title={title}
      onClick={() => useGraph.getState().setFilter(filter, !on)}
      className={on ? TOGGLE_ON : 'text-muted hover:text-foreground'}
    >
      {children}
    </Button>
  );
}
