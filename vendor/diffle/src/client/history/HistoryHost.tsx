import { useApi } from '@/client/api.js';
import { useReviewActive } from '@/client/runtime.js';
import { useReviewStore } from '@/client/store.js';
import { waitForHistoryReview } from './navigation.js';
import { lazy, Suspense, useEffect, useState, useRef } from 'react';
import { useCommands } from '@/client/commands/CommandProvider.js';

const Browser = lazy(() => import('./HistoryBrowser.js'));
export function HistoryHost() {
  const useStore = useReviewStore();

  const api = useApi();
  const enabled = useReviewActive();
  const registry = useCommands();
  const navigation = useRef<AbortController | null>(null);
  const close = () => {
    navigation.current?.abort();
    setOpen(false);
  };
  useEffect(() => () => navigation.current?.abort(), []);
  const [open, setOpen] = useState(false),
    [loaded, setLoaded] = useState(false);
  useEffect(
    () =>
      enabled
        ? registry?.register({
            'app.history': {
              run: () => {
                setLoaded(true);
                setOpen(true);
              },
            },
            'app.changes': { run: close },
          })
        : undefined,
    [registry, enabled],
  );
  return loaded ? (
    <div className="history-overlay" hidden={!open}>
      <Suspense fallback={<p>{'Loading history…'}</p>}>
        <Browser
          transport={api.request}
          active={open}
          onClose={close}
          onReview={async (request) => {
            navigation.current?.abort();
            const job = new AbortController();
            navigation.current = job;
            const result = await useStore.getState().enter(request);
            job.signal.throwIfAborted();
            if (typeof result === 'object') throw new Error(result.error);
            if (result === 'superseded')
              await waitForHistoryReview(
                request,
                () => useStore.getState().snapshot?.review,
                (listener) =>
                  useStore.subscribe((state, previous) => {
                    if (state.snapshot !== previous.snapshot) listener(state.snapshot?.review);
                  }),
                job.signal,
              );
            job.signal.throwIfAborted();
            setOpen(false);
          }}
        />
      </Suspense>
    </div>
  ) : null;
}
