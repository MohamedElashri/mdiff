import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { guardDiffsWorkerEntry, diffsWorkerEntry } from '../../scripts/diffs-worker-plugin.js';

const source = readFileSync(new URL('../../node_modules/@pierre/diffs/dist/worker/worker.js', import.meta.url), 'utf8');
const transformed = guardDiffsWorkerEntry(source);
const listeners = transformed.split('// mdiff-worker-listeners:start')[1]!.split('// mdiff-worker-listeners:end')[0]!;
const install = new Function('self', 'WorkerGlobalScope', 'handleMessage', listeners) as (
  self: object,
  scope: unknown,
  handle: (data: unknown) => void,
) => void;
class WorkerScope {
  handlers = new Map<string, ((event: { data: unknown }) => void)[]>();
  addEventListener(type: string, callback: (event: { data: unknown }) => void) {
    this.handlers.set(type, [...(this.handlers.get(type) ?? []), callback]);
  }
}

describe('pinned diff renderer worker entry', () => {
  it('does not attach worker RPC or worker error handlers to the browser window', () => {
    const window = new WorkerScope();
    install(window, undefined, () => {
      throw Error('Window RPC must never run');
    });
    expect(window.handlers.size).toBe(0);
    install({}, WorkerScope, () => {
      throw Error('Window RPC must never run');
    });
  });
  it('handles each message once when query and helper imports evaluate separate module copies', () => {
    const worker = new WorkerScope();
    const calls: unknown[] = [];
    install(worker, WorkerScope, (data) => calls.push(data));
    install(worker, WorkerScope, () => {
      throw Error('Duplicate entry used default render options');
    });
    expect(worker.handlers.get('message')).toHaveLength(1);
    expect(worker.handlers.get('error')).toHaveLength(1);
    worker.handlers.get('message')![0]!({ data: { type: 'initialize', theme: 'github-dark' } });
    expect(calls).toEqual([{ type: 'initialize', theme: 'github-dark' }]);
  });
  it('leaves Vite-generated Worker constructor and URL wrappers alone', () => {
    const transform = diffsWorkerEntry().transform;
    if (typeof transform !== 'function') throw Error('Expected the transform hook');
    for (const query of ['worker', 'sharedworker', 'url', 'raw', 'worker&inline']) {
      expect(
        Reflect.apply(transform, {}, [
          'export default function WorkerWrapper() {}',
          `/node_modules/@pierre/diffs/dist/worker/worker.js?${query}`,
        ]),
      ).toBeUndefined();
    }
  });
  it('fails visibly when a dependency update changes the entry boundary', () => {
    expect(() => guardDiffsWorkerEntry('export {};')).toThrow('entry changed');
  });
});
