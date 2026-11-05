// @vitest-environment jsdom
import { act, createElement, createRef } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { FloatingPanel } from '@/client/ui/FloatingPanel.js';

it('escapes clipping, stays on a small screen, coalesces scroll work, and releases listeners', async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement('div');
  host.style.overflow = 'auto';
  const anchor = document.createElement('button');
  host.append(anchor);
  document.body.append(host);
  const measure = vi
    .spyOn(anchor, 'getBoundingClientRect')
    .mockReturnValue({ left: 610, right: 635, bottom: 90 } as DOMRect);
  vi.stubGlobal('innerWidth', 640);
  vi.stubGlobal('innerHeight', 480);
  const panelRef = createRef<HTMLDivElement>();
  let frame: FrameRequestCallback | undefined;
  const request = vi.fn((callback: FrameRequestCallback) => {
    frame = callback;
    return 1;
  });
  vi.stubGlobal('requestAnimationFrame', request);
  const cancel = vi.fn();
  vi.stubGlobal('cancelAnimationFrame', cancel);
  const mount = document.createElement('div');
  host.append(mount);
  const root = createRoot(mount);
  try {
    await act(() =>
      root.render(
        createElement(FloatingPanel, { anchor: { current: anchor }, panelRef, role: 'dialog' }, 'Review menu'),
      ),
    );
    const panel = panelRef.current!;
    expect(panel.parentElement).toBe(document.body);
    expect(panel.style.position).toBe('fixed');
    expect(panel.style.maxWidth).toBe('624px');
    expect(panel.style.maxHeight).toBe('376px');
    Object.defineProperty(panel, 'offsetWidth', { value: 500 });
    for (let i = 0; i < 1000; i++) document.dispatchEvent(new Event('scroll'));
    expect(request).toHaveBeenCalledTimes(1);
    frame!(0);
    expect(measure).toHaveBeenCalledTimes(2);
    expect(panel.style.left).toBe('132px');
    document.dispatchEvent(new Event('scroll'));
    await act(() => root.unmount());
    expect(cancel).toHaveBeenCalledWith(1);
    const count = request.mock.calls.length;
    document.dispatchEvent(new Event('scroll'));
    window.dispatchEvent(new Event('resize'));
    expect(request).toHaveBeenCalledTimes(count);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  } finally {
    host.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});
