/**
 * Pure helpers for the Windows notifications card on the Today tab: when it
 * shows, what each row says, relative times, and what stays listed while a
 * dismiss is still on its way to Action Center.
 */

import type { NotificationFeed, NotificationItem, PluginState } from "./native";

/** Id of the built-in Windows notifications plugin. Mirrors Rust `plugins::notifications::ID`. */
export const NOTIFICATIONS_PLUGIN = "notifications";

/** Rows the card shows before "Show all". */
export const COLLAPSED_ROWS = 5;

/**
 * What the card shows: nothing, why it can't list notifications, or the list
 * (which may be empty).
 */
export type CardMode = "hidden" | "blocked" | "list";

/**
 * Decides what the notifications card shows.
 *
 * @param plugin - The notifications plugin's state, if known yet.
 * @param feed - The latest feed from the core.
 * @returns `blocked` when Windows stands in the way (no permission, unsupported, failed) and there's nothing to list.
 */
export function cardMode(plugin: PluginState | undefined, feed: NotificationFeed | null): CardMode {
  if (!plugin?.enabled) return "hidden";
  if (feed) return "list";
  const { state } = plugin.status;
  return state === "needsPermission" || state === "unsupported" || state === "error" ? "blocked" : "hidden";
}

const shortDate = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });

/**
 * A compact age for a notification, like Action Center's.
 *
 * @param createdMs - Creation time, Unix ms; `0` when Windows didn't say.
 * @param now - Reference time, Unix ms.
 * @returns `now`, `5m`, `3h`, `2d`, a short date after a week, or `""` when unknown.
 * @example
 * relativeTime(Date.now() - 120_000, Date.now()); // "2m"
 */
export function relativeTime(createdMs: number, now: number): string {
  if (!Number.isFinite(createdMs) || createdMs <= 0) return "";
  const minutes = Math.floor((now - createdMs) / 60_000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  return shortDate.format(createdMs);
}

/** The text of one row: a one-line headline and up to two lines under it. */
export interface NoteLines {
  headline: string;
  body: string;
}

/**
 * Splits a notification into a headline and body. Toasts without a title
 * promote their body; toasts with no text at all fall back to the app name,
 * so a row is never blank.
 *
 * @param note - The notification.
 */
export function noteLines(note: NotificationItem): NoteLines {
  if (note.title) return { headline: note.title, body: note.body };
  if (note.body) return { headline: note.body, body: "" };
  return { headline: note.app, body: "" };
}

/**
 * Notifications still listed: the feed minus the ones being dismissed.
 *
 * @param feed - The latest feed.
 * @param hidden - Ids dismissed optimistically.
 */
export function listedNotes(feed: NotificationFeed | null, hidden: ReadonlySet<number>): NotificationItem[] {
  return feed ? feed.notes.filter((n) => !hidden.has(n.id)) : [];
}

/**
 * How many notifications Action Center holds once pending dismisses land.
 *
 * @param feed - The latest feed.
 * @param hidden - Ids dismissed optimistically.
 */
export function remainingTotal(feed: NotificationFeed | null, hidden: ReadonlySet<number>): number {
  if (!feed) return 0;
  const gone = feed.notes.reduce((n, note) => n + (hidden.has(note.id) ? 1 : 0), 0);
  return Math.max(0, feed.total - gone);
}

/**
 * Forgets optimistic dismisses the feed has caught up with.
 *
 * @param hidden - Ids dismissed optimistically.
 * @param feed - The latest feed.
 * @returns `hidden` itself when nothing changed, so React can skip a render.
 */
export function pruneHidden(hidden: ReadonlySet<number>, feed: NotificationFeed | null): ReadonlySet<number> {
  if (hidden.size === 0) return hidden;
  const live = new Set(feed?.notes.map((n) => n.id) ?? []);
  const kept = [...hidden].filter((id) => live.has(id));
  return kept.length === hidden.size ? hidden : new Set(kept);
}
