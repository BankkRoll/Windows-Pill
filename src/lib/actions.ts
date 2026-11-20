/**
 * User-facing task operations with toasts and undo, shared by every view.
 */

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { type TabId, usePillStore } from "../store";
import { pageKey, pluginTabId, usePageStore } from "./pages";
import {
  api,
  errorMessage,
  isDetached,
  pluginApi,
  type GhIssue,
  type NewTask,
  type Project,
  type RunChoice,
  type SystemPage,
  type Task,
  type TaskStatus,
} from "./native";
import { isWebUrl } from "./format";
import { refOf } from "./tasks";

function toast(message: string, undo?: () => void) {
  usePillStore.getState().showToast({ message, tone: "info", undo });
}

/**
 * Shows a failed operation's message as an error toast.
 *
 * @param e - Rejection from an `api` call.
 */
export function reportError(e: unknown) {
  usePillStore.getState().showToast({ message: errorMessage(e), tone: "error" });
}

/**
 * Runs an operation, turning failures into an error toast.
 *
 * @param op - Async operation.
 * @returns The result, or `undefined` if it failed.
 */
export async function attempt<T>(op: () => Promise<T>): Promise<T | undefined> {
  try {
    return await op();
  } catch (e) {
    reportError(e);
    return undefined;
  }
}

/**
 * Creates a task and offers to undo it.
 *
 * @param project - Destination, or `null` for the Inbox.
 * @param draft - Task fields.
 */
export async function addTask(project: Project | null, draft: NewTask): Promise<Task | undefined> {
  const created = await attempt(() => api.createTask(refOf(project), draft));
  if (created) {
    toast(`Added to ${project?.name ?? "Inbox"}`, () => void attempt(() => api.deleteTask(refOf(project), created.id)));
  }
  return created;
}

/**
 * Moves a task to a new column, with undo.
 *
 * @param project - Where the task lives.
 * @param task - Task to change.
 * @param status - New column.
 */
export async function setStatus(project: Project | null, task: Task, status: TaskStatus) {
  const previous = task.status;
  const ok = await attempt(() => api.updateTask(refOf(project), task.id, { status }));
  if (ok && status === "done") {
    toast(`Completed “${task.title}”`, () => void attempt(() => api.updateTask(refOf(project), task.id, { status: previous })));
  }
}

/**
 * Opens the project folder in File Explorer.
 *
 * @param project - Project to open.
 */
export async function openFolder(project: Project) {
  await openOutside(() => api.openProjectFolder(project.id));
}

/**
 * Opens the project in its editor (Cursor, VS Code or Explorer).
 *
 * @param project - Project to open.
 */
export async function openEditor(project: Project) {
  const label = await openOutside(() => api.openInEditor(project.id));
  if (label) toast(`Opening ${project.name} in ${label}`);
}

/**
 * Copies the exact prompt an agent would receive for this task.
 *
 * @param project - Project the task belongs to.
 * @param task - Task to render.
 */
export async function copyPrompt(project: Project, task: Task) {
  const text = await attempt(() => api.renderTaskPrompt(project.id, task.id));
  if (text && (await attempt(() => writeText(text).then(() => true)))) toast("Prompt copied");
}

/**
 * Starts an agent on a task and offers to jump to its live log.
 *
 * @param project - Project the task belongs to.
 * @param task - Task to work on.
 * @param plan - Plan first (overrides the project default when set).
 */
export async function startAgent(project: Project, task: Task, plan?: boolean, choice?: RunChoice) {
  const run = await attempt(() => api.startAgent(project.id, task.id, plan, choice));
  if (!run) return;
  showRun(project.id, run.id);
  usePillStore.getState().showToast({
    message: run.planMode ? `Planning “${task.title}”` : `Agent started on “${task.title}”`,
    tone: "info",
  });
}

/**
 * Starts the same task once per setup to compare the results side by side.
 * Nothing is pushed until one run is picked in the Agents tab.
 *
 * @param project - Project the task belongs to.
 * @param task - Task to run.
 * @param choices - One setup per run, e.g. Claude Code and Codex.
 */
export async function compareAgents(project: Project, task: Task, choices: RunChoice[]) {
  const runs = await attempt(() => api.compareAgents(project.id, task.id, choices));
  const first = runs?.[0];
  if (!first) return;
  showRun(project.id, first.id);
  usePillStore.getState().showToast({ message: `Comparing ${runs.length} setups on “${task.title}”`, tone: "info" });
}

/**
 * Switches to the Agents tab with a run open. If the header is focused on a
 * different project, it switches there too so the run isn't filtered out.
 *
 * @param projectId - Project the run belongs to.
 * @param runId - Run to open.
 */
export function showRun(projectId: string, runId: string) {
  const s = usePillStore.getState();
  if (s.selectedProjectId && s.selectedProjectId !== projectId) s.selectProject(projectId);
  s.selectRun(runId);
  s.openDetail(null);
  s.setActiveTab("agents");
}

/**
 * Deletes a task. Undo re-creates it with the same content.
 *
 * @param project - Where the task lives.
 * @param task - Task to delete.
 */
export async function removeTask(project: Project | null, task: Task) {
  const ok = await attempt(() => api.deleteTask(refOf(project), task.id).then(() => true));
  if (ok) {
    toast(
      `Deleted “${task.title}”`,
      () =>
        void attempt(() =>
          api.createTask(refOf(project), {
            title: task.title,
            notes: task.notes,
            status: task.status,
            priority: task.priority,
            tags: task.tags,
            due: task.due,
            agentReady: task.agentReady,
          }),
        ),
    );
  }
}

/**
 * Runs something that opens outside Pill (a folder, an editor, a browser)
 * and then closes the notch, since the user's attention has moved there.
 * The detached window is a normal app window and stays as it is.
 *
 * @param op - The opening action.
 * @returns The action's result, or `undefined` if it failed (already reported).
 */
export async function openOutside<T>(op: () => Promise<T>): Promise<T | undefined> {
  // Boxed so an action that resolves to undefined still counts as success.
  const boxed = await attempt(async () => ({ value: await op() }));
  if (boxed && !isDetached) usePillStore.getState().collapse();
  return boxed?.value;
}

/**
 * Opens the pop-out window on a tab and closes the notch, which would
 * otherwise stay open over the new window while the cursor rests on it.
 *
 * @param tab - Tab to show.
 * @param projectId - Project to focus, if any.
 */
export function popOut(tab: TabId, projectId: string | null) {
  void openOutside(() => api.openDetached(tab, projectId));
}

/**
 * Shows a plugin's page in its header tab: one project's page, or the
 * overview with `null`. Picks the project in the header too, since the
 * tab's sidebar and the header picker are one selection.
 *
 * @param pluginId - A plugin with a tab.
 * @param projectId - Project to show, or `null` for all projects.
 * @param row - A row to scroll to and unfold, if the page has one with this
 *   id. Plugins give an alert's row the alert's own id for this.
 * @example openPluginPage("cve-watch", project.id, item.id)
 */
export function openPluginPage(pluginId: string, projectId: string | null, row?: string) {
  usePageStore.getState().setFocus(row ? { key: pageKey(pluginId, projectId), row } : null);
  const s = usePillStore.getState();
  s.selectProject(projectId);
  s.setActiveTab(pluginTabId(pluginId));
}

/**
 * Opens an http(s) link in the default browser and closes the notch.
 *
 * @param url - Link to open; anything but http(s) is ignored.
 */
export function openLink(url: string) {
  if (isWebUrl(url)) void openOutside(() => openUrl(url));
}

/**
 * Shows a file in Explorer and closes the notch.
 *
 * @param path - File to reveal.
 */
export function revealFile(path: string) {
  void openOutside(() => revealItemInDir(path));
}

/**
 * Adds an assigned GitHub issue to the board as a task. An issue that
 * already has a task just returns it.
 *
 * @param project - Project the issue belongs to.
 * @param issue - Issue to add.
 * @param status - Column for the new task.
 * @returns The linked task, or `undefined` if it failed (already reported).
 */
export async function trackIssue(project: Project, issue: GhIssue, status: TaskStatus = "todo"): Promise<Task | undefined> {
  const task = await attempt(() => api.trackIssue(project.id, issue.number, status));
  if (task) toast(`Added #${issue.number} to ${project.name}`);
  return task;
}

/**
 * Turns a GitHub issue into a task (or finds its task) and starts an agent
 * on it, then shows the run.
 *
 * @param project - Project the issue belongs to.
 * @param issue - Issue to work on.
 * @param plan - Plan first (overrides the project default when set).
 * @param choice - Provider, model and effort for the run.
 */
export async function startIssueAgent(project: Project, issue: GhIssue, plan?: boolean, choice?: RunChoice) {
  const run = await attempt(() => api.startIssue(project.id, issue.number, plan, choice));
  if (!run) return;
  showRun(project.id, run.id);
  usePillStore.getState().showToast({
    message: run.planMode ? `Planning #${issue.number}` : `Agent started on #${issue.number}`,
    tone: "info",
  });
}

/**
 * Opens a Windows Settings page (privacy, speech, sound) and closes the
 * notch, since the fix happens there.
 *
 * @param page - Page from the core's fixed list.
 */
export function openSystemSettings(page: SystemPage) {
  void openOutside(() => pluginApi.openSystemSettings(page));
}

/** Opens the external plugins folder in Explorer, creating it if needed. */
export function openPluginsFolder() {
  void openOutside(() => pluginApi.openFolder());
}
