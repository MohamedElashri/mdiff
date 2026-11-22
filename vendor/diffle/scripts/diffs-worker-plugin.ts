import type { Plugin } from 'vite';

/**
 * @pierre/diffs 1.6.1 highlighter chunks import helpers from its worker entry.
 * In dev that entry can load both with Vite's worker query and as a plain module;
 * it can also load in the window. Only the first real worker entry may own RPC.
 * Keep this explicit guard until the pinned package separates entry side effects.
 */
export function guardDiffsWorkerEntry(code: string): string {
  const start = code.indexOf('self.addEventListener("error",');
  const end = code.indexOf('async function handleMessage(', start);
  if (start < 0 || end < start)
    throw new Error('Diffs worker entry changed; review the listener guard before updating.');
  const listeners = code.slice(start, end);
  if ((listeners.match(/self\.addEventListener\(/g) ?? []).length !== 2)
    throw new Error('Unexpected diffs worker listeners; review the listener guard.');
  return (
    code.slice(0, start) +
    `
// mdiff-worker-listeners:start
if (typeof WorkerGlobalScope !== 'undefined' && self instanceof WorkerGlobalScope &&
    !self[Symbol.for('mdiff.diffs.worker.listeners')]) {
  self[Symbol.for('mdiff.diffs.worker.listeners')] = true;
${listeners}}
// mdiff-worker-listeners:end
` +
    code.slice(end)
  );
}

export function diffsWorkerEntry(): Plugin {
  return {
    name: 'mdiff-diffs-worker-entry',
    enforce: 'pre',
    transform(code, id) {
      // Vite synthesizes these modules as Worker constructor/URL wrappers, not SDK source.
      const query = new URLSearchParams(id.split('?')[1] ?? '');
      if (['worker', 'sharedworker', 'raw', 'url'].some((key) => query.has(key))) return;
      if (!id.split('?')[0]?.replaceAll('\\', '/').endsWith('/@pierre/diffs/dist/worker/worker.js')) return;
      return { code: guardDiffsWorkerEntry(code), map: null };
    },
  };
}
