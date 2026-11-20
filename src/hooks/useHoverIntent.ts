import { useCallback, useEffect, useState } from "react";

/** Delays for {@link useHoverIntent}, in milliseconds. */
export interface HoverIntentDelays {
  /** Cursor must rest this long before opening. */
  openDelay: number;
  /** Cursor must stay away this long before closing. */
  closeDelay: number;
}

/**
 * Turns a raw hovered signal into an intentional open/closed state, so a fast
 * swipe past the pill does nothing and brief exits don't slam it shut.
 *
 * @param hovered - Whether the cursor is over the shape right now.
 * @param delays - Open and close delays.
 * @param hold - While true the panel stays open regardless of hover (e.g. an input has focus).
 * @param openOnHover - When false, only `setOpen` opens (click to open); leaving still closes.
 * @returns `[open, setOpen]`; `setOpen` bypasses the delays (clicks, hotkeys).
 */
export function useHoverIntent(
  hovered: boolean,
  { openDelay, closeDelay }: HoverIntentDelays,
  hold = false,
  openOnHover = true,
): [boolean, (open: boolean) => void] {
  const [open, setOpenState] = useState(false);

  useEffect(() => {
    if (hold) return;
    if (hovered === open) return;
    if (hovered && !openOnHover) return;
    const id = window.setTimeout(() => setOpenState(hovered), hovered ? openDelay : closeDelay);
    return () => window.clearTimeout(id);
  }, [hovered, open, hold, openDelay, closeDelay, openOnHover]);

  const setOpen = useCallback((next: boolean) => setOpenState(next), []);
  return [open, setOpen];
}
