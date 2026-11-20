/**
 * The command bar's AI: types for its proposed actions, how they read in the
 * preview, and what approving them does. Nothing here runs until the user
 * approves a preview.
 */

import { invoke } from "@tauri-apps/api/core";
import { useChatStore } from "../chatStore";
import { usePillStore } from "../store";
import { attempt, showRun } from "./actions";
import { api, chatApi, type AiProvider, type RunChoice, type Snapshot } from "./native";

/** What happens to a created task. */
export type StartMode = "none" | "agent" | "compare";

/** One proposed action. Mirrors Rust `assistant::Action`. */
export type AssistantAction =
  | { type: "create_task"; projectId: string | null; title: string; notes: string; priority: 1 | 2 | 3 | null; due: string | null; start: StartMode }
  | { type: "start_agent"; projectId: string; taskId: string; compare: boolean }
  | { type: "chat"; projectId: string; message: string };

/** The assistant's answer. Mirrors Rust `assistant::Plan`. */
export interface AssistantPlan {
  reply: string;
  actions: AssistantAction[];
  provider: AiProvider;
  model: string | null;
}

/** Asks the assistant. Resolves when it has answered. */
export const askAssistant = (text: string, choice: RunChoice | null) => invoke<AssistantPlan>("ask_assistant", { text, choice });

/** Stops the assistant request in progress. */
export const stopAssistant = () => invoke<void>("stop_assistant");

const BOTH: RunChoice[] = [
  { provider: "claude", model: "", effort: "" },
  { provider: "codex", model: "", effort: "" },
];

/**
 * A one-line description of an action for the preview.
 *
 * @param a - Action.
 * @param snapshot - For project and task names.
 */
export function describeAction(a: AssistantAction, snapshot: Snapshot): { title: string; where: string; color?: string } {
  const project = (id: string | null) => snapshot.projects.find((p) => p.id === id);
  switch (a.type) {
    case "create_task": {
      const p = project(a.projectId);
      return { title: a.title, where: p?.name ?? "Inbox", color: p?.color };
    }
    case "start_agent": {
      const p = project(a.projectId);
      const task = p?.tasks.find((t) => t.id === a.taskId);
      return { title: `${a.compare ? "Compare agents on" : "Start agent on"} “${task?.title ?? a.taskId}”`, where: p?.name ?? "", color: p?.color };
    }
    case "chat": {
      const p = project(a.projectId);
      return { title: `Ask in chat: ${a.message}`, where: p?.name ?? "", color: p?.color };
    }
  }
}

/**
 * Carries out approved actions in order. Failures are reported and skipped,
 * so one bad action doesn't stop the rest.
 *
 * @param actions - Approved actions.
 * @returns How many were carried out.
 */
export async function runActions(actions: readonly AssistantAction[]): Promise<number> {
  let done = 0;
  let started: { projectId: string; runId: string } | null = null;
  for (const a of actions) {
    switch (a.type) {
      case "create_task": {
        const task = await attempt(() =>
          api.createTask(a.projectId, {
            title: a.title,
            notes: a.notes,
            priority: a.priority,
            due: a.due,
            status: "todo",
          }),
        );
        if (!task) break;
        done++;
        const projectId = a.projectId;
        if (projectId && a.start === "agent") {
          const run = await attempt(() => api.startAgent(projectId, task.id));
          if (run) started = { projectId, runId: run.id };
        }
        if (projectId && a.start === "compare") {
          const runs = await attempt(() => api.compareAgents(projectId, task.id, BOTH));
          if (runs?.[0]) started = { projectId, runId: runs[0].id };
        }
        break;
      }
      case "start_agent": {
        const runId = a.compare
          ? (await attempt(() => api.compareAgents(a.projectId, a.taskId, BOTH)))?.[0]?.id
          : (await attempt(() => api.startAgent(a.projectId, a.taskId)))?.id;
        if (runId) {
          done++;
          started = { projectId: a.projectId, runId };
        }
        break;
      }
      case "chat": {
        const chat = await attempt(() => chatApi.create(a.projectId, "read"));
        if (!chat) break;
        done++;
        const s = usePillStore.getState();
        s.selectProject(a.projectId);
        s.setActiveTab("chat");
        useChatStore.getState().open(a.projectId, chat.id);
        await attempt(() => chatApi.send(a.projectId, chat.id, a.message, null));
        await useChatStore.getState().refresh(a.projectId);
        break;
      }
    }
  }
  // A chat action already switched to Chat; otherwise land on the run that started.
  if (started && !actions.some((a) => a.type === "chat")) showRun(started.projectId, started.runId);
  return done;
}
