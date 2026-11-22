import { createHoverControl, hoverControl } from './lsp/hoverControl.js';
import { createContext, useContext, useRef, useMemo, useLayoutEffect, useEffect, type ReactNode } from 'react';
import { ApiContext, createApi } from './api.js';
import { createReviewStore, ReviewStoreContext, type StoreIdentity } from './store.js';
import { createViews, ViewsContext } from './views/state.js';
import { createGraph, GraphContext } from './graph/state.js';
import { createLspTarget, lspTarget } from './lsp/target.js';
import { createOccurrenceControl, occurrenceControl } from './lsp/occurrences.js';
import { createWordNavigation, defaultWordNavigation } from './lsp/wordNav.js';
import type { ReviewTransport } from './transport.js';

/** A live review's mutable frontend resources. No module-global transport rebinding. */
export function createReviewScope(transport: ReviewTransport, identity: StoreIdentity) {
  const lifetime = new AbortController();
  const subscriptions = new Set<() => void>();
  const scopedTransport: ReviewTransport = {
    async request(path, init) {
      lifetime.signal.throwIfAborted();
      const abort = new AbortController();
      const cancel = () => abort.abort();
      const signal = init?.signal;
      signal?.throwIfAborted();
      lifetime.signal.addEventListener('abort', cancel, { once: true });
      signal?.addEventListener('abort', cancel, { once: true });
      try {
        const response = await transport.request(path, { ...init, signal: abort.signal });
        abort.signal.throwIfAborted();
        return response;
      } finally {
        lifetime.signal.removeEventListener('abort', cancel);
        signal?.removeEventListener('abort', cancel);
      }
    },
    subscribe(message, ready, lost) {
      lifetime.signal.throwIfAborted();
      let live = true;
      const stop = transport.subscribe(
        (msg) => live && message(msg),
        () => live && ready(),
        () => live && lost(),
      );
      const dispose = () => {
        if (!live) return;
        live = false;
        subscriptions.delete(dispose);
        stop();
      };
      subscriptions.add(dispose);
      if (lifetime.signal.aborted) dispose();
      return dispose;
    },
  };
  const api = createApi(scopedTransport);
  const target = createLspTarget();
  const store = createReviewStore(api, identity, target);
  const views = createViews(store);
  const graph = createGraph(api, store, views);
  views.attach({ graph: () => graph });
  store.getState().registerView({
    here: () => views.activeView().place?.() ?? null,
    leave: () => views.showView('diff'),
    restore: (place) => graph.restore(place),
  });
  const occurrences = createOccurrenceControl(store);
  const words = createWordNavigation(store, target, occurrences);
  const hover = createHoverControl(store);
  const disposers = new Set<() => void>();
  let disposed = false;
  return {
    api,
    store,
    views,
    graph,
    target,
    occurrences,
    words,
    hover,
    identity,
    onDispose(dispose: () => void) {
      if (disposed) dispose();
      else disposers.add(dispose);
    },
    suspend() {
      hover.cancel();
      words.clearWordFocus();
      words.setViewer(() => null);
      occurrences.reset();
      target.set(null);
      target.focus(null);
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      lifetime.abort();
      for (const dispose of disposers) dispose();
      disposers.clear();
      for (const stop of subscriptions) stop();
      hover.cancel();
      words.clearWordFocus();
      words.setViewer(() => null);
      occurrences.reset();
      target.set(null);
      target.focus(null);
      graph.dispose();
      views.reset();
      store.dispose();
    },
    diagnostics: () => ({ subscriptions: subscriptions.size, disposed }),
  };
}
export type ReviewScope = ReturnType<typeof createReviewScope>;
const VisibilityContext = createContext(true);
const RootContext = createContext<React.RefObject<HTMLElement | null> | null>(null);
export const useReviewActive = () => useContext(VisibilityContext);
export function useReviewRoot(): Pick<ParentNode, 'querySelector' | 'querySelectorAll'> {
  const ref = useContext(RootContext);
  return useMemo(
    () => ({
      querySelector: <T extends Element = Element>(selector: string) =>
        (ref?.current ?? document).querySelector<T>(selector),
      querySelectorAll: <T extends Element = Element>(selector: string) =>
        (ref?.current ?? document).querySelectorAll<T>(selector),
    }),
    [ref],
  );
}
const DomContext = createContext({
  target: lspTarget,
  occurrences: occurrenceControl,
  words: defaultWordNavigation,
  hover: hoverControl,
});
export const useReviewDom = () => useContext(DomContext);

export function ReviewScopeProvider({
  scope,
  children,
  active = true,
  attachDom = true,
}: {
  scope: ReviewScope;
  children?: ReactNode;
  active?: boolean;
  attachDom?: boolean;
}) {
  const root = useRef<HTMLDivElement>(null);
  // Registry subscribers can rerender the desktop shell. Keep controller identity
  // stable so its children do not unregister/register commands on every shell render.
  const dom = useMemo(
    () => ({
      target: scope.target,
      occurrences: scope.occurrences,
      words: scope.words,
      hover: scope.hover,
    }),
    [scope],
  );
  useLayoutEffect(() => {
    if (attachDom) return scope.graph.bindRoot(() => root.current);
  }, [scope, attachDom]);
  useEffect(() => {
    if (!active) scope.suspend();
  }, [scope, active]);
  return (
    <VisibilityContext value={active}>
      <RootContext value={root}>
        <div ref={root} data-review-scope style={{ display: 'contents' }}>
          <ApiContext value={scope.api}>
            <ReviewStoreContext value={scope.store}>
              <ViewsContext value={scope.views}>
                <GraphContext value={scope.graph}>
                  <DomContext value={dom}>{children}</DomContext>
                </GraphContext>
              </ViewsContext>
            </ReviewStoreContext>
          </ApiContext>
        </div>
      </RootContext>
    </VisibilityContext>
  );
}
