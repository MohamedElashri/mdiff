import { defaultStore, type ReviewStore } from '@/client/store.js';
import type { TokenTarget } from './target.js';
export function createHoverControl(useStore: ReviewStore = defaultStore) {
  /** Pointer rest before the server is asked; moving across tokens asks nothing. */
  const SHOW_MS = 300;
  /** Grace after leaving the token, so the pointer can travel into the tooltip. */
  const LINGER_MS = 150;

  // Module state: token enter / leave fire constantly and must not re-render.
  // The tooltip stays while the pointer is over its token or over the tooltip. The two reports arrive
  // in either order: the diff derives a token leave from the <pre>'s pointerleave, which the browser
  // fires after the pointerout React has already turned into the tooltip's enter. So both are tracked
  // and the close is derived from them, not from whichever handler ran last.
  let overToken = false;
  let overTip = false;
  let showTimer: ReturnType<typeof setTimeout> | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;
  const clearShow = () => {
    if (showTimer) clearTimeout(showTimer);
    showTimer = null;
  };
  const clearHide = () => {
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = null;
  };
  /** Close after the grace unless the pointer is over the token or the tooltip. */
  const settle = () => {
    clearHide();
    if (overToken || overTip) return;
    hideTimer = setTimeout(() => {
      hideTimer = null;
      useStore.getState().closeHover();
    }, LINGER_MS);
  };

  /** Hover lifecycle for the diff's tokens; the store owns what is shown. */
  const hoverControl = {
    /** The pointer entered `el`, which shows `target`. */
    enter(target: TokenTarget, el: HTMLElement): void {
      overToken = true;
      clearShow();
      settle();
      showTimer = setTimeout(() => {
        showTimer = null;
        const r = el.getBoundingClientRect();
        void useStore.getState().requestHover(target, { left: r.left, top: r.top, bottom: r.bottom });
      }, SHOW_MS);
    },
    /** The pointer left the token: nothing pending shows, and an open tooltip closes unless the pointer is on it. */
    leave(): void {
      overToken = false;
      clearShow();
      settle();
    },
    /** The pointer is inside the tooltip: keep it. */
    tipEnter(): void {
      overTip = true;
      settle();
    },
    /** The pointer left the tooltip, or the tooltip went away from under it. */
    tipLeave(): void {
      // Only a change settles: StrictMode also runs the unmount cleanup once at mount.
      if (!overTip) return;
      overTip = false;
      settle();
    },
    /** Scroll, click, or a key: nothing pending, nothing shown. */
    cancel(): void {
      overToken = false;
      overTip = false;
      clearShow();
      clearHide();
      useStore.getState().closeHover();
    },
  };

  return hoverControl;
}
export const hoverControl = createHoverControl();
export const LINGER_MS = 150;
