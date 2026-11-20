/**
 * Commands for the command bar: what can be run, and how typed text ranks
 * them. Commands are built from the current state each time the bar opens,
 * so project and task commands always match what exists.
 */

import { compareAgents, openEditor, openFolder, popOut, startAgent } from "./actions";
import { useChatStore } from "../chatStore";
import { api, type Appearance, type PillConfig, type Project, type Snapshot, type Task } from "./native";
import { SURFACES, type Palette } from "./theme";
import { usePillStore, type TabId } from "../store";
import { pluginTabId, pluginTabs } from "./pages";

/** Visual group a command belongs to. */
export type CommandGroup = "Go to" | "Projects" | "Agents" | "Chat" | "Pill";

/** One runnable command. */
export interface Command {
  id: string;
  group: CommandGroup;
  title: string;
  /** Secondary text, e.g. the project a task belongs to. */
  hint?: string;
  /** Extra words that should match, beyond the title. */
  keywords?: string;
  /** Project color dot, when the command is about a project. */
  color?: string;
  run: () => void | Promise<void>;
}

/**
 * Scores how well `query` matches `text`: whole-word prefixes beat
 * substrings, which beat scattered letters. `0` means no match.
 *
 * @param query - What the user typed (any case).
 * @param text - Candidate text.
 * @returns A score; higher is better.
 * @example matchScore("brd", "Go to Board") > 0 // scattered letters still match
 */
export function matchScore(query: string, text: string): number {
  const q = query.trim().toLowerCase();
  const t = text.toLowerCase();
  if (!q) return 1;
  const words = q.split(/\s+/);
  let score = 0;
  for (const w of words) {
    if (new RegExp(`(^|[\\s/_.-])${w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`).test(t)) score += 30;
    else if (t.includes(w)) score += 15;
    else {
      // Letters in order, anywhere.
      let i = 0;
      for (const ch of t) if (ch === w[i]) i++;
      if (i < w.length) return 0;
      score += 4;
    }
  }
  // Shorter candidates are closer matches for the same words.
  return score + Math.max(0, 10 - t.length / 8);
}

/**
 * Ranks commands for a query, best first.
 *
 * @param query - Typed text.
 * @param commands - All commands.
 * @param limit - Maximum to return.
 */
export function rankCommands(query: string, commands: readonly Command[], limit = 8): Command[] {
  return commands
    .map((c) => ({ c, s: Math.max(matchScore(query, c.title), matchScore(query, `${c.title} ${c.keywords ?? ""} ${c.hint ?? ""}`) - 5) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s)
    .slice(0, limit)
    .map((x) => x.c);
}

const TABS: readonly { id: TabId; title: string; keywords: string }[] = [
  { id: "today", title: "Today", keywords: "due doing home" },
  { id: "board", title: "Board", keywords: "kanban columns tasks" },
  { id: "chat", title: "Chat", keywords: "ask conversation notes" },
  { id: "agents", title: "Agents", keywords: "runs prs" },
  { id: "settings", title: "Settings", keywords: "preferences options config" },
];

const APPEARANCE_COMMANDS: readonly (readonly [Appearance, string, string])[] = [
  ["dark", "Dark mode", "appearance theme night black"],
  ["light", "Light mode", "appearance theme day white bright"],
  ["system", "Match Windows light or dark mode", "appearance theme system auto follow"],
];

/**
 * Every command available right now.
 *
 * @param snapshot - Projects and tasks.
 * @param config - App config (for theme commands).
 * @param selected - Project picked in the header, if any.
 * @param palette - Palette in use, which names the surface styles.
 */
export function buildCommands(snapshot: Snapshot, config: PillConfig, selected: Project | null, palette: Palette = "dark"): Command[] {
  const s = () => usePillStore.getState();
  const out: Command[] = [];

  for (const t of TABS) {
    out.push({ id: `tab:${t.id}`, group: "Go to", title: t.title, keywords: t.keywords, run: () => s().setActiveTab(t.id) });
  }
  for (const p of pluginTabs(s().plugins)) {
    const tab = pluginTabId(p.id);
    out.push({ id: `tab:${tab}`, group: "Go to", title: p.tab.title, keywords: `${p.name} plugin`, run: () => s().setActiveTab(tab) });
  }

  for (const p of snapshot.projects) {
    out.push({
      id: `project:${p.id}`,
      group: "Projects",
      title: p.name,
      hint: "Focus project",
      keywords: "open switch project",
      color: p.color,
      run: () => s().selectProject(p.id),
    });
    out.push({ id: `editor:${p.id}`, group: "Projects", title: `Open ${p.name} in editor`, keywords: "code cursor vscode", color: p.color, run: () => void openEditor(p) });
    out.push({ id: `folder:${p.id}`, group: "Projects", title: `Open ${p.name} folder`, keywords: "explorer files", color: p.color, run: () => void openFolder(p) });
  }
  out.push({ id: "projects:all", group: "Projects", title: "All projects", keywords: "clear focus everything", run: () => s().selectProject(null) });

  const openTasks: { project: Project; task: Task }[] = snapshot.projects.flatMap((project) =>
    project.tasks.filter((t) => t.status !== "done").map((task) => ({ project, task })),
  );
  for (const { project, task } of openTasks) {
    out.push({
      id: `agent:${task.id}`,
      group: "Agents",
      title: `Start agent: ${task.title}`,
      hint: project.name,
      keywords: "run build fix",
      color: project.color,
      run: () => void startAgent(project, task),
    });
    out.push({
      id: `compare:${task.id}`,
      group: "Agents",
      title: `Compare Claude Code vs Codex: ${task.title}`,
      hint: project.name,
      keywords: "both versus side by side",
      color: project.color,
      run: () =>
        void compareAgents(project, task, [
          { provider: "claude", model: "", effort: "" },
          { provider: "codex", model: "", effort: "" },
        ]),
    });
  }

  if (selected) {
    out.push({
      id: "chat:new",
      group: "Chat",
      title: `New chat about ${selected.name}`,
      keywords: "ask talk conversation",
      color: selected.color,
      run: () => {
        useChatStore.getState().open(selected.id, null);
        s().setActiveTab("chat");
      },
    });
  }

  out.push({ id: "pill:popout", group: "Pill", title: "Pop out into a window", keywords: "detach window bigger", run: () => popOut(s().activeTab, s().selectedProjectId) });
  out.push({ id: "pill:rescan", group: "Pill", title: "Rescan projects", keywords: "refresh find repos", run: () => void api.rescan() });
  for (const theme of ["glass", "graphite", "black"] as const) {
    if (config.theme !== theme) {
      out.push({
        id: `theme:${theme}`,
        group: "Pill",
        title: `Theme: ${SURFACES[theme][palette].label}`,
        keywords: `appearance look surface ${theme}`,
        run: () => void api.updateSettings({ theme }),
      });
    }
  }
  for (const [appearance, title, keywords] of APPEARANCE_COMMANDS) {
    if (config.appearance !== appearance) {
      out.push({ id: `appearance:${appearance}`, group: "Pill", title, keywords, run: () => void api.updateSettings({ appearance }) });
    }
  }
  return out;
}
