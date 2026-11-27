import type { CodeViewHandle } from '@pierre/diffs/react';
import { watchRenderedRows } from './rows.js';

/**
 * Visible indentation, trailing spaces, runs of spaces, and tabs in rendered code.
 * The spans keep the original characters, so layout, selection, copying, and the text-node
 * walkers (search, occurrences, word navigation) see the same text as before.
 */

/** Set on the span around a run of spaces (`space`) or a single tab (`tab`). */
export const WHITESPACE_ATTR = 'data-ws';

/** Injected into the viewer's shadow DOM. Paints without taking room, so no row changes height or width. */
export const WHITESPACE_CSS = `
[data-ws] { --ws-color: color-mix(in srgb, var(--fg-2) 30%, transparent); }
/* One centered dot per character cell; in a monospace font a space is 1ch wide. */
[data-ws='space'] {
  background: radial-gradient(circle, var(--ws-color) 0.07em, transparent 0.09em) 0 50% / 1ch 1lh repeat-x;
}
[data-ws='tab'] { position: relative; }
[data-ws='tab']::before {
  content: '→'; position: absolute; left: 0; color: var(--ws-color); pointer-events: none; user-select: none;
}
`;

const RUN = / +|\t/g;

/** Longer lines stay unmarked, as the viewer leaves them unhighlighted (its `tokenizeMaxLineLength` default). */
export const MAX_MARKED_LINE = 1000;

/** Mark whitespace that can be difficult to distinguish in a diff. */
export function markWhitespace(row: Element): void {
  const line = row.textContent ?? '';
  // Interior single spaces are already visible as gaps. Avoid creating and painting a span
  // for each of them, which can dominate the cost of redrawing highlighted rows in WebKit.
  if (line.length > MAX_MARKED_LINE || !/^[ \t]|[ \t]$| {2,}|\t/.test(line)) return;

  const indentation = line.match(/^[ \t]*/)?.[0].length ?? 0;
  const trailing = line.length - (line.match(/[ \t]*$/)?.[0].length ?? 0);
  const walker = row.ownerDocument.createTreeWalker(row, NodeFilter.SHOW_TEXT);
  const pending: { text: Text; parts: (Node | string)[] }[] = [];
  let offset = 0;
  for (let text = walker.nextNode() as Text | null; text; text = walker.nextNode() as Text | null) {
    const start = offset;
    offset += text.length;
    if (text.parentElement?.hasAttribute(WHITESPACE_ATTR)) continue;

    const parts: (Node | string)[] = [];
    let end = 0;
    for (const { 0: value, index } of text.data.matchAll(RUN)) {
      const column = start + index;
      const tab = value === '\t';
      // Adjacent spaces can fall in different syntax or word-diff spans.
      const isolated = value.length === 1 && line[column - 1] !== ' ' && line[column + 1] !== ' ';
      if (!tab && isolated && column >= indentation && column < trailing) continue;
      if (index > end) parts.push(text.data.slice(end, index));
      const mark = row.ownerDocument.createElement('span');
      mark.setAttribute(WHITESPACE_ATTR, tab ? 'tab' : 'space');
      mark.textContent = value;
      parts.push(mark);
      end = index + value.length;
    }
    if (!parts.length) continue;
    if (end < text.length) parts.push(text.data.slice(end));
    pending.push({ text, parts });
  }
  for (const { text, parts } of pending) text.replaceWith(...parts);
}

/** The rows a mutation touched or added, including those in an item's shadow root as it mounts. */
function touchedRows(record: MutationRecord): Element[] {
  const el = record.target instanceof Element ? record.target : record.target.parentElement;
  const row = el?.closest('[data-line]');
  if (row) return [row];
  const rows: Element[] = [];
  for (const node of record.addedNodes) {
    if (!(node instanceof Element)) continue;
    if (node.matches('[data-line]')) rows.push(node);
    rows.push(...node.querySelectorAll('[data-line]'), ...(node.shadowRoot?.querySelectorAll('[data-line]') ?? []));
  }
  return rows;
}

export function installWhitespaceMarks(
  viewer: () => CodeViewHandle<unknown, undefined> | null,
  scroller: HTMLElement,
): () => void {
  // The frame pass discovers newly mounted roots. The observer marks changes inside known roots before paint.
  const marked = new WeakSet<Element>();
  const mark = (row: Element) => {
    markWhitespace(row);
    marked.add(row);
  };
  return watchRenderedRows(
    viewer,
    scroller,
    () => () => {},
    (items) => {
      for (const { root } of items)
        for (const row of root.querySelectorAll('[data-line]')) if (!marked.has(row)) mark(row);
    },
    (records) => {
      const rows = new Set<Element>();
      for (const record of records) {
        // Each replacement inserts a whitespace span. Ignore our own writes and process a
        // row once even if the highlighter replaced several of its tokens.
        if ([...record.addedNodes].some((node) => node instanceof Element && node.hasAttribute(WHITESPACE_ATTR)))
          continue;
        for (const row of touchedRows(record)) rows.add(row);
      }
      for (const row of rows) if (row.isConnected) mark(row);
    },
  );
}
