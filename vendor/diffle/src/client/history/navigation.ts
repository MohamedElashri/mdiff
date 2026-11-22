import type { EnterRequest, Review } from '@/shared/protocol.js';
export function matchesHistoryReview(request: EnterRequest, review?: Review): boolean {
  return request.kind === 'comparison'
    ? review?.kind === 'range' &&
        review.old === (request.old ?? ':empty-tree:') &&
        review.new === request.new &&
        review.base === request.base
    : request.kind === 'revspec' &&
        request.args.length === 1 &&
        review?.kind === 'commit' &&
        request.args[0] === `${review.sha}^!`;
}
/** An owned snapshot push can supersede its HTTP reply before hydration finishes. */
export function waitForHistoryReview(
  request: EnterRequest,
  current: () => Review | undefined,
  subscribe: (listener: (review?: Review) => void) => () => void,
  signal: AbortSignal,
): Promise<void> {
  signal.throwIfAborted();
  if (matchesHistoryReview(request, current())) return Promise.resolve();
  return new Promise((resolve, reject) => {
    let dispose = () => {};
    const finish = (error?: Error) => {
      clearTimeout(timer);
      dispose();
      signal.removeEventListener('abort', abort);
      if (error) reject(error);
      else resolve();
    };
    const abort = () => finish(new DOMException('Navigation cancelled', 'AbortError'));
    const timer = setTimeout(
      () => finish(new Error('The comparison changed before navigation completed. Try again.')),
      4000,
    );
    dispose = subscribe((review) => {
      if (matchesHistoryReview(request, review)) finish();
      else finish(new Error('Another comparison replaced this navigation.'));
    });
    signal.addEventListener('abort', abort, { once: true });
    if (signal.aborted) abort();
    else if (matchesHistoryReview(request, current())) finish();
  });
}
