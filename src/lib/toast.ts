/**
 * Timing for the panel's toast line.
 */

/** How long a toast stays up, by tone; errors get longer to be read. */
export const TOAST_MS = { info: 4_000, error: 7_000 } as const;

/**
 * How much longer a toast should stay up. A toast's time runs from when it
 * was raised, not from when a view first shows it, so one raised while no
 * view could show it (the notch collapsed after a quick-add) doesn't pop up
 * long after the fact.
 *
 * @param toast - The toast's tone and `Date.now()` when it was raised.
 * @param now - Current time in ms.
 * @returns Milliseconds left; zero or less means it is stale.
 * @example
 * toastTimeLeft({ tone: "info", at: 1_000 }, 2_000); // 3_000
 */
export function toastTimeLeft(toast: { tone: keyof typeof TOAST_MS; at: number }, now: number): number {
  return toast.at + TOAST_MS[toast.tone] - now;
}
