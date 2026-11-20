/**
 * Pure selectors over a {@link Snapshot}: what's due, what's in progress,
 * and where a task lives.
 */

import { isoDay } from "./format";
import { ACTIVE_RUN_STATES, SHIPPED_RUN_STATES, type Project, type ProjectRef, type RunRecord, type Snapshot, type Task } from "./native";

/** A task together with the project it belongs to (`null` = Inbox). */
export interface Located {
  task: Task;
  project: Project | null;
}

/** A project's tasks for the Today tab. */
export interface TodayGroup {
  project: Project | null;
  tasks: Task[];
}

/** Whether a task is open and due on or before `today` (`YYYY-MM-DD`). */
export function isDue(task: Task, today: string): boolean {
  return task.status !== "done" && task.due !== null && task.due <= today;
}

/**
 * Tasks for the Today tab: everything due today or overdue, plus everything
 * in Doing, grouped by project in activity order (Inbox last). Within a
 * group tasks follow their manual order, so drag-reordering sticks.
 *
 * @param snapshot - Current data.
 * @param now - Reference time.
 */
export function todayGroups(snapshot: Snapshot, now: Date = new Date()): TodayGroup[] {
  const today = isoDay(now);
  const pick = (tasks: Task[]) => tasks.filter((t) => isDue(t, today) || t.status === "doing").toSorted((a, b) => a.order - b.order);
  const groups: TodayGroup[] = snapshot.projects
    .map((project) => ({ project, tasks: pick(project.tasks) }))
    .filter((g) => g.tasks.length > 0);
  const inbox = pick(snapshot.inbox);
  if (inbox.length) groups.push({ project: null, tasks: inbox });
  return groups;
}

/**
 * Number of open tasks due today or earlier, across everything.
 *
 * @param snapshot - Current data.
 * @param now - Reference time.
 */
export function dueCount(snapshot: Snapshot, now: Date = new Date()): number {
  const today = isoDay(now);
  return (
    snapshot.inbox.filter((t) => isDue(t, today)).length +
    snapshot.projects.reduce((n, p) => n + p.tasks.filter((t) => isDue(t, today)).length, 0)
  );
}

/**
 * Finds a task anywhere in the snapshot.
 *
 * @param snapshot - Current data.
 * @param taskId - Task to find.
 */
export function locate(snapshot: Snapshot, taskId: string): Located | null {
  const inInbox = snapshot.inbox.find((t) => t.id === taskId);
  if (inInbox) return { task: inInbox, project: null };
  for (const project of snapshot.projects) {
    const task = project.tasks.find((t) => t.id === taskId);
    if (task) return { task, project };
  }
  return null;
}

/**
 * New `order` values after a drag reorder. The moved tasks keep occupying
 * the same set of order "slots" (just permuted), so their positions relative
 * to tasks not shown in the list are unchanged.
 *
 * @param before - Tasks in their previous order.
 * @param after - The same tasks in their new order.
 * @returns Updates only for tasks whose order actually changed.
 */
export function reorderSlots(before: readonly Task[], after: readonly Task[]): { id: string; order: number }[] {
  const slots = before.map((t) => t.order).toSorted((a, b) => a - b);
  // Equal slots (e.g. hand-written files) can't express an order; spread them.
  const distinct = new Set(slots).size === slots.length;
  return after
    .map((t, i) => ({ id: t.id, order: distinct ? slots[i]! : i + 1, prev: t.order }))
    .filter((u) => u.order !== u.prev)
    .map(({ id, order }) => ({ id, order }));
}

/**
 * `order` value for a card dropped at `index` in a column, between its new
 * neighbors. Only the moved card changes; fractional values leave room for
 * later drops.
 *
 * @param column - The destination column's tasks in display order, without the dragged card.
 * @param index - Insertion index within `column`.
 * @example
 * dropOrder([{ order: 1 }, { order: 2 }], 1); // 1.5
 */
export function dropOrder(column: readonly Pick<Task, "order">[], index: number): number {
  const prev = column[index - 1]?.order;
  const next = column[index]?.order;
  if (prev === undefined && next === undefined) return 1;
  if (prev === undefined) return next! - 1;
  if (next === undefined) return prev + 1;
  return (prev + next) / 2;
}

/** How long a finished run keeps alerting if it's never opened. */
const ALERT_WINDOW_MS = 24 * 60 * 60 * 1000;

/**
 * Status-cluster counts from agent runs: running (including queued),
 * failed and ready for review (PR open, pushed or committed). Finished runs
 * alert until seen or for 24 hours.
 *
 * @param runs - All runs.
 * @param seen - Run ids the user has looked at.
 * @param now - Reference time.
 */
export function runSignals(
  runs: readonly RunRecord[],
  seen: ReadonlySet<string>,
  now: number = Date.now(),
): { agent: number; failed: number; pr: number; approval: number } {
  const fresh = (r: RunRecord) => !seen.has(r.id) && (!r.endedAt || now - Date.parse(r.endedAt) < ALERT_WINDOW_MS);
  return {
    agent: runs.filter((r) => ACTIVE_RUN_STATES.includes(r.state)).length,
    failed: runs.filter((r) => (r.state === "failed" || r.state === "interrupted") && fresh(r)).length,
    pr: runs.filter((r) => SHIPPED_RUN_STATES.includes(r.state) && fresh(r)).length,
    approval: runs.filter((r) => r.state === "awaiting_approval").length,
  };
}

/** The {@link ProjectRef} for a located task. */
export function refOf(project: Project | null): ProjectRef {
  return project?.id ?? null;
}

/**
 * Short label for a run that stopped to wait for the user, or `null` when it
 * isn't waiting: a plan to approve, or a comparison to pick from.
 *
 * @param run - The task's latest run, if any.
 */
export function waitingLabel(run: Pick<RunRecord, "state"> | null): string | null {
  if (run?.state === "awaiting_approval") return "Plan ready";
  if (run?.state === "ready") return "Pick a version";
  return null;
}
