import { type Direction, defaultGraph, type GraphController } from './state.js';

const STEPS: Record<string, Direction> = {
  h: 'left',
  ArrowLeft: 'left',
  l: 'right',
  ArrowRight: 'right',
  k: 'up',
  ArrowUp: 'up',
  j: 'down',
  ArrowDown: 'down',
};

const ZOOM_STEP = 1.25;
/** How soon a second `z` makes `zz`, as the review's chords wait. */
const CHORD_MS = 800;

/** Keys the graph leaves to the review: help, theme, the mode picker, and `M` back to the diff. */
export const GRAPH_PASSES = new Set(['?', 't', 'm', 'M', 'o']);

/**
 * Opens the graph's find, or takes the reader back to its field, its text selected to type over. Ctrl/⌘+F
 * opens it too: the browser's find cannot see boxes panned out of view.
 */
export function openFind(controller: GraphController = defaultGraph): void {
  const useGraph = controller.store;
  useGraph.getState().openFind();
  requestAnimationFrame(() =>
    controller.root()?.querySelector<HTMLInputElement>('input[aria-label="Find in the graph"]')?.select(),
  );
}

/** The graph view's own keys; true when `e` was one. The keymap calls it only while the graph shows. */
export function graphKeys(e: KeyboardEvent, controller: GraphController = defaultGraph): boolean {
  const useGraph = controller.store;
  const canvasControl = controller.canvas;
  const graph = useGraph.getState();
  const step = STEPS[e.key];
  if (step) {
    graph.step(step);
    return true;
  }
  switch (e.key) {
    case 'Enter': {
      const node = graph.view?.graph.nodes.find((n) => n.id === graph.selected);
      if (node) graph.pick({ path: node.path, line: node.line });
      return true;
    }
    case 's':
      graph.setFilter('symbols', !graph.filters.symbols);
      return true;
    case 'd':
      graph.setFilter('consumers', !graph.filters.consumers);
      return true;
    case 'T':
      graph.setFilter('tests', !graph.filters.tests);
      return true;
    case '/':
      openFind(controller);
      return true;
    case 'n':
    case 'N':
      if (graph.find == null) return false;
      graph.stepFind(e.key === 'n' ? 1 : -1);
      return true;
    case 'z':
      // `zz` does what a double-click does: frames the selection with its links, else fits the graph.
      if (e.repeat) return true;
      if (e.timeStamp - controller.keys.zAt > CHORD_MS) {
        controller.keys.zAt = e.timeStamp;
        return true;
      }
      controller.keys.zAt = -Infinity;
      if (graph.selected) canvasControl.frame(graph.selected);
      else canvasControl.fit();
      return true;
    case '0':
      canvasControl.fit();
      return true;
    case '+':
    case '=':
      canvasControl.zoom(ZOOM_STEP);
      return true;
    case '-':
      canvasControl.zoom(1 / ZOOM_STEP);
      return true;
    default:
      return false;
  }
}
