import { ACTIVE_RUN_STATES, type RunRecord } from "../lib/native";
import { usePillStore } from "../store";

/**
 * The most recent agent run for a task, and whether it's still going.
 *
 * @param taskId - Task to look up.
 * @returns `{ run, active, needsYou }`; `run` is `null` when the task has
 * never run, and `needsYou` is true while its agent waits for an approval.
 */
export function useTaskRun(taskId: string): { run: RunRecord | null; active: boolean; needsYou: boolean } {
  const run = usePillStore((s) => s.runs.find((r) => r.taskId === taskId) ?? null);
  const needsYou = usePillStore((s) => s.approvals.some((a) => a.taskId === taskId));
  return { run, active: !!run && ACTIVE_RUN_STATES.includes(run.state), needsYou };
}

/**
 * Returns a function that jumps to a task's latest run in the Agents tab.
 */
export function useShowRun(): (taskId: string) => void {
  return (taskId) => {
    const s = usePillStore.getState();
    const run = s.runs.find((r) => r.taskId === taskId);
    if (run) s.selectRun(run.id);
    s.openDetail(null);
    s.setActiveTab("agents");
  };
}
