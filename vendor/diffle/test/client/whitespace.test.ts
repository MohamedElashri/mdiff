// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { MAX_MARKED_LINE, markWhitespace } from '@/client/review/whitespace.js';

/** A rendered row as the highlighter emits it: one span per token, a word-diff mark nested in one. */
function row(): HTMLElement {
  const el = document.createElement('div');
  el.dataset.line = '1';
  el.innerHTML =
    '<span data-char="0">\t  </span><span data-char="3">x = </span>' +
    '<span data-char="7">"a<span data-diff-span="">  b</span>"</span><span data-char="13">\t</span>';
  return el;
}

describe('markWhitespace', () => {
  it('marks indentation, multiple spaces, and tabs while preserving the original text', () => {
    const el = row();
    const text = el.textContent;
    markWhitespace(el);
    expect(el.textContent).toBe(text);
    expect(el.innerHTML).toBe(
      '<span data-char="0"><span data-ws="tab">\t</span><span data-ws="space">  </span></span>' +
        '<span data-char="3">x = </span>' +
        '<span data-char="7">"a<span data-diff-span=""><span data-ws="space">  </span>b</span>"</span>' +
        '<span data-char="13"><span data-ws="tab">\t</span></span>',
    );
  });

  it('marks trailing spaces and runs that cross token boundaries', () => {
    const el = document.createElement('div');
    el.innerHTML = '<span>a </span><span> b c</span><span> </span>';
    markWhitespace(el);
    expect(el.textContent).toBe('a  b c ');
    expect(el.children[0]!.querySelector('[data-ws]')?.textContent).toBe(' ');
    expect(el.children[1]!.innerHTML).toBe('<span data-ws="space"> </span>b c');
    expect(el.children[2]!.querySelector('[data-ws]')?.textContent).toBe(' ');
  });

  it('leaves a marked row as it is', () => {
    const el = row();
    markWhitespace(el);
    const html = el.innerHTML;
    markWhitespace(el);
    expect(el.innerHTML).toBe(html);
  });

  it('leaves a line longer than the viewer highlights unmarked', () => {
    const el = document.createElement('div');
    el.textContent = 'a '.repeat(MAX_MARKED_LINE);
    markWhitespace(el);
    expect(el.querySelector('[data-ws]')).toBeNull();
  });
});
