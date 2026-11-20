import { useEffect, useId, useMemo } from "react";
import { focusWindow, releaseFocus } from "../lib/native";
import { usePillStore } from "../store";

/**
 * Props that let a text field receive keystrokes inside the non-activating
 * overlay window.
 *
 * The pill never takes focus on its own, so typing would otherwise go to the
 * app underneath. On pointer-down the window asks the core for keyboard
 * focus; while the field is focused, the panel is held open even if the
 * cursor drifts away.
 *
 * @returns Handlers to spread onto an `<input>` or `<textarea>`, plus `release`
 * to hand focus back to the previous app (e.g. on Escape).
 */
export function useKeyboardFocus() {
  const id = useId();
  const hold = usePillStore((s) => s.hold);

  useEffect(() => () => hold(`input${id}`, false), [hold, id]);

  return useMemo(
    () => ({
      props: {
        onPointerDown: () => void focusWindow(),
        onFocus: () => hold(`input${id}`, true),
        onBlur: () => hold(`input${id}`, false),
      },
      release: (el?: HTMLElement | null) => {
        el?.blur();
        hold(`input${id}`, false);
        void releaseFocus();
      },
    }),
    [hold, id],
  );
}
