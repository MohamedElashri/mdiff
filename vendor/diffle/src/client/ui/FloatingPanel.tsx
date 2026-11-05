import { useLayoutEffect, useRef, type HTMLAttributes, type RefObject } from 'react';
import { createPortal } from 'react-dom';

/** Keep toolbar panels outside scrolling ancestors and inside the visible viewport. */
export function FloatingPanel({
  anchor,
  panelRef,
  align = 'start',
  children,
  style,
  ...props
}: HTMLAttributes<HTMLDivElement> & {
  anchor: RefObject<HTMLElement | null>;
  panelRef: RefObject<HTMLDivElement | null>;
  align?: 'start' | 'end';
}) {
  const frame = useRef<number | null>(null);
  useLayoutEffect(() => {
    const panel = panelRef.current;
    const trigger = anchor.current;
    if (!panel || !trigger) return;
    const place = () => {
      const rect = trigger.getBoundingClientRect();
      const width = document.documentElement.clientWidth || window.innerWidth;
      const height = window.innerHeight;
      const top = Math.min(rect.bottom + 6, height - 48);
      panel.style.maxWidth = `${Math.max(0, width - 16)}px`;
      panel.style.maxHeight = `${Math.max(0, height - top - 8)}px`;
      panel.style.left = `${Math.max(8, Math.min(align === 'end' ? rect.right - panel.offsetWidth : rect.left, width - panel.offsetWidth - 8))}px`;
      panel.style.top = `${top}px`;
      panel.style.visibility = 'visible';
    };
    const schedule = () => {
      if (frame.current !== null) return;
      frame.current = requestAnimationFrame(() => {
        frame.current = null;
        place();
      });
    };
    place();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(schedule);
    observer?.observe(panel);
    observer?.observe(trigger);
    window.addEventListener('resize', schedule);
    document.addEventListener('scroll', schedule, true);
    return () => {
      observer?.disconnect();
      window.removeEventListener('resize', schedule);
      document.removeEventListener('scroll', schedule, true);
      if (frame.current !== null) cancelAnimationFrame(frame.current);
      frame.current = null;
    };
  }, [anchor, panelRef, align]);
  return createPortal(
    <div
      {...props}
      ref={panelRef}
      style={{ ...style, position: 'fixed', zIndex: 40, overflow: 'auto', visibility: 'hidden' }}
    >
      {children}
    </div>,
    document.body,
  );
}
