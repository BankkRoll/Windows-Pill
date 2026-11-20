/**
 * Pure helpers that turn plugin state into what the notch and the Today tab
 * render: cluster signals, the one live item, widgets, the feed, how the
 * small collapsed notch divides its width, and how long plugin lists fold.
 *
 * Plugin states arrive in Settings order (built-ins, then external plugins
 * as the user arranged them), so every list here keeps that order.
 */

import type { FeedItem, FeedSeverity, LiveItem, PluginState, PluginTone, PluginWidget } from "./native";
import type { Signal } from "../store";

/**
 * Plugin signals for the status cluster. Built-ins that feed one of Pill's
 * own kinds (the notification count) merge into it; everything else keeps
 * the plugin's icon. Critical and high feed items add up to one
 * `security` signal.
 *
 * @param plugins - Every plugin's state.
 */
export function pluginSignals(plugins: readonly PluginState[]): Signal[] {
  const enabled = plugins.filter((p) => p.enabled);
  const urgent = enabled.reduce((n, p) => n + p.feed.filter((i) => URGENT.has(i.severity)).length, 0);
  const own = enabled.flatMap((p) =>
    p.signals.map((s): Signal =>
      s.kind === "notification"
        ? { kind: "notification", count: s.count }
        : { kind: "plugin", count: s.count, plugin: { key: `${p.id}:${s.id}`, icon: s.icon, tone: s.tone, label: s.label || p.name } },
    ),
  );
  return urgent > 0 ? [{ kind: "security", count: urgent }, ...own] : own;
}

const URGENT: ReadonlySet<FeedSeverity> = new Set(["critical", "high"]);

/** Most urgent first. */
export const SEVERITY_ORDER: readonly FeedSeverity[] = ["critical", "high", "medium", "low", "info"];

/** A feed item and the plugin that sent it. */
export interface FeedEntry {
  plugin: PluginState;
  item: FeedItem;
}

/**
 * Every enabled plugin's feed items, most severe first; equal severities
 * keep Settings order, then the order the plugin sent them.
 *
 * @param plugins - Every plugin's state.
 */
export function feedEntries(plugins: readonly PluginState[]): FeedEntry[] {
  const all = plugins.filter((p) => p.enabled).flatMap((plugin) => plugin.feed.map((item) => ({ plugin, item })));
  return all
    .map((entry, i) => ({ entry, i }))
    .sort((a, b) => SEVERITY_ORDER.indexOf(a.entry.item.severity) - SEVERITY_ORDER.indexOf(b.entry.item.severity) || a.i - b.i)
    .map(({ entry }) => entry);
}

/** Feed items from one plugin about the same thing, e.g. one advisory in several projects. */
export interface FeedGroup {
  /** Stable key: plugin id plus the item's link, or its title without one. */
  key: string;
  /** The entry that stands for the group: the first in feed order. */
  first: FeedEntry;
  /** Every entry, in feed order, `first` included. */
  entries: FeedEntry[];
}

/**
 * Folds feed entries that share a plugin and a link (or, without a link, a
 * title) into one group, so an advisory that hits ten projects reads as one
 * row rather than ten. Groups are ranked by their first entry's severity,
 * then by how many entries they hold, so the most widespread comes first.
 *
 * @param entries - Output of {@link feedEntries}.
 * @example
 * groupFeed(feedEntries(plugins)).map((g) => g.entries.length) // [3, 1, 1]
 */
export function groupFeed(entries: readonly FeedEntry[]): FeedGroup[] {
  const groups = new Map<string, FeedGroup>();
  for (const entry of entries) {
    const key = `${entry.plugin.id}\u0000${entry.item.url ?? entry.item.title}`;
    const group = groups.get(key);
    if (group) group.entries.push(entry);
    else groups.set(key, { key, first: entry, entries: [entry] });
  }
  const rank = (g: FeedGroup) => SEVERITY_ORDER.indexOf(g.first.item.severity);
  return [...groups.values()].sort((a, b) => rank(a) - rank(b) || b.entries.length - a.entries.length);
}

/** The live item the collapsed notch shows, and who sent it. */
export interface ActiveLive {
  plugin: PluginState;
  live: LiveItem;
}

/**
 * The notch shows one live item: the first enabled plugin, in Settings
 * order, that has one.
 *
 * @param plugins - Every plugin's state.
 */
export function activeLive(plugins: readonly PluginState[]): ActiveLive | null {
  const plugin = plugins.find((p) => p.enabled && p.live);
  return plugin?.live ? { plugin, live: plugin.live } : null;
}

/** A widget and the plugin that owns it. */
export interface ActiveWidget {
  plugin: PluginState;
  widget: PluginWidget;
}

/**
 * Widgets for the Today tab, in Settings order.
 *
 * @param plugins - Every plugin's state.
 */
export function pluginWidgets(plugins: readonly PluginState[]): ActiveWidget[] {
  return plugins.flatMap((p) => (p.enabled && p.widget ? [{ plugin: p, widget: p.widget }] : []));
}

/** A third-party plugin's Today card: its widget, its feed, or both. */
export interface ExtensionCard {
  plugin: PluginState;
  widget: Extract<PluginWidget, { kind: "card" }> | null;
}

/** Third-party cards the Today tab draws in full; the rest fold into one row. */
export const MAX_TODAY_EXTENSIONS = 3;

/** What the Today tab shows above the tasks, in two tiers. */
export interface TodayCards {
  /** Built-in widgets (Now playing), drawn first, beside the notifications card. */
  native: ActiveWidget[];
  /** External plugins with a card or feed items, in Settings order, at most {@link MAX_TODAY_EXTENSIONS}. */
  extensions: ExtensionCard[];
  /** Further external cards, in Settings order, folded into one "+N more" row. */
  more: ExtensionCard[];
}

/**
 * Splits the Today tab's plugin cards into Pill's own, which come first, and
 * third-party ones below them. An external plugin with only feed items still
 * gets a card, so its alerts always live inside it. Past
 * {@link MAX_TODAY_EXTENSIONS} third-party cards, the rest go to `more`.
 *
 * @param plugins - Every plugin's state.
 */
export function todayCards(plugins: readonly PluginState[]): TodayCards {
  const native: ActiveWidget[] = [];
  const extensions: ExtensionCard[] = [];
  for (const plugin of plugins) {
    if (!plugin.enabled) continue;
    if (plugin.source === "builtin") {
      if (plugin.widget) native.push({ plugin, widget: plugin.widget });
      continue;
    }
    const widget = plugin.widget?.kind === "card" ? plugin.widget : null;
    if (widget || plugin.feed.length > 0) extensions.push({ plugin, widget });
  }
  const { shown, hidden } = foldList(extensions, MAX_TODAY_EXTENSIONS);
  return { native, extensions: shown, more: hidden };
}

/** A list cut down to its first entries, see {@link foldList}. */
export interface Folded<T> {
  /** Drawn in full, in list order. */
  shown: T[];
  /** Behind a "+N more" row, in list order. */
  hidden: T[];
}

/**
 * Keeps the first `limit` entries of a long list and folds the rest behind
 * a "+N more" row. Entries `keep` picks stay visible wherever they are (the
 * open page, say), so folding never hides what the user is looking at.
 *
 * @param items - The whole list, in display order.
 * @param limit - Entries shown before folding.
 * @param options.keep - Entries that are never folded away.
 * @param options.minHidden - Fold only when at least this many would hide;
 *   with 2, a list one past the limit shows in full, since a "+1 more" row
 *   would take the same room as the entry it hides.
 * @example
 * foldList(["a", "b", "c", "d", "e"], 3) // { shown: ["a", "b", "c"], hidden: ["d", "e"] }
 */
export function foldList<T>(
  items: readonly T[],
  limit: number,
  { keep, minHidden = 1 }: { keep?: (item: T) => boolean; minHidden?: number } = {},
): Folded<T> {
  const shown: T[] = [];
  const hidden: T[] = [];
  items.forEach((item, i) => (i < limit || keep?.(item) ? shown : hidden).push(item));
  return hidden.length < Math.max(1, minHidden) ? { shown: [...items], hidden: [] } : { shown, hidden };
}

/** How the collapsed notch spends its width. */
export interface CollapsedLayout {
  showDate: boolean;
  /** Signals drawn, most urgent first. */
  signals: Signal[];
  /** Signals that didn't fit, shown as `+N`. */
  overflow: number;
}

/**
 * Splits the collapsed notch's width between time, date, live item and
 * status cluster. The peek shape is 244 px wide: the time always shows, a
 * media live item costs about one signal, a text live item about four, and
 * the date gives way first.
 *
 * @param signals - Merged signals, most urgent first.
 * @param live - The live item, if any.
 */
export function collapsedLayout(signals: readonly Signal[], live: LiveItem | null): CollapsedLayout {
  const [dateUpTo, max] = live === null ? [2, 5] : live.kind === "media" ? [1, 4] : [0, 2];
  const fits = signals.length <= max;
  const shown = fits ? signals.length : max - 1;
  return {
    showDate: signals.length <= dateUpTo,
    signals: signals.slice(0, shown),
    overflow: signals.length - shown,
  };
}

/**
 * Text color class for a plugin tone.
 *
 * @param tone - Plugin tone.
 */
export function toneText(tone: PluginTone): string {
  return TONE_TEXT[tone];
}

const TONE_TEXT: Record<PluginTone, string> = {
  neutral: "text-white/70",
  accent: "text-accent",
  green: "text-status-green",
  amber: "text-status-amber",
  red: "text-status-red",
  purple: "text-status-purple",
};
