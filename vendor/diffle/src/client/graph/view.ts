import { Network } from 'lucide-react';
import type { ViewDef } from '@/client/views/state.js';
import { lspBlocker } from '@/shared/lsp.js';
import { GraphDetails } from './details/GraphDetails.js';
import { GRAPH_HELP } from './help.js';
import { GRAPH_PASSES, graphKeys, openFind } from './keys.js';
import { GraphView } from './GraphView.js';

/** The change graph: changed symbols and the code using them, which needs the language servers. */
export const graphView: ViewDef = {
  id: 'graph',
  label: 'Graph',
  icon: Network,
  title: 'Changed symbols and the code using them (M)',
  Main: GraphView,
  Side: GraphDetails,
  help: GRAPH_HELP,
  panelLabel: 'details panel',
  panelWidth: '21.25rem',
  blocked: (s) => lspBlocker(s.lsp, s.snapshot),
  enter: (scope) => scope.graph().enter(),
  leave: (scope) => scope.graph().leave(),
  place: (scope) => scope.graph().place(),
  keys: (e, scope) => graphKeys(e, scope.graph()),
  passes: GRAPH_PASSES,
  escape(inField, scope) {
    const graph = scope.graph().store.getState();
    // An open find closes first, even from its own field.
    if (graph.find != null) {
      graph.closeFind();
      return true;
    }
    if (inField) return false;
    graph.deselect();
    return true;
  },
  find: (scope) => openFind(scope.graph()),
  pickFile: (path, scope) => scope.graph().store.getState().focusFile(path),
};
