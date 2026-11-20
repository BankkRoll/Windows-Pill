import {
  Board16Regular,
  Bot16Regular,
  CalendarToday16Regular,
  Chat16Regular,
  Settings16Regular,
  WindowNew16Regular,
  type FluentIcon,
} from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useMemo, useRef, type ComponentType } from "react";
import { SHAPES } from "../lib/shapes";
import { usePillStore, type TabId } from "../store";
import { attempt, popOut } from "../lib/actions";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { KeyboardEvent as ReactKeyboardEvent, MouseEvent as ReactMouseEvent } from "react";
import { api } from "../lib/native";
import { useReportViewing } from "../hooks/useReportViewing";
import { useUpdateStore } from "../lib/updates";
import { headerTabs, pluginTabId, pluginTabs, tabPlugin } from "../lib/pages";
import { PLUGIN_ICONS } from "./PluginIcon";
import { PluginTab } from "./PluginTab";
import { PluginTabMenu } from "./PluginTabMenu";
import { ProjectPicker } from "./ProjectPicker";
import { CommandBar } from "./CommandBar";
import { ErrorBoundary } from "./ErrorBoundary";
import { IssueDetail } from "./IssueDetail";
import { TaskDetail } from "./TaskDetail";
import { AgentsTab } from "./tabs/AgentsTab";
import { BoardTab } from "./tabs/BoardTab";
import { ChatTab } from "./tabs/ChatTab";
import { SettingsTab } from "./tabs/SettingsTab";
import { TodayTab } from "./tabs/TodayTab";
import { Toast } from "./Toast";
import { IconButton } from "./ui";
import { WindowControls } from "./WindowControls";

interface TabDef {
  id: TabId;
  label: string;
  icon: FluentIcon;
  view: ComponentType;
}

/** Tabs on the left of the header. Settings lives behind the gear on the right. */
const TABS: readonly TabDef[] = [
  { id: "today", label: "Today", icon: CalendarToday16Regular, view: TodayTab },
  { id: "board", label: "Board", icon: Board16Regular, view: BoardTab },
  { id: "chat", label: "Chat", icon: Chat16Regular, view: ChatTab },
  { id: "agents", label: "Agents", icon: Bot16Regular, view: AgentsTab },
];

const VIEWS: Record<string, ComponentType> = {
  ...Object.fromEntries(TABS.map((t) => [t.id, t.view])),
  settings: SettingsTab,
};

/**
 * The expanded panel: tab bar, project picker and settings gear on top, the
 * active tab in the middle, and an always-present quick-add bar at the bottom.
 *
 * In the notch it has the notch's fixed size and a pop-out button; in the
 * detached window it fills the window, the header drags it, and window
 * controls replace the pop-out button.
 *
 * @param props.windowed - Renders for the detached window.
 */
export function ExpandedPanel({ windowed = false }: { windowed?: boolean }) {
  const { width, height } = SHAPES.expanded;
  const selectedProjectId = usePillStore((s) => s.selectedProjectId);
  const setFilled = usePillStore((s) => s.setFilled);
  useReportViewing(windowed);

  // Hand-rolled instead of data-tauri-drag-region so a double-click fills the
  // monitor exactly and dragging a filled window first shrinks it under the
  // cursor. The native drag starts only once the mouse actually moves: an
  // immediate drag loop would swallow the second click of a double-click.
  // Only blank header space drags; buttons keep their clicks.
  const lastClick = useRef(0);
  const onHeaderMouseDown = (e: ReactMouseEvent<HTMLElement>) => {
    if (!windowed || e.button !== 0 || e.target !== e.currentTarget) return;
    e.preventDefault();
    const now = performance.now();
    if (now - lastClick.current < 450) {
      lastClick.current = 0;
      void attempt(() => api.toggleFill().then(setFilled));
      return;
    }
    const start = { x: e.screenX, y: e.screenY };
    const ratio = e.clientX / window.innerWidth;
    const cleanup = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    const onMove = (m: MouseEvent) => {
      if (Math.hypot(m.screenX - start.x, m.screenY - start.y) < 4) return;
      cleanup();
      const drag = () => getCurrentWindow().startDragging();
      if (usePillStore.getState().filled) {
        setFilled(false);
        void attempt(() => api.restoreForDrag(ratio).then(drag));
      } else {
        void attempt(drag);
      }
    };
    const onUp = () => {
      cleanup();
      lastClick.current = now;
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };
  const onTabKey = (e: ReactKeyboardEvent<HTMLElement>) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const tabs = [...e.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]')];
    const at = tabs.indexOf(document.activeElement as HTMLElement);
    if (at < 0) return;
    e.preventDefault();
    tabs[(at + (e.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length]?.focus();
  };
  const active = usePillStore((s) => s.activeTab);
  const detail = usePillStore((s) => (s.detailTaskId !== null ? "task" : s.detailIssue !== null ? "issue" : null));
  const detailOpen = detail !== null;

  // Opening the panel (or focusing the pop-out) is when GitHub data is
  // wanted; the core skips projects synced in the last minute.
  useEffect(() => {
    const ping = () => void api.githubRefresh(null).catch(() => {});
    ping();
    window.addEventListener("focus", ping);
    return () => window.removeEventListener("focus", ping);
  }, []);
  const setTab = usePillStore((s) => s.setActiveTab);
  const updateReady = useUpdateStore((s) => s.status.kind === "ready");
  const plugins = usePillStore((s) => s.plugins);
  const extraTabs = useMemo(() => pluginTabs(plugins), [plugins]);
  const activePlugin = tabPlugin(active);
  const { shown: headerPlugins, overflow } = useMemo(() => headerTabs(extraTabs, activePlugin), [extraTabs, activePlugin]);
  const pluginShown = activePlugin !== null && extraTabs.some((p) => p.id === activePlugin);
  const View = VIEWS[active] ?? TodayTab;

  // A plugin turned off (or gone after a reload) takes its tab with it.
  useEffect(() => {
    if (activePlugin !== null && !pluginShown && plugins.length > 0) setTab("today");
  }, [activePlugin, pluginShown, plugins.length, setTab]);

  return (
    <div data-popover-bounds className="relative flex flex-col" style={windowed ? { width: "100%", height: "100%" } : { width, height }}>
      <header
        onMouseDown={onHeaderMouseDown}
        className={`grid shrink-0 grid-cols-[1fr_auto_1fr] items-center px-3 ${windowed ? "h-12 pr-0" : "h-11 pt-1"}`}
      >
        <nav onMouseDown={onHeaderMouseDown} className="flex items-center gap-0.5 self-stretch">
          <div
            role="tablist"
            aria-label="Tabs"
            onMouseDown={onHeaderMouseDown}
            onKeyDown={onTabKey}
            className="flex items-center gap-0.5 self-stretch"
          >
            {TABS.map((t) => (
              <IconButton
                key={t.id}
                icon={t.icon}
                label={t.label}
                active={active === t.id}
                role="tab"
                aria-selected={active === t.id}
                aria-pressed={undefined}
                onClick={() => setTab(t.id)}
              />
            ))}
            {extraTabs.length > 0 && <span aria-hidden className="mx-1 h-4 w-px bg-white/10" />}
            {headerPlugins.map((p) => (
              <IconButton
                key={p.id}
                icon={PLUGIN_ICONS[p.tab.icon]}
                label={p.tab.title}
                active={active === pluginTabId(p.id)}
                role="tab"
                aria-selected={active === pluginTabId(p.id)}
                aria-pressed={undefined}
                onClick={() => setTab(pluginTabId(p.id))}
              />
            ))}
          </div>
          <PluginTabMenu plugins={overflow} />
        </nav>
        <ProjectPicker />
        <div onMouseDown={onHeaderMouseDown} className="flex items-center justify-end gap-0.5 self-stretch">
          {!windowed && (
            <IconButton icon={WindowNew16Regular} label="Pop out into a window" onClick={() => popOut(active, selectedProjectId)} />
          )}
          <IconButton
            icon={Settings16Regular}
            label={updateReady ? "Settings (update ready)" : "Settings"}
            badge={updateReady}
            active={active === "settings"}
            onClick={() => setTab(active === "settings" ? "today" : "settings")}
          />
          {windowed && <WindowControls />}
        </div>
      </header>

      <main className="scroll-fade relative min-h-0 flex-1 overflow-y-auto px-2">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={detail ?? active}
            className="h-full"
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.12 }}
          >
            <ErrorBoundary>
              {detail === "task" ? (
                <TaskDetail />
              ) : detail === "issue" ? (
                <IssueDetail />
              ) : activePlugin !== null ? (
                pluginShown && <PluginTab pluginId={activePlugin} />
              ) : (
                <View />
              )}
            </ErrorBoundary>
          </motion.div>
        </AnimatePresence>
      </main>

      {active !== "chat" && active !== "settings" && activePlugin === null && !detailOpen ? (
        <footer className="shrink-0 px-3 pt-2 pb-3">
          <CommandBar variant="panel" />
        </footer>
      ) : (
        <div className="h-2 shrink-0" />
      )}
      <Toast />
    </div>
  );
}
