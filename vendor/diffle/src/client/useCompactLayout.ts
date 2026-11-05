import { useEffect, useState } from 'react';
import { useStore } from './store.js';
export function useCompactLayout() {
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
    [],
  );
  useEffect(() => {
    if (!compact) return;
    const key = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'b') {
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
  }, [compact, drawer]);
  return { compact, drawer, setDrawer };
}
