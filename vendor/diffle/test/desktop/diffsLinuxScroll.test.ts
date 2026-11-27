import { readFileSync } from 'node:fs';
import { describe, expect, it, vi } from 'vitest';
import { bufferLinuxDiffScroll } from '../../../../scripts/diffs-linux-scroll.js';

const installed = readFileSync(
  new URL('../../../../node_modules/@pierre/diffs/dist/components/CodeView.js', import.meta.url),
  'utf8',
);
const patched = bufferLinuxDiffScroll(installed);
const handlers = patched.slice(patched.indexOf('\thandleScroll ='), patched.indexOf('\tclearPendingScroll ='));
const createViewer = new Function(
  'globalThis',
  `class CodeView { static __STOP = false; ${handlers} }
  return new CodeView();`,
) as (platform: { isTauri: boolean; navigator: { platform: string } }) => {
  handleScroll(): void;
  getLinuxScrollOverscroll(top: number, height: number): number;
};

function fixture(platform = 'Linux x86_64', isTauri = true) {
  return Object.assign(createViewer({ isTauri, navigator: { platform } }), {
    renderState: { firstIndex: 3, lastIndex: 3, stickyTop: 200, stickyBottom: 1600 },
    config: { overscrollSize: 200 },
    items: [{ top: 0, height: 3200 }],
    heightDirty: false,
    layoutDirtyIndex: undefined as number | undefined,
    pendingLayoutAnchor: undefined as object | undefined,
    pendingScrollTarget: undefined as object | undefined,
    scrollAnimation: undefined as object | undefined,
    scrollDirty: false,
    shouldRebaseScroll: vi.fn(() => false),
    getHeight: vi.fn(() => 680),
    getItemTopOffset: vi.fn(() => 0),
    getScrollTop: vi.fn(() => 450),
    getScrollHeight: vi.fn(() => 5000),
    suspendScrollInteractions: vi.fn(),
    notifyScroll: vi.fn(),
    render: vi.fn(),
  });
}

describe('Linux desktop diff scroll buffer', () => {
  it('keeps mounted rows and continues notifying scroll observers inside the measured range', () => {
    const viewer = fixture();
    viewer.handleScroll();
    expect(viewer.render).not.toHaveBeenCalled();
    expect(viewer.scrollDirty).toBe(true);
    expect(viewer.notifyScroll).toHaveBeenCalledOnce();
  });

  it.each([230, 855, 2400])('renders near either edge or after a large jump to %i', (position) => {
    const viewer = fixture();
    viewer.getScrollTop.mockReturnValue(position);
    viewer.handleScroll();
    expect(viewer.render).toHaveBeenCalledOnce();
  });

  it('does not require a buffer beyond the logical start or end of the review', () => {
    const viewer = fixture();
    viewer.renderState = { firstIndex: 0, lastIndex: 0, stickyTop: 0, stickyBottom: 5000 };
    viewer.getScrollTop.mockReturnValueOnce(0).mockReturnValueOnce(4340);
    viewer.getItemTopOffset.mockReturnValue(20);
    viewer.handleScroll();
    viewer.handleScroll();
    expect(viewer.render).not.toHaveBeenCalled();
  });

  it.each([
    { heightDirty: true },
    { layoutDirtyIndex: 0 },
    { pendingLayoutAnchor: {} },
    { pendingScrollTarget: {} },
    { scrollAnimation: {} },
    { renderState: { firstIndex: 3, lastIndex: 4, stickyTop: 200, stickyBottom: 1600 } },
    { renderState: { firstIndex: -1, lastIndex: -1, stickyTop: -1, stickyBottom: -1 } },
    { shouldRebaseScroll: () => true },
  ])('leaves unsafe window reuse to the normal renderer (%j)', (state) => {
    const viewer = fixture();
    Object.assign(viewer, state);
    viewer.handleScroll();
    expect(viewer.render).toHaveBeenCalledOnce();
  });

  it.each([
    ['Linux x86_64', false],
    ['MacIntel', true],
  ] as const)('leaves the regular scroll path on %s with Tauri %s', (platform, isTauri) => {
    const viewer = fixture(platform, isTauri);
    viewer.handleScroll();
    expect(viewer.render).toHaveBeenCalledOnce();
    expect(viewer.getScrollTop).not.toHaveBeenCalled();
  });

  it('detects a changed upstream handler', () => {
    expect(() => bufferLinuxDiffScroll('export {};')).toThrow('scroll handler changed');
  });

  it('grows the rendered window only when it stays inside a single diff', () => {
    const viewer = fixture();
    viewer.renderState.firstIndex = 0;
    viewer.renderState.lastIndex = 0;
    expect(viewer.getLinuxScrollOverscroll(0, 700)).toBe(520);
    expect(viewer.getLinuxScrollOverscroll(1750, 700)).toBe(520);
    expect(viewer.getLinuxScrollOverscroll(2100, 700)).toBe(200);
    viewer.items = [{ top: 1000, height: 3200 }];
    expect(viewer.getLinuxScrollOverscroll(1190, 700)).toBe(200);
    expect(viewer.getLinuxScrollOverscroll(1700, 700)).toBe(520);
    viewer.items = [
      { top: 0, height: 280 },
      { top: 296, height: 280 },
    ];
    expect(viewer.getLinuxScrollOverscroll(0, 700)).toBe(200);
    viewer.items = [{ top: 0, height: 3200 }];
    viewer.renderState.lastIndex = 1;
    expect(viewer.getLinuxScrollOverscroll(0, 700)).toBe(200);
  });

  it('detects a changed upstream scroll window', () => {
    expect(() =>
      bufferLinuxDiffScroll(installed.replace('overscrollSize: this.config.overscrollSize', 'overscrollSize: 12')),
    ).toThrow('scroll window changed');
  });
});
