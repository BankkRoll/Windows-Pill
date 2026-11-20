/**
 * Ordering between one-off fetches and the event stream that keeps the
 * same state current.
 */

/** Tracks which pieces of state events have written since a fetch began. */
export interface Freshness {
  /** Records that an event wrote `key`. */
  touch(key: string): void;
  /**
   * Starts a fetch for `key`. The returned function applies a result only
   * if no event wrote `key` since, because the event is newer than the
   * snapshot the fetch was answered from.
   */
  guard<T>(key: string, apply: (value: T) => void): (value: T) => void;
}

/**
 * Creates a {@link Freshness} tracker.
 *
 * @example
 * const fresh = freshness();
 * subscribe(EVENTS.runs, (runs) => { fresh.touch("runs"); setRuns(runs); });
 * void api.listRuns().then(fresh.guard("runs", setRuns));
 */
export function freshness(): Freshness {
  const writes = new Map<string, number>();
  return {
    touch(key) {
      writes.set(key, (writes.get(key) ?? 0) + 1);
    },
    guard(key, apply) {
      const seen = writes.get(key) ?? 0;
      return (value) => {
        if ((writes.get(key) ?? 0) === seen) apply(value);
      };
    },
  };
}
