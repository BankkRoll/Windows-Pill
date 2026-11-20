import { useEffect, useState } from "react";

/**
 * Current time, re-rendering on each minute boundary.
 *
 * NOTE: A fresh timeout is scheduled every minute rather than using
 * `setInterval`, so the clock re-aligns after sleep/resume or timer drift.
 *
 * @returns A `Date` that updates when the displayed minute changes.
 */
export function useClock(): Date {
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    let id = 0;
    const schedule = () => {
      const d = new Date();
      const msToNextMinute = 60_000 - (d.getSeconds() * 1000 + d.getMilliseconds());
      id = window.setTimeout(() => {
        setNow(new Date());
        schedule();
      }, msToNextMinute + 20);
    };
    schedule();
    const onVisible = () => setNow(new Date());
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearTimeout(id);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, []);

  return now;
}
