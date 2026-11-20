/**
 * Locale-aware formatting for the pill's clock and dates, and the checks and
 * labels for links.
 */

const timeFormat = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const dateFormat = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  day: "numeric",
  month: "short",
});

/**
 * Formats the clock time using the user's locale (12h or 24h).
 *
 * @param d - Time to format.
 * @returns For example `9:41 PM` or `21:41`.
 */
export function formatTime(d: Date): string {
  return timeFormat.format(d);
}

/**
 * Formats the short date shown next to the clock.
 *
 * @param d - Date to format.
 * @returns For example `Thu, Nov 19`.
 */
export function formatDate(d: Date): string {
  return dateFormat.format(d);
}

const weekdayFormat = new Intl.DateTimeFormat(undefined, { weekday: "short" });
const monthDayFormat = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });

/** How a due date relates to today, for coloring. */
export type DueTone = "overdue" | "today" | "soon" | "later";

/**
 * Short human label for a due date.
 *
 * @param due - `YYYY-MM-DD`.
 * @param now - Reference time.
 * @returns Label and tone: "Today", "Tomorrow", a weekday within the week, else "Nov 30".
 * @example
 * dueLabel("2026-11-20", new Date(2026, 10, 19)); // { text: "Tomorrow", tone: "soon" }
 */
export function dueLabel(due: string, now: Date = new Date()): { text: string; tone: DueTone } {
  const [y, m, d] = due.split("-").map(Number) as [number, number, number];
  const date = new Date(y, m - 1, d);
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const days = Math.round((date.getTime() - today.getTime()) / 86_400_000);
  if (days < 0) return { text: monthDayFormat.format(date), tone: "overdue" };
  if (days === 0) return { text: "Today", tone: "today" };
  if (days === 1) return { text: "Tomorrow", tone: "soon" };
  if (days < 7) return { text: weekdayFormat.format(date), tone: "soon" };
  return { text: monthDayFormat.format(date), tone: "later" };
}

/**
 * Local calendar date as `YYYY-MM-DD`, the format used for task due dates.
 *
 * @param d - Date to convert.
 * @example
 * isoDay(new Date(2026, 10, 19)); // "2026-11-19"
 */
export function isoDay(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/**
 * Whether a string is an absolute http(s) URL, the only kind Pill opens in
 * the browser. The scheme is matched case-insensitively, as URLs allow.
 *
 * @param url - Candidate link.
 * @example
 * isWebUrl("HTTPS://example.com"); // true
 * isWebUrl("file:///C:/Windows"); // false
 */
export function isWebUrl(url: string): boolean {
  try {
    const { protocol } = new URL(url);
    return protocol === "http:" || protocol === "https:";
  } catch {
    return false;
  }
}

/**
 * A link's host for labels, without a leading `www.`.
 *
 * @param url - Link to describe.
 * @returns The host, or `link` when the URL doesn't parse.
 * @example
 * hostOf("https://www.github.com/x"); // "github.com"
 */
export function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, "");
  } catch {
    return "link";
  }
}
