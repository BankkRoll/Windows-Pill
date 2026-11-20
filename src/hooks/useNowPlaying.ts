import { useEffect, useState } from "react";
import { mediaApi, subscribe, type NowPlaying } from "../lib/native";

/**
 * Now playing's timeline, modes and session list, kept current from the
 * core's media event.
 *
 * @returns The latest state, or `null` when nothing plays (or before the first read).
 */
export function useNowPlaying(): NowPlaying | null {
  const [state, setState] = useState<NowPlaying | null>(null);
  useEffect(() => {
    let disposed = false;
    let heard = false;
    const off = subscribe<NowPlaying | null>(mediaApi.event, (next) => {
      heard = true;
      setState(next);
    });
    void mediaApi
      .state()
      .then((next) => {
        // NOTE: an event that arrived while the read was in flight is newer.
        if (!disposed && !heard) setState(next);
      })
      .catch(() => {});
    return () => {
      disposed = true;
      off();
    };
  }, []);
  return state;
}
