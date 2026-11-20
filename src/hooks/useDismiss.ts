import { useCallback, useEffect, useId, useLayoutEffect, useRef, type KeyboardEvent as ReactKeyboardEvent, type RefObject } from "react";
import { usePillStore } from "../store";

/**
 * Shared behavior for anything that floats over the panel while open:
 * menus, pickers and popovers. While `open`, it
 *
 * - holds the notch open, so it can't collapse under the menu,
 * - dismisses on a pointer-down outside every element in `inside`,
 * - dismisses when the window loses focus (clicking into another app),
 * - dismisses on Escape, consuming the key so a task or issue page behind
 *   the menu doesn't close with it.
 *
 * Escape is handled through the returned key handler when focus is inside
 * the menu, so controls in it can claim the key first (with
 * `preventDefault` or `stopPropagation`), and through a window listener when
 * focus is elsewhere.
 *
 * @param open - Whether the menu is showing.
 * @param onDismiss - Closes it.
 * @param inside - The trigger, the menu and anything else that counts as inside.
 * @returns A key handler for the element wrapping the trigger and the menu.
 * @example
 * const onKeyDown = useDismiss(open, () => setOpen(false), [root]);
 * return <div ref={root} onKeyDown={onKeyDown}>…</div>;
 */
export function useDismiss(
  open: boolean,
  onDismiss: () => void,
  inside: readonly RefObject<HTMLElement | null>[],
): (e: ReactKeyboardEvent) => void {
  const holdId = `menu${useId()}`;
  const hold = usePillStore((s) => s.hold);
  const latest = useRef({ onDismiss, inside });
  useLayoutEffect(() => {
    latest.current = { onDismiss, inside };
  });

  useEffect(() => {
    if (!open) return;
    hold(holdId, true);
    const contains = (node: EventTarget | null) => latest.current.inside.some((r) => r.current?.contains(node as Node));
    const onDown = (e: PointerEvent) => {
      if (!contains(e.target)) latest.current.onDismiss();
    };
    const onBlur = () => latest.current.onDismiss();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || contains(e.target)) return;
      e.preventDefault();
      latest.current.onDismiss();
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("blur", onBlur);
    // Capture, so this runs before the page-level Escape listeners.
    window.addEventListener("keydown", onKey, true);
    return () => {
      hold(holdId, false);
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open, hold, holdId]);

  return useCallback(
    (e: ReactKeyboardEvent) => {
      if (!open || e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      latest.current.onDismiss();
    },
    [open],
  );
}
