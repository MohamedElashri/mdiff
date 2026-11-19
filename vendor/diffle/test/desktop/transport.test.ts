import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { request, reviewTransport } from '@/client/transport.js';
// oxlint-disable-next-line no-restricted-imports -- Desktop adapter lives outside the engine alias.
import { bindReview } from '../../../../src/transport';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

const encoder = new TextEncoder();
function packet(status: number, contentType: string, body: Uint8Array) {
  const header = encoder.encode(JSON.stringify({ status, contentType }));
  const bytes = new Uint8Array(4 + header.length + body.length);
  new DataView(bytes.buffer).setUint32(0, header.length, true);
  bytes.set(header, 4);
  bytes.set(body, 4 + header.length);
  return bytes.buffer;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(listen).mockReset();
  bindReview('review-one');
});

it('decodes native text, JSON, empty replies, and binary images', async () => {
  vi.mocked(invoke)
    .mockResolvedValueOnce(packet(200, 'text/plain', encoder.encode('diff\n+über')))
    .mockResolvedValueOnce(packet(409, 'application/json', encoder.encode('{"error":"Review changed"}')))
    .mockResolvedValueOnce(packet(204, 'application/octet-stream', new Uint8Array()))
    .mockResolvedValueOnce(packet(200, 'image/png', Uint8Array.of(0, 255, 128, 10)));
  await expect((await request('api/patch')).text()).resolves.toBe('diff\n+über');
  const error = await request('api/snapshot');
  expect(error).toMatchObject({ ok: false, status: 409 });
  await expect(error.json()).resolves.toEqual({ error: 'Review changed' });
  const empty = await request('api/threads', { method: 'DELETE' });
  expect(empty).toMatchObject({ ok: true, status: 204 });
  await expect(empty.text()).resolves.toBe('');
  const image = await (await request('api/image')).blob();
  expect(image.type).toBe('image/png');
  expect(new Uint8Array(await image.arrayBuffer())).toEqual(Uint8Array.of(0, 255, 128, 10));
  expect(vi.mocked(invoke).mock.calls.every(([, args]) => args && !('requestId' in args && args.requestId))).toBe(true);
});

it('rejects truncated native packets', async () => {
  vi.mocked(invoke).mockResolvedValueOnce(new ArrayBuffer(3));
  vi.mocked(invoke).mockResolvedValueOnce(Uint8Array.of(8, 0, 0, 0).buffer);
  await expect(request('api/snapshot')).rejects.toThrow('Invalid native response');
  await expect(request('api/snapshot')).rejects.toThrow('Invalid native response');
});

it('scopes cancellation to the bound review and preserves the abort reason', async () => {
  const abort = new AbortController();
  const native = deferred<ArrayBuffer>();
  vi.mocked(invoke).mockReturnValueOnce(native.promise).mockResolvedValueOnce(undefined);
  const result = request('api/patch', { signal: abort.signal });
  const [, args] = vi.mocked(invoke).mock.calls[0]!;
  expect(args).toMatchObject({ id: 'review-one', requestId: expect.any(String), path: '/api/patch' });
  bindReview('review-two');
  abort.abort();
  expect(invoke).toHaveBeenLastCalledWith('cancel_review_request', {
    id: 'review-one',
    requestId: (args as { requestId: string }).requestId,
  });
  native.reject('Request cancelled');
  await expect(result).rejects.toBe(abort.signal.reason);
  await expect(request('api/patch', { signal: abort.signal })).rejects.toBe(abort.signal.reason);
  expect(invoke).toHaveBeenCalledTimes(2);
});

it('tears down a native listener once even when registration finishes late', async () => {
  const registration = deferred<() => void>();
  vi.mocked(listen).mockReturnValueOnce(registration.promise);
  const ready = vi.fn();
  const lost = vi.fn();
  const close = reviewTransport()!.subscribe(vi.fn(), ready, lost);
  close();
  const unlisten = vi.fn();
  registration.resolve(unlisten);
  await registration.promise;
  close();
  expect(unlisten).toHaveBeenCalledOnce();
  expect(ready).not.toHaveBeenCalled();
  expect(lost).not.toHaveBeenCalled();
});
