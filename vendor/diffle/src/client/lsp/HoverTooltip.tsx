import { useEffect, useRef, type RefObject } from 'react';
import { Markdown } from '@/client/Markdown.js';
import { useStore, type HoverState } from '@/client/store.js';
import { TipSurface } from '@/client/ui/TipSurface.js';

export { hoverControl, LINGER_MS } from './hoverControl.js';
import { useReviewDom } from '@/client/runtime.js';
/** Editor-style tooltip below the hovered symbol with the language server's hover text. */
export function HoverTooltip() {
  const { hover: hoverControl } = useReviewDom();
  const hover = useStore((s) => s.hover);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Escape goes through the keymap's `escape()`, which closes the tooltip first; any other key
    // means the reader is working and the tooltip is in the way.
    // Bare modifiers are exempt: Ctrl pressed to prepare a Ctrl+click must not dismiss the tip.
    const MODIFIERS = new Set(['Shift', 'Control', 'Alt', 'Meta', 'CapsLock']);
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' && !MODIFIERS.has(e.key)) hoverControl.cancel();
    };
    // Capture-phase listeners see events from inside the tooltip too (its own scrollbar, a text
    // selection in a signature); those keep it open.
    const inTip = (e: Event) => e.target instanceof Node && !!ref.current?.contains(e.target);
    const cancel = (e: Event) => {
      if (!inTip(e)) hoverControl.cancel();
    };
    document.addEventListener('keydown', onKey, true);
    document.addEventListener('scroll', cancel, true);
    document.addEventListener('pointerdown', cancel, true);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      document.removeEventListener('scroll', cancel, true);
      document.removeEventListener('pointerdown', cancel, true);
    };
  }, [hoverControl]);
  if (!hover) return null;
  return <Tip hover={hover} ref={ref} />;
}

function Tip({ hover, ref }: { hover: HoverState; ref: RefObject<HTMLDivElement | null> }) {
  const { hover: hoverControl } = useReviewDom();
  // Dismissed from under the pointer (a key, a click elsewhere), the box gets no pointer leave.
  useEffect(() => hoverControl.tipLeave, [hoverControl.tipLeave]);
  return (
    <TipSurface
      ref={ref}
      anchor={hover.anchor}
      className="hover-markdown max-h-[45vh] max-w-[min(40rem,_90vw)] overflow-auto"
      onPointerEnter={hoverControl.tipEnter}
      onPointerLeave={hoverControl.tipLeave}
    >
      <Markdown text={hover.contents} path={hover.target.path} highlight />
    </TipSurface>
  );
}
