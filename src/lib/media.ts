/**
 * Pure helpers for the Now playing card: advancing the playback position
 * between updates, formatting times, and turning pointer and keyboard input
 * into seek targets.
 */

import type { MediaSession, MediaTimeline, RepeatMode } from "./native";

/**
 * Where playback is now. Players report their position only now and then,
 * so while playing it advances from the last report at the playback rate.
 *
 * @param timeline - Last reported timeline.
 * @param playing - Whether the session is playing.
 * @param now - Current Unix time in ms.
 * @returns Position in ms, within the track.
 * @example positionAt({ positionMs: 1000, durationMs: 5000, updatedAt: 0, rate: 1 }, true, 1500) // 2500
 */
export function positionAt(timeline: MediaTimeline, playing: boolean, now: number): number {
  const elapsed = playing ? Math.max(0, now - timeline.updatedAt) * timeline.rate : 0;
  return Math.min(timeline.durationMs, Math.max(0, timeline.positionMs + elapsed));
}

/**
 * Formats a track time as `m:ss`, or `h:mm:ss` from an hour.
 *
 * @param ms - Time in ms; negative or non-finite values count as zero.
 * @example formatClock(83_000) // "1:23"
 */
export function formatClock(ms: number): string {
  const total = Number.isFinite(ms) && ms > 0 ? Math.floor(ms / 1000) : 0;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/**
 * Fraction of a horizontal track under the pointer.
 *
 * @param clientX - Pointer x in client pixels.
 * @param left - Track's left edge in client pixels.
 * @param width - Track width in pixels.
 * @returns 0 to 1.
 */
export function trackFraction(clientX: number, left: number, width: number): number {
  if (!(width > 0)) return 0;
  return Math.min(1, Math.max(0, (clientX - left) / width));
}

/** Arrow keys move this far. */
const STEP_MS = 5_000;

/**
 * Seek target for a key on the seek bar: arrows move 5 s, Page Up/Down a
 * tenth of the track (at least 10 s), Home and End jump to the ends.
 *
 * @param key - `KeyboardEvent.key`.
 * @param positionMs - Position the key moves from.
 * @param durationMs - Track length.
 * @returns The new position, or `null` for keys the bar doesn't use.
 */
export function keySeek(key: string, positionMs: number, durationMs: number): number | null {
  const page = Math.max(10_000, durationMs / 10);
  const delta: Record<string, number> = {
    ArrowLeft: -STEP_MS,
    ArrowDown: -STEP_MS,
    ArrowRight: STEP_MS,
    ArrowUp: STEP_MS,
    PageDown: -page,
    PageUp: page,
  };
  let next: number;
  if (key === "Home") next = 0;
  else if (key === "End") next = durationMs;
  else if (key in delta) next = positionMs + delta[key]!;
  else return null;
  return Math.min(durationMs, Math.max(0, next));
}

/**
 * The repeat button's label for a mode, saying what is on now.
 *
 * @param mode - Current mode.
 */
export function repeatLabel(mode: RepeatMode): string {
  return mode === "track" ? "Repeat one" : mode === "list" ? "Repeat all" : "Repeat off";
}

/**
 * Artwork width for a height, keeping the image's shape within the card's
 * limits (square to 16:9).
 *
 * @param aspect - Width over height.
 * @param height - Box height in CSS px.
 */
export function artworkWidth(aspect: number, height: number): number {
  const a = Number.isFinite(aspect) ? Math.min(16 / 9, Math.max(1, aspect)) : 1;
  return Math.round(height * a);
}

/**
 * Whether two timelines report the same thing, so a seek waiting for the
 * player can tell a fresh report from a repeat of the old one.
 *
 * @param a - One timeline, or `null` for none.
 * @param b - The other.
 */
export function sameTimeline(a: MediaTimeline | null, b: MediaTimeline | null): boolean {
  if (a === null || b === null) return a === b;
  return a.positionMs === b.positionMs && a.updatedAt === b.updatedAt && a.durationMs === b.durationMs && a.rate === b.rate;
}

/** One app's sessions in the player switcher. */
export interface SessionGroup {
  app: string;
  /** The app's icon, from the first session that has one. */
  icon: string | null;
  entries: {
    session: MediaSession;
    /** 2, 3... for a session whose title and artist repeat an earlier one in the app; otherwise `null`. */
    repeat: number | null;
  }[];
}

/**
 * Groups sessions by app for the player switcher, so Chrome's tabs sit under
 * one Chrome heading and Spotify under its own. Every session stays: two tabs
 * on the same video are still two players, so repeats are numbered instead.
 *
 * @param sessions - Every session, in the core's order.
 * @returns Groups in order of each app's first session.
 * @example
 * groupSessions([chromeA, spotify, chromeB]).map((g) => g.app) // ["Chrome", "Spotify"]
 */
export function groupSessions(sessions: readonly MediaSession[]): SessionGroup[] {
  const groups = new Map<string, SessionGroup>();
  const seen = new Map<string, number>();
  for (const session of sessions) {
    let group = groups.get(session.app);
    if (!group) groups.set(session.app, (group = { app: session.app, icon: null, entries: [] }));
    group.icon ??= session.icon;
    const key = [session.app, session.title.trim(), session.artist.trim()].join("|");
    const count = (seen.get(key) ?? 0) + 1;
    seen.set(key, count);
    group.entries.push({ session, repeat: count > 1 ? count : null });
  }
  return [...groups.values()];
}
