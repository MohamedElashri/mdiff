import { expect, it, vi } from 'vitest';
// oxlint-disable-next-line no-restricted-imports -- Exercise the desktop's cached provider source.
import { hostingEntries } from '../../../../src/hosting/palette';
import { CommandRegistry } from '@/client/commands/registry';
const review = { site: { provider: 'github' as const, host: 'enterprise.test' }, repository: 'team/repo', number: 7 };
const row = {
  review,
  title: 'Fix review ordering',
  author: 'ada',
  body: '',
  state: 'open',
  head: null,
  url: 'https://enterprise.test/team/repo/pull/7',
};
it('ranks a bounded discovery page without provider I/O and opens its exact host/target', () => {
  const open = vi.fn();
  const entries = hostingEntries(
    Array.from({ length: 100 }, (_, i) => ({ ...row, title: `Change ${i}` })),
    '',
    () => false,
    open,
  );
  expect(entries).toHaveLength(20);
  entries[0]!.run();
  expect(open).toHaveBeenCalledWith(review, 'Change 0');
  expect(hostingEntries([row], 'enterprise.test', () => true, open)[0]!.reason).toContain('Wait');
  expect(hostingEntries([row], 'enterprise.test', () => true, open)[0]!.run()).toBe(false);
  expect(open).toHaveBeenCalledTimes(1);
  expect(hostingEntries([], 'fix', () => false, open)).toEqual([]);
});
it('does not advertise a hosting group in standalone registries without a desktop source', () => {
  const registry = new CommandRegistry();
  expect(registry.hasSourceGroup('hosting')).toBe(false);
  const remove = registry.registerSource({
    id: 'desktop.hosting',
    group: 'hosting',
    open: () => ({ search: () => [] }),
  });
  expect(registry.hasSourceGroup('hosting')).toBe(true);
  remove();
  expect(registry.hasSourceGroup('hosting')).toBe(false);
});
