/**
 * Calendar math for the date picker: parsing `YYYY-MM-DD` days, stepping by
 * days and months, building month grids and finding the locale's first
 * weekday. Everything works on local midnight dates so DST never shifts a day.
 */

/**
 * Parses a `YYYY-MM-DD` day into a local midnight `Date`.
 *
 * @param day - Day string, as stored on tasks.
 * @returns The date, or `null` when the string isn't a valid day.
 * @example
 * parseDay("2026-11-21"); // Sat Nov 21 2026 00:00 local
 */
export function parseDay(day: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!m) return null;
  const [y, mo, d] = [Number(m[1]), Number(m[2]) - 1, Number(m[3])];
  const date = new Date(y, mo, d);
  return date.getFullYear() === y && date.getMonth() === mo && date.getDate() === d ? date : null;
}

/**
 * Local midnight of the given moment.
 *
 * @param d - Any time on the wanted day.
 */
export function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

/**
 * Moves a date by whole calendar days.
 *
 * @param d - Starting date.
 * @param n - Days to add; negative goes back.
 * @returns A new local midnight date.
 */
export function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
}

/**
 * Moves a date by whole months, clamping the day to the target month's
 * length so Jan 31 + 1 month is Feb 28/29 rather than early March.
 *
 * @param d - Starting date.
 * @param n - Months to add; negative goes back.
 * @returns A new local midnight date.
 * @example
 * addMonths(new Date(2026, 0, 31), 1); // Feb 28 2026
 */
export function addMonths(d: Date, n: number): Date {
  const first = new Date(d.getFullYear(), d.getMonth() + n, 1);
  const last = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
  return new Date(first.getFullYear(), first.getMonth(), Math.min(d.getDate(), last));
}

/**
 * Whether two dates fall on the same calendar day.
 *
 * @param a - First date.
 * @param b - Second date.
 */
export function sameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

/**
 * First day of the week containing `d`.
 *
 * @param d - Any date in the week.
 * @param firstDay - Weekday the week starts on, `0` Sunday to `6` Saturday.
 */
export function startOfWeek(d: Date, firstDay: number): Date {
  return addDays(d, -((d.getDay() - firstDay + 7) % 7));
}

/**
 * The 6 × 7 days shown for a month, starting on `firstDay` and padded with
 * the neighbouring months so every month has the same height.
 *
 * @param year - Full year.
 * @param month - Month index, `0` for January.
 * @param firstDay - Weekday the grid starts on, `0` Sunday to `6` Saturday.
 * @returns 42 local midnight dates.
 */
export function monthGrid(year: number, month: number, firstDay: number): Date[] {
  const start = startOfWeek(new Date(year, month, 1), firstDay);
  return Array.from({ length: 42 }, (_, i) => addDays(start, i));
}

/**
 * Weekday the user's locale starts the week on.
 *
 * @param locale - BCP 47 tag; defaults to the browser language.
 * @returns `0` Sunday to `6` Saturday. Falls back to Monday.
 */
export function firstDayOfWeek(locale?: string): number {
  try {
    const loc = new Intl.Locale(locale ?? navigator.language) as Intl.Locale & {
      getWeekInfo?: () => { firstDay: number };
      weekInfo?: { firstDay: number };
    };
    // COMPAT: Chromium ships `getWeekInfo()`; older engines exposed a `weekInfo` getter.
    const info = loc.getWeekInfo?.() ?? loc.weekInfo;
    if (info) return info.firstDay % 7;
  } catch {
    // Unknown locale tag or no `navigator` (tests); use the default below.
  }
  return 1;
}
