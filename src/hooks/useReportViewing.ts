import { useEffect, useState } from "react";
import { api, type Viewing } from "../lib/native";
import { sameViewing, viewingOf } from "../lib/viewing";
import { usePillStore } from "../store";

/** Re-sent this often while showing; the core forgets a view after 75 s. */
const HEARTBEAT_MS = 30_000;

/**
 * Tells the core which run and task this window shows, so it can skip
 * toasts about something the user is already looking at. Reports `null`
 * while the window isn't in front of the user and when the panel unmounts.
 *
 * @param needsFocus - Only count the window while it has focus and is
 * visible (the pop-out). The notch passes `false`: it is only mounted
 * while expanded, and expanding it means the user is looking.
 */
export function useReportViewing(needsFocus: boolean) {
  const focused = useWindowFocus();
  const present = !needsFocus || focused;

  useEffect(() => {
    let last: Viewing | null = null;
    const send = (v: Viewing | null) => void api.setViewing(v).catch(() => {});
    const report = (force = false) => {
      const next = present ? viewingOf(usePillStore.getState()) : null;
      if (!force && sameViewing(next, last)) return;
      last = next;
      send(next);
    };
    report(true);
    const unsubscribe = usePillStore.subscribe((s, prev) => {
      if (
        s.activeTab !== prev.activeTab ||
        s.detailTaskId !== prev.detailTaskId ||
        s.selectedRunId !== prev.selectedRunId ||
        s.selectedProjectId !== prev.selectedProjectId ||
        s.runs !== prev.runs
      ) {
        report();
      }
    });
    const heartbeat = window.setInterval(() => {
      if (last) report(true);
    }, HEARTBEAT_MS);
    return () => {
      unsubscribe();
      window.clearInterval(heartbeat);
      send(null);
    };
  }, [present]);
}

/** Whether this window has focus and isn't minimized or hidden. */
function useWindowFocus(): boolean {
  const [focused, setFocused] = useState(() => document.hasFocus() && document.visibilityState === "visible");
  useEffect(() => {
    const update = () => setFocused(document.hasFocus() && document.visibilityState === "visible");
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    document.addEventListener("visibilitychange", update);
    return () => {
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
      document.removeEventListener("visibilitychange", update);
    };
  }, []);
  return focused;
}
