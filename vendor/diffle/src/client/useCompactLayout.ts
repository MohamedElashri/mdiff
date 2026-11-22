import { useReviewStore } from '@/client/store.js';
import { useEffect, useState } from 'react';

import { useCommands } from './commands/CommandProvider.js';
import { keyboardTarget } from './commands/shortcuts.js';
export function useCompactLayout() {
  const useStore = useReviewStore();

  const registry = useCommands();
  const [compact, setCompact] = useState(
    () => typeof matchMedia === 'function' && matchMedia('(max-width: 980px)').matches,
  );
  const [drawer, setDrawer] = useState<'tree' | 'panel' | null>(null);
  useEffect(() => {
    const mq = matchMedia('(max-width: 980px)');
    const update = () => {
      setCompact(mq.matches);
      setDrawer(null);
    };
    mq.addEventListener('change', update);
    return () => mq.removeEventListener('change', update);
  }, []);
  useEffect(
    () =>
      useStore.subscribe((next, prev) => {
        if (next.activePath !== prev.activePath) setDrawer((current) => (current === 'tree' ? null : current));
      }),
    [useStore],
  );
  useEffect(() => {
    if (!compact) return;
    const key = (e: KeyboardEvent) => {
      if (e.defaultPrevented || keyboardTarget(e).blocked) return;
      if (!registry && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'b') {
        e.preventDefault();
        e.stopImmediatePropagation();
        const pane = e.shiftKey ? 'panel' : 'tree';
        setDrawer((d) => (d === pane ? null : pane));
      }
      if (e.key === 'Escape' && drawer) {
        e.preventDefault();
        e.stopImmediatePropagation();
        setDrawer(null);
      }
    };
    window.addEventListener('keydown', key, true);
    return () => window.removeEventListener('keydown', key, true);
  }, [compact, drawer, registry]);
  return { compact, drawer, setDrawer };
}
