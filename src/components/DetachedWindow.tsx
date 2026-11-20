import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";
import { api, EVENTS, isTauri, subscribe, type DetachedNav } from "../lib/native";
import { usePillStore, type TabId } from "../store";
import { locate } from "../lib/tasks";
import { tabPlugin } from "../lib/pages";
import { ExpandedPanel } from "./ExpandedPanel";
import { GlassBackground } from "./Glass";

const TABS: readonly TabId[] = ["today", "board", "chat", "agents", "settings"];

/**
 * Applies a {@link DetachedNav} to the store. Unknown tabs and projects that
 * no longer exist are ignored, so a stale request never blanks the window. A
 * plugin's tab is taken as asked; the panel falls back to Today if that
 * plugin has no tab once its state arrives.
 *
 * @param nav - View to show.
 */
export function applyDetachedNav(nav: DetachedNav) {
  const s = usePillStore.getState();
  if (nav.tab && ((TABS as readonly string[]).includes(nav.tab) || tabPlugin(nav.tab) !== null)) s.setActiveTab(nav.tab as TabId);
  if (nav.projectId === null || s.snapshot.projects.some((p) => p.id === nav.projectId)) s.selectProject(nav.projectId);
  // A notification names a run (shown in Agents) or else a task (its page).
  // setActiveTab closed any task page above, so this comes after it. The run
  // list may still be loading in a new window; the selection holds until then.
  if (nav.runId) s.selectRun(nav.runId);
  else if (nav.taskId && locate(s.snapshot, nav.taskId)) s.openDetail(nav.taskId);
}

/**
 * Keeps this window's monitor wallpaper in the store while the glass theme
 * is on: loaded once, refreshed every 10 s for wallpaper changes, and again
 * when the window lands on a different monitor or the glass source changes.
 * Ordinary moves need nothing; the glass layer just shifts.
 */
function useMonitorWallpaper(enabled: boolean) {
  const setBackdrop = usePillStore((s) => s.setBackdrop);
  const source = usePillStore((s) => s.config.glassSource);
  const image = usePillStore((s) => s.config.glassImage);
  useEffect(() => {
    if (!enabled || !isTauri) {
      setBackdrop(null);
      return;
    }
    let alive = true;
    const load = () =>
      void api
        .sampleBackdrop()
        .then((b) => {
          const current = usePillStore.getState().backdrop;
          // Unchanged wallpaper: keep the same object so nothing re-renders.
          if (alive && JSON.stringify(b) !== JSON.stringify(current)) setBackdrop(b);
        })
        .catch(() => {});
    load();
    const interval = window.setInterval(load, 10_000);
    let timer = 0;
    const moved = getCurrentWindow().onMoved(({ payload }) => {
      const current = usePillStore.getState().backdrop;
      if (!current) return;
      const [mx, my, mw, mh] = current.monitor;
      const inside = payload.x >= mx - 50 && payload.y >= my - 50 && payload.x < mx + mw && payload.y < my + mh;
      if (inside) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(load, 150);
    });
    return () => {
      alive = false;
      window.clearInterval(interval);
      window.clearTimeout(timer);
      void moved.then((f) => f());
    };
  }, [enabled, source, image, setBackdrop]);
}

/** Root of the detached window: the full panel over the theme's surface. */
export function DetachedWindow() {
  const theme = usePillStore((s) => s.config.theme);
  useMonitorWallpaper(theme === "glass");
  useEffect(() => subscribe<DetachedNav>(EVENTS.detachedNav, applyDetachedNav), []);

  return (
    <div className="relative h-full w-full">
      {theme === "glass" && <GlassBackground />}
      <div className="relative h-full w-full">
        <ExpandedPanel windowed />
      </div>
    </div>
  );
}
