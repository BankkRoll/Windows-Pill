import { useEffect } from "react";
import { api, type LogLine } from "../lib/native";
import { usePillStore } from "../store";

const NONE: readonly LogLine[] = [];

/**
 * A run's activity log: the saved history, loaded once per run, followed by
 * the lines that keep arriving live. A window opened mid-run gets the
 * earlier transcript too, not just what was logged after it opened.
 *
 * @param runId - Run to show, or `null` for none.
 * @returns The run's lines, oldest first.
 */
export function useRunLog(runId: string | null): readonly LogLine[] {
  const lines = usePillStore((s) => (runId ? s.logs[runId] : undefined));
  const loaded = usePillStore((s) => runId === null || s.logHistory.has(runId));
  const loadLog = usePillStore((s) => s.loadLog);

  useEffect(() => {
    if (runId === null || loaded) return;
    void api.runLog(runId).then(
      (history) => loadLog(runId, history),
      () => loadLog(runId, []),
    );
  }, [runId, loaded, loadLog]);

  return lines ?? NONE;
}
