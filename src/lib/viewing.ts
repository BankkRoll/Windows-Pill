/**
 * What the panel is showing, in the terms notifications care about: which
 * run and which task the user is looking at right now.
 */

import type { RunRecord, Viewing } from "./native";

/**
 * The run the Agents tab shows: the selected one if it passes the project
 * filter, else the newest visible run.
 *
 * @param runs - All runs, newest first.
 * @param projectId - Project filter from the header; `null` shows all.
 * @param selectedRunId - Run picked in the list, if any.
 * @returns The run on screen, or `null` when the list is empty.
 */
export function displayedRun(
  runs: readonly RunRecord[],
  projectId: string | null,
  selectedRunId: string | null,
): RunRecord | null {
  const visible = projectId ? runs.filter((r) => r.projectId === projectId) : runs;
  return visible.find((r) => r.id === selectedRunId) ?? visible[0] ?? null;
}

/** The parts of the UI store {@link viewingOf} reads. */
export interface ViewState {
  activeTab: string;
  /** Task whose page is open; it covers whatever tab is behind it. */
  detailTaskId: string | null;
  runs: readonly RunRecord[];
  selectedProjectId: string | null;
  selectedRunId: string | null;
}

/**
 * The run and task on screen, or `null` when nothing a notification could
 * be about is showing.
 *
 * @param s - Current panel state.
 * @example
 * viewingOf({ activeTab: "board", detailTaskId: "t_1", runs: [], selectedProjectId: null, selectedRunId: null });
 * // => { runId: null, taskId: "t_1" }
 */
export function viewingOf(s: ViewState): Viewing | null {
  if (s.detailTaskId) return { runId: null, taskId: s.detailTaskId };
  if (s.activeTab !== "agents") return null;
  const run = displayedRun(s.runs, s.selectedProjectId, s.selectedRunId);
  return run ? { runId: run.id, taskId: run.taskId } : null;
}

/**
 * Whether two views point at the same run and task.
 *
 * @param a - One view, or `null`.
 * @param b - Another view, or `null`.
 */
export function sameViewing(a: Viewing | null, b: Viewing | null): boolean {
  return a?.runId === b?.runId && a?.taskId === b?.taskId;
}
