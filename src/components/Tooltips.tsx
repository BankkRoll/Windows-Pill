import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

interface Tip {
  text: string;
  x: number;
  y: number;
  above: boolean;
  /** Horizontal limits the bubble must stay within. */
  minX: number;
  maxX: number;
}

const DELAY_MS = 450;
const GAP = 6;
/** Marks an aria attribute this module set, so a changed title can update it. */
const OWNED = "data-tip-aria";

/**
 * Moves an element's `title` to `data-tip`, which keeps the native tooltip
 * away, and returns the tip text.
 *
 * NOTE: `title` is also how the browser names a control that has no text
 * of its own (icon buttons), and adds a description to one that does.
 * Taking it away would leave such controls unnamed for screen readers, so
 * the same text moves to `aria-label` or `aria-description`, unless the
 * element was already labelled some other way.
 */
function adoptTitle(el: HTMLElement): string | null {
  const title = el.getAttribute("title");
  if (title) {
    el.setAttribute("data-tip", title);
    el.removeAttribute("title");
    const owned = el.getAttribute(OWNED);
    if (owned) el.setAttribute(owned, title);
    else if (!el.hasAttribute("aria-label") && !el.hasAttribute("aria-labelledby")) {
      const attr = el.textContent?.trim() ? "aria-description" : "aria-label";
      el.setAttribute(attr, title);
      el.setAttribute(OWNED, attr);
    }
  }
  return el.getAttribute("data-tip");
}

/**
 * Themed tooltips for every element with a `title`, mounted once per window.
 *
 * The native tooltip is a system-styled box that ignores the theme, so on
 * hover or keyboard focus the `title` moves to `data-tip` (which stops the
 * native one) and a styled bubble shows instead. Elements keep using plain
 * `title` props.
 */
export function Tooltips() {
  const [tip, setTip] = useState<Tip | null>(null);
  const bubble = useRef<HTMLDivElement>(null);
  const [left, setLeft] = useState(0);

  useLayoutEffect(() => {
    if (!tip || !bubble.current) return;
    const w = bubble.current.offsetWidth;
    setLeft(Math.min(Math.max(tip.x - w / 2, tip.minX), tip.maxX - w));
  }, [tip]);

  useEffect(() => {
    let timer = 0;
    let current: HTMLElement | null = null;

    const hide = () => {
      window.clearTimeout(timer);
      current = null;
      setTip(null);
    };

    const show = (el: HTMLElement | null) => {
      if (el === current) return;
      hide();
      if (!el) return;
      const text = adoptTitle(el);
      if (!text) return;
      current = el;
      timer = window.setTimeout(() => {
        if (current !== el || !el.isConnected) return;
        const r = el.getBoundingClientRect();
        // Stay inside the notch panel: anything drawn outside its shape is clipped.
        const bounds =
          el.closest("[data-popover-bounds]")?.getBoundingClientRect() ?? new DOMRect(0, 0, window.innerWidth, window.innerHeight);
        const above = r.bottom + 32 > bounds.bottom;
        setTip({
          text,
          x: r.left + r.width / 2,
          y: above ? r.top - GAP : r.bottom + GAP,
          above,
          minX: bounds.left + 8,
          maxX: bounds.right - 8,
        });
      }, DELAY_MS);
    };

    const onOver = (e: PointerEvent) => show((e.target as Element | null)?.closest<HTMLElement>("[title], [data-tip]") ?? null);
    // Keyboard users get the same hint on the control they tab to; a mouse
    // click focuses too, but isn't :focus-visible, so it doesn't double up.
    const onFocus = (e: FocusEvent) => {
      const el = e.target;
      if (el instanceof HTMLElement && el.matches(":focus-visible") && el.matches("[title], [data-tip]")) show(el);
    };
    const onBlur = (e: FocusEvent) => {
      if (e.target === current) hide();
    };

    window.addEventListener("pointerover", onOver, true);
    window.addEventListener("focusin", onFocus, true);
    window.addEventListener("focusout", onBlur, true);
    window.addEventListener("pointerdown", hide, true);
    window.addEventListener("scroll", hide, true);
    window.addEventListener("blur", hide);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("pointerover", onOver, true);
      window.removeEventListener("focusin", onFocus, true);
      window.removeEventListener("focusout", onBlur, true);
      window.removeEventListener("pointerdown", hide, true);
      window.removeEventListener("scroll", hide, true);
      window.removeEventListener("blur", hide);
    };
  }, []);

  return createPortal(
    <AnimatePresence>
      {tip && (
        <motion.div
          key={`${tip.text}:${tip.x}:${tip.y}`}
          role="tooltip"
          initial={{ opacity: 0, y: tip.above ? 3 : -3 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          ref={bubble}
          className="surface-menu pointer-events-none fixed z-[100] w-max max-w-72 rounded-md px-2 py-1 text-[11.5px] leading-snug text-white/90 shadow-[0_6px_20px] shadow-shade/45 ring-1 ring-white/10"
          style={{
            left,
            top: tip.y,
            translate: tip.above ? "0 -100%" : undefined,
          }}
        >
          {tip.text}
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}
