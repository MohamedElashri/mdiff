import type { LucideIcon } from 'lucide-react';
import type { ComponentType } from 'react';
import { create, useStore as useZustand } from 'zustand';
import { createContext, useContext } from 'react';
import type { GraphController } from '@/client/graph/state.js';
import { type LayoutState, type ReviewState, defaultStore, type ReviewStore, type ViewPlace } from '@/client/store.js';

export type ViewId = 'diff' | 'graph';

type HelpRow = [keys: string, action: string];
export type HelpSection = { title: string; rows: HelpRow[] };

/**
 * A view of the review: what it shows in the main pane and in the side panel, and how it takes part
 * in the keys, the tree and the jump list while it is shown. The diff is the base the others cover:
 * jumps land in it, and leaving another view returns to it.
 */
export interface ViewDef {
  id: ViewId;
  label: string;
  icon: LucideIcon;
  /** The switch's tooltip, with the key that shows the view. */
  title: string;
  Main: ComponentType;
  Side: ComponentType;
  /** Header controls that act on this view alone. */
  Controls?: ComponentType;
  /** Names the side panel, for its toggle. */
  panelLabel: string;
  /**
   * The shortcuts that work while the view shows, one array of sections per help column. Every key
   * appears once; each row renders on one line, so descriptions stay short.
   */
  help: HelpSection[][];
  /** The side panel's width until the reader drags it, as a CSS length. */
  panelWidth: string;
  /** Why the view cannot be shown now, or null. */
  blocked?(s: ReviewState): string | null;
  enter?(scope: ViewScope): void;
  leave?(scope: ViewScope): void;
  /** Where the reader is in the view, for the jump list; Ctrl+o after a jump into the diff restores it. */
  place?(scope: ViewScope): ViewPlace;
  /** The view's own keys; true when `e` was one. Unless `passes` holds it, any other key does nothing. */
  keys?(e: KeyboardEvent, scope: ViewScope): boolean;
  passes?: ReadonlySet<string>;
  /** Escape within the view; true when it used the key. `inField` when a text field has focus. */
  escape?(inField: boolean, scope: ViewScope): boolean;
  /** Ctrl/⌘+F: the view's own find, in place of the browser's. */
  find?(scope: ViewScope): void;
  /** The reader picked `path` in the file tree again while the view shows. */
  pickFile?(path: string, scope: ViewScope): void;
}

interface ViewsState {
  active: ViewId;
  /** Shown at least once: a view stays mounted under the active one, so it keeps its scroll, pan and zoom. */
  mounted: ViewId[];
}

export interface ViewScope {
  graph(): GraphController;
}
export function createViews(review: ReviewStore = defaultStore) {
  const useViews = create<ViewsState>(() => ({ active: 'diff', mounted: ['diff'] }));
  let scope: ViewScope;
  const bind = (view: ViewDef) => ({
    ...view,
    enter: view.enter ? () => view.enter!(scope) : undefined,
    leave: view.leave ? () => view.leave!(scope) : undefined,
    place: view.place ? () => view.place!(scope) : undefined,
    keys: view.keys ? (e: KeyboardEvent) => view.keys!(e, scope) : undefined,
    escape: view.escape ? (field: boolean) => view.escape!(field, scope) : undefined,
    find: view.find ? () => view.find!(scope) : undefined,
    pickFile: view.pickFile ? (path: string) => view.pickFile!(path, scope) : undefined,
  });
  const activeView = () => bind(viewOf(useViews.getState().active));
  const showView = (id: ViewId) => {
    const { active, mounted } = useViews.getState();
    if (id === active) return;
    const next = bind(viewOf(id));
    const blocked = next.blocked?.(review.getState());
    if (blocked) return review.getState().flash(blocked);
    bind(viewOf(active)).leave?.();
    useViews.setState({ active: id, mounted: mounted.includes(id) ? mounted : [...mounted, id] });
    next.enter?.();
  };
  return {
    store: useViews,
    activeView,
    showView,
    toggleView: (id: ViewId) => showView(useViews.getState().active === id ? 'diff' : id),
    attach(value: ViewScope) {
      scope = value;
    },
    reset() {
      useViews.setState({ active: 'diff', mounted: ['diff'] });
    },
  };
}
export type ViewController = ReturnType<typeof createViews>;
export const defaultViews = createViews();
export const ViewsContext = createContext<ViewController | null>(null);
export const useViewActions = () => useContext(ViewsContext) ?? defaultViews;
export const useReviewViews = () => useViewActions().store;
export const useViews = Object.assign(function useViews<T>(selector: (state: ViewsState) => T): T {
  return useZustand(useReviewViews(), selector);
}, defaultViews.store);

const defs = new Map<ViewId, ViewDef>();

/** Registers the views, in the switch's order. The app does once, at startup, before anything renders. */
export function defineViews(views: ViewDef[]): void {
  for (const view of views) defs.set(view.id, view);
}

export function listViews(): ViewDef[] {
  return [...defs.values()];
}

function viewOf(id: ViewId): ViewDef {
  const view = defs.get(id);
  if (!view) throw new Error(`view ${id} is not defined`);
  return view;
}

export const activeView = () => defaultViews.activeView();
export function useActiveView() {
  const views = useViewActions();
  useViews((s) => s.active);
  return views.activeView();
}
export const showView = (id: ViewId) => defaultViews.showView(id);

/** Shows `id`, or the diff again when `id` already shows. */
export function toggleView(id: ViewId): void {
  defaultViews.toggleView(id);
}

const NO_PANEL = { width: null, visible: true };

export function panelOf(layout: LayoutState, view: ViewId): { width: number | null; visible: boolean } {
  return layout.panels[view] ?? NO_PANEL;
}

/** The page grid's columns: the tree, the main pane, and the side panel of `view`. */
export function gridColumns(layout: LayoutState, view: ViewId): string {
  const panel = panelOf(layout, view);
  const length = (px: number | null, fallback: string) => (px == null ? fallback : `${px}px`);
  return [
    layout.treeVisible ? `${length(layout.treeWidth, '17.5rem')} 4px` : '',
    'minmax(0, 1fr)',
    panel.visible ? `4px ${length(panel.width, defs.get(view)?.panelWidth ?? '21.25rem')}` : '',
  ]
    .filter(Boolean)
    .join(' ');
}
