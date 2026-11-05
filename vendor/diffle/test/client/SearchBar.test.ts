// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

vi.mock('@/client/api.js', () => ({ api: {} }));
const { useStore } = await import('@/client/store.js');
const { SearchBar } = await import('@/client/keyboard/SearchBar.js');

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  useStore.setState((s) => ({
    activePath: 'a.txt',
    search: {
      ...s.search,
      open: false,
      query: '',
      input: '',
      matches: [],
      kind: 'text',
      scope: 'diff',
      content: { file: 'diff', diff: 'diff' },
    },
  }));
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  act(() =>
    root.render(
      createElement(
        'div',
        null,
        createElement('header', null, createElement(SearchBar)),
        createElement('section', null, createElement(SearchBar, { path: 'a.txt' })),
      ),
    ),
  );
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

it('keeps the local form mounted when toggling and remembers the global mode separately', () => {
  act(() => useStore.getState().openSearch('file'));
  const input = host.querySelector('section input');
  const select = () => host.querySelector<HTMLSelectElement>('[aria-label="Search content"]')!;
  const choose = (value: string) =>
    act(() => {
      select().value = value;
      select().dispatchEvent(new Event('change', { bubbles: true }));
    });
  choose('full');
  expect(host.querySelector('section input')).toBe(input);
  expect(host.querySelector('header input')).toBeNull();
  expect(select().value).toBe('full');
  expect(useStore.getState().search.path).toBe('a.txt');

  act(() => useStore.getState().openSearch('diff'));
  expect(host.querySelector('header input')).not.toBeNull();
  expect(select().value).toBe('diff');
  choose('removed');
  expect(host.querySelector('header input')).not.toBeNull();
  act(() => useStore.getState().openSearch('file'));
  expect(select().value).toBe('full');
  choose('both');
  act(() => useStore.getState().openSearch('diff'));
  expect(select().value).toBe('removed');
});
