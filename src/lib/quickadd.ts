/**
 * Quick-add syntax: one line becomes a task.
 *
 * - `#name`   project (fuzzy matched against project names)
 * - `!1`–`!3` priority (1 = high)
 * - `@today`, `@tomorrow`/`@tmr`, `@mon`…`@sun`, `@2026-11-21`, `@+3d`, `@+2w`
 * - `+tag`    tag
 *
 * Everything else is the title. Tokens may appear anywhere in the line.
 */

import { isoDay } from "./format";

/** One recognized token, for highlighting in the input. */
export interface QuickToken {
  kind: "project" | "priority" | "due" | "tag";
  /** Raw text including its sigil. */
  text: string;
}

/** Result of parsing a quick-add line. */
export interface QuickParse {
  title: string;
  /** Text after `#`, if any (last one wins). */
  projectQuery: string | null;
  priority: 1 | 2 | 3 | null;
  /** `YYYY-MM-DD`. */
  due: string | null;
  tags: string[];
  tokens: QuickToken[];
  /** `@…` tokens that didn't parse as a date. */
  invalidDue: string | null;
}

const WEEKDAYS = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/**
 * Resolves a due-date word relative to `today`.
 *
 * Weekdays mean the next occurrence, counting today: `@thu` on a Thursday is today.
 *
 * @param word - Text after `@`, lowercased.
 * @param today - Reference date (local).
 * @returns `YYYY-MM-DD`, or `null` if unrecognized.
 * @example
 * resolveDue("fri", new Date(2026, 10, 19)); // "2026-11-20"
 */
export function resolveDue(word: string, today: Date): string | null {
  const base = new Date(today.getFullYear(), today.getMonth(), today.getDate());
  const plus = (days: number) => {
    const d = new Date(base);
    d.setDate(d.getDate() + days);
    return isoDay(d);
  };
  if (word === "today" || word === "tod") return isoDay(base);
  if (word === "tomorrow" || word === "tmr" || word === "tom") return plus(1);
  const rel = /^\+(\d{1,3})([dw])$/.exec(word);
  if (rel) return plus(Number(rel[1]) * (rel[2] === "w" ? 7 : 1));
  const iso = /^(\d{4})-(\d{2})-(\d{2})$/.exec(word);
  if (iso) {
    const d = new Date(Number(iso[1]), Number(iso[2]) - 1, Number(iso[3]));
    return isoDay(d) === word ? word : null;
  }
  const idx = WEEKDAYS.findIndex((w) => word === w || (word.length >= 3 && w.startsWith(word.slice(0, 3)) && fullDay(w).startsWith(word)));
  if (idx >= 0) return plus((idx - base.getDay() + 7) % 7);
  return null;
}

function fullDay(short: string): string {
  return { sun: "sunday", mon: "monday", tue: "tuesday", wed: "wednesday", thu: "thursday", fri: "friday", sat: "saturday" }[short] ?? short;
}

/**
 * Parses a quick-add line.
 *
 * @param input - Raw text from the quick-add field.
 * @param today - Reference date for relative due dates.
 * @example
 * parseQuickAdd("Fix reels #outlaw !1 @fri +bug", new Date(2026, 10, 19));
 * // { title: "Fix reels", projectQuery: "outlaw", priority: 1, due: "2026-11-20", tags: ["bug"], … }
 */
export function parseQuickAdd(input: string, today: Date = new Date()): QuickParse {
  const out: QuickParse = {
    title: "",
    projectQuery: null,
    priority: null,
    due: null,
    tags: [],
    tokens: [],
    invalidDue: null,
  };
  const words: string[] = [];
  for (const word of input.split(/\s+/).filter(Boolean)) {
    if (/^#[^\s#]+$/.test(word)) {
      out.projectQuery = word.slice(1);
      out.tokens.push({ kind: "project", text: word });
    } else if (/^![123]$/.test(word)) {
      out.priority = Number(word[1]) as 1 | 2 | 3;
      out.tokens.push({ kind: "priority", text: word });
    } else if (/^@\S+$/.test(word)) {
      const due = resolveDue(word.slice(1).toLowerCase(), today);
      if (due) {
        out.due = due;
        out.invalidDue = null;
        out.tokens.push({ kind: "due", text: word });
      } else {
        out.invalidDue = word;
        words.push(word);
      }
    } else if (/^\+[\p{L}\p{N}_-]+$/u.test(word)) {
      const tag = word.slice(1).toLowerCase();
      if (!out.tags.includes(tag)) out.tags.push(tag);
      out.tokens.push({ kind: "tag", text: word });
    } else {
      words.push(word);
    }
  }
  out.title = words.join(" ");
  return out;
}

/** Something with a name that can be fuzzy matched. */
export interface Named {
  name: string;
}

/**
 * Picks the best match for `query` among `items`.
 *
 * Ranking: exact > prefix > word prefix > substring > subsequence. Ties keep
 * input order, so passing projects most-recent-first prefers active ones.
 * Dashes, underscores and spaces are ignored when comparing.
 *
 * @param query - Text typed after `#`.
 * @param items - Candidates, most preferred first.
 * @returns The best candidate, or `null` if nothing matches.
 */
export function fuzzyMatch<T extends Named>(query: string, items: readonly T[]): T | null {
  const norm = (s: string) => s.toLowerCase().replace(/[\s_-]+/g, "");
  const q = norm(query);
  if (!q) return null;
  let best: T | null = null;
  let bestScore = 0;
  for (const item of items) {
    const name = norm(item.name);
    const words = item.name.toLowerCase().split(/[\s_-]+/);
    let score = 0;
    if (name === q) score = 5;
    else if (name.startsWith(q)) score = 4;
    else if (words.some((w) => w.startsWith(q))) score = 3;
    else if (name.includes(q)) score = 2;
    else if (isSubsequence(q, name)) score = 1;
    if (score > bestScore) {
      best = item;
      bestScore = score;
    }
  }
  return best;
}

function isSubsequence(needle: string, hay: string): boolean {
  let i = 0;
  for (const ch of hay) if (ch === needle[i]) i++;
  return i === needle.length;
}
