import { useEffect, useState } from "react";

/**
 * Current time in milliseconds, refreshed every `intervalMs`.
 *
 * @param intervalMs - Refresh period (e.g. 1000 for a live elapsed timer).
 * @returns `Date.now()` as of the last tick.
 */
export function useNow(intervalMs: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}
