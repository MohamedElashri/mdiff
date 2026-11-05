import { test } from 'node:test';
import assert from 'node:assert/strict';
import { UpdateController } from '../src/update-controller.ts';

function fixture(options = {}) {
  const calls = [];
  const update = {
    version: '0.2.0', body: '- A new feature',
    async download(progress) {
      calls.push('download');
      progress({ event: 'Started', data: { contentLength: 100 } });
      progress({ event: 'Progress', data: { chunkLength: 40 } });
      progress({ event: 'Progress', data: { chunkLength: 60 } });
      if (options.downloadError) throw new Error('Invalid signature');
    },
    async install() { calls.push('install'); if (options.installError) throw new Error('Permission denied'); },
    async close() { calls.push('close'); },
  };
  const controller = new UpdateController({
    async info() { calls.push('info'); return { version: '0.1.0', supported: options.supported ?? true, reason: 'Use an AppImage' }; },
    async check() { calls.push('check'); if (options.checkError) throw new Error('Offline'); return options.current ? null : update; },
    async restart() { calls.push('restart'); if (options.restartError) throw new Error('Restart failed'); },
  });
  return { controller, calls, update };
}

test('does not check, download, install, or restart without user actions', async () => {
  const { controller: c, calls } = fixture();
  c.subscribe(() => {});
  assert.deepEqual(calls, []);
  await c.install(); await c.restart();
  assert.deepEqual(calls, []);
  await c.check();
  assert.equal(c.getSnapshot().phase, 'available');
  assert.deepEqual(calls, ['info', 'check']);
  const states = [];
  c.subscribe(() => states.push(c.getSnapshot()));
  await c.install();
  assert.equal(c.getSnapshot().phase, 'ready');
  assert.ok(states.some(s => s.phase === 'installing'));
  assert.ok(states.some(s => s.downloaded === 100));
  assert.equal(calls.includes('restart'), false);
  await c.restart();
  assert.equal(calls.at(-1), 'restart');
});
test('deduplicates rapid clicks and preserves progress when subscribers close', async () => {
  const { controller: c, calls } = fixture();
  const stop = c.subscribe(() => {});
  await Promise.all([c.check(), c.check()]);
  assert.equal(calls.filter(v => v === 'check').length, 1);
  stop();
  await Promise.all([c.install(), c.install(), c.check()]);
  assert.equal(calls.filter(v => v === 'download').length, 1);
  assert.equal(c.getSnapshot().phase, 'ready');
});
test('reports no update without downloading', async () => {
  const { controller: c, calls } = fixture({ current: true });
  await c.check(); await c.install();
  assert.equal(c.getSnapshot().phase, 'current');
  assert.deepEqual(calls, ['info', 'check']);
});
test('unsupported installations never contact the update endpoint', async () => {
  const { controller: c, calls } = fixture({ supported: false });
  await c.check();
  assert.deepEqual(calls, ['info']);
  assert.match(c.getSnapshot().error, /AppImage/);
});
test('check failures can be retried', async () => {
  const { controller: c, calls } = fixture({ checkError: true });
  await c.check(); await c.check();
  assert.equal(c.getSnapshot().phase, 'idle');
  assert.match(c.getSnapshot().error, /Offline/);
  assert.equal(calls.filter(v => v === 'check').length, 2);
});
test('failed signature or download prevents installation and restart', async () => {
  const { controller: c, calls } = fixture({ downloadError: true });
  await c.check(); await c.install(); await c.restart();
  assert.equal(c.getSnapshot().phase, 'available');
  assert.match(c.getSnapshot().error, /Invalid signature/);
  assert.equal(calls.includes('install'), false);
  assert.equal(calls.includes('restart'), false);
});
test('failed installation stays retryable without restarting', async () => {
  const { controller: c, calls } = fixture({ installError: true });
  await c.check(); await c.install(); await c.restart();
  assert.equal(c.getSnapshot().phase, 'available');
  assert.match(c.getSnapshot().error, /Permission denied/);
  assert.equal(calls.includes('restart'), false);
});
test('failed restart retains the installed state', async () => {
  const { controller: c } = fixture({ restartError: true });
  await c.check(); await c.install(); await c.restart();
  assert.equal(c.getSnapshot().phase, 'ready');
  assert.match(c.getSnapshot().error, /Restart failed/);
});
test('rechecking releases the previous update resource', async () => {
  const { controller: c, calls } = fixture();
  await c.check(); await c.check();
  assert.deepEqual(calls, ['info', 'check', 'info', 'close', 'check']);
});
