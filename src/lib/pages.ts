/**
 * Plugin tabs: which plugins add a header tab, the pages they sent, and how
 * the tab's project sidebar is ordered.
 *
 * Responsibilities:
 * - Map plugins to header tabs (`plugin:<id>`) and back, and split them
 *   between the header and its overflow menu.
 * - Mirror the core's page cache per plugin and project, fed by
 *   `open_plugin_view` answers and `pill://plugin-page` events.
 * - Order the sidebar: the plugin's order from its `nav` message first,
 *   then the projects it didn't mention, filtered by a search.
 */

import { create } from "zustand";
import type { NavItem, PageChip, PageState, PluginState, PluginTabSpec, Project } from "./native";
import type { TabId } from "../store";

/** Plugin tabs shown in the header at most; more would crowd the built-in tabs. The rest open from a menu. */
export const MAX_PLUGIN_TABS = 3;

/** A plugin that has a tab. */
export type TabPlugin = PluginState & { tab: PluginTabSpec };

/** Prefix of a plugin tab's {@link TabId}. */
const PREFIX = "plugin:";

/**
 * The header tab id of a plugin.
 *
 * @param pluginId - Plugin id.
 * @example pluginTabId("cve-watch") // "plugin:cve-watch"
 */
export function pluginTabId(pluginId: string): TabId {
  return `${PREFIX}${pluginId}`;
}

/**
 * The plugin a tab id names, or `null` for Pill's own tabs.
 *
 * @param tab - Any tab id.
 */
export function tabPlugin(tab: string): string | null {
  return tab.startsWith(PREFIX) && tab.length > PREFIX.length ? tab.slice(PREFIX.length) : null;
}

/**
 * Enabled plugins with a tab, in Settings order (the user's order, which the
 * core already applied).
 *
 * @param plugins - Every plugin's state.
 */
export function pluginTabs(plugins: readonly PluginState[]): TabPlugin[] {
  return plugins.filter((p): p is TabPlugin => p.enabled && p.tab !== null);
}

/** Plugin tabs split between the header and its overflow menu. */
export interface HeaderTabs {
  /** Drawn as icons after the built-in tabs. */
  shown: TabPlugin[];
  /** Listed in the `+N` menu, in Settings order. */
  overflow: TabPlugin[];
}

/**
 * Splits plugin tabs between the header and its overflow menu. Pinned tabs
 * fill the header in Settings order, up to `max`. The active tab is always
 * in the header: when it would be in the menu it takes a free slot, or else
 * the last one, whose tab moves to the menu.
 *
 * @param tabs - Output of {@link pluginTabs}.
 * @param activePlugin - Plugin whose tab is open, if any.
 * @param max - Header slots for plugin tabs.
 * @example
 * headerTabs(tabs, "e").shown.map((p) => p.id) // ["a", "b", "e"]
 */
export function headerTabs(tabs: readonly TabPlugin[], activePlugin: string | null, max = MAX_PLUGIN_TABS): HeaderTabs {
  const shown = tabs.filter((p) => p.pinned).slice(0, max);
  const active = tabs.find((p) => p.id === activePlugin);
  if (active && max > 0 && !shown.includes(active)) {
    if (shown.length >= max) shown.length = max - 1;
    shown.push(active);
  }
  return { shown, overflow: tabs.filter((p) => !shown.includes(p)) };
}

/**
 * Whether a plugin's pages are per project. Plugins without the `projects`
 * capability never learn project ids, so they only have an overview page.
 *
 * @param plugin - The plugin.
 */
export function hasProjectPages(plugin: PluginState): boolean {
  return plugin.capabilities.includes("projects");
}

/**
 * The cache key of one page.
 *
 * @param pluginId - Plugin id.
 * @param project - Project id, or `null` for the overview.
 */
export function pageKey(pluginId: string, project: string | null): string {
  return `${pluginId}/${project ?? "*"}`;
}

/** A row to bring into view once its page is on screen. */
export interface PageFocus {
  /** The page's {@link pageKey}. */
  key: string;
  /** The row's id, as the plugin gave it. */
  row: string;
}

interface PageStore {
  /** Last page per {@link pageKey}. */
  pages: Readonly<Record<string, PageState>>;
  put: (pluginId: string, project: string | null, state: PageState) => void;
  /** Set when the page was opened from an alert; cleared once shown. */
  focus: PageFocus | null;
  setFocus: (focus: PageFocus | null) => void;
}

/** Pages plugins sent, as the core last reported them. */
export const usePageStore = create<PageStore>((set) => ({
  pages: {},
  focus: null,
  setFocus: (focus) => set({ focus }),
  put: (pluginId, project, state) =>
    set((s) => {
      const key = pageKey(pluginId, project);
      // An older answer (from `open_plugin_view`) must not replace a newer event.
      if ((s.pages[key]?.receivedAt ?? -1) > state.receivedAt) return s;
      return { pages: { ...s.pages, [key]: state } };
    }),
}));

/** One entry in a plugin tab's sidebar. */
export interface NavEntry {
  /** `null` for "All projects". */
  project: Project | null;
  badge: PageChip | null;
}

/**
 * The sidebar's project entries: those the plugin listed in its `nav`
 * message, in its order (most exposed first, for CVE watch), then the rest
 * in Pill's order. Ids Pill doesn't know are ignored. A search keeps the
 * projects whose name contains it.
 *
 * @param projects - Pill's projects.
 * @param nav - The plugin's sidebar entries.
 * @param query - Search text.
 */
export function navEntries(projects: readonly Project[], nav: readonly NavItem[], query = ""): NavEntry[] {
  const byId = new Map(projects.map((p) => [p.id, p]));
  const listed: NavEntry[] = [];
  const seen = new Set<string>();
  for (const item of nav) {
    const project = item.project ? byId.get(item.project) : undefined;
    if (!project || seen.has(project.id)) continue;
    seen.add(project.id);
    listed.push({ project, badge: item.badge });
  }
  const rest = projects.filter((p) => !seen.has(p.id)).map((project): NavEntry => ({ project, badge: null }));
  const q = query.trim().toLowerCase();
  const all = [...listed, ...rest];
  return q ? all.filter((e) => e.project?.name.toLowerCase().includes(q)) : all;
}

/**
 * The badge the plugin gave one project, if any.
 *
 * @param nav - The plugin's sidebar entries.
 * @param projectId - Project id.
 */
export function navBadge(nav: readonly NavItem[], projectId: string): PageChip | null {
  return nav.find((i) => i.project === projectId)?.badge ?? null;
}

/**
 * The badge the plugin gave its "All projects" entry.
 *
 * @param nav - The plugin's sidebar entries.
 */
export function overviewBadge(nav: readonly NavItem[]): PageChip | null {
  return nav.find((i) => i.project === null)?.badge ?? null;
}
