/**
 * UI state. The Rust core owns all persistent data; this store mirrors what
 * the core sends plus purely visual state (open tab, focus holds, toasts).
 */

import { create } from "zustand";
import {
  DEFAULT_CONFIG,
  type Approval,
  type Backdrop,
  type LogLine,
  type NotificationFeed,
  type PillConfig,
  type PluginIcon,
  type PluginState,
  type PluginTone,
  type ProjectGithub,
  type RunRecord,
  type Snapshot,
} from "./lib/native";
import type { ShapeMode } from "./lib/shapes";
import { mergeLog } from "./lib/transcript";

/** Categories of things that need attention, each with its own icon and color. */
export type SignalKind = "agent" | "pr" | "failed" | "approval" | "security" | "due" | "plugin" | "notification";

/** Look of a signal drawn by an external or built-in plugin. */
export interface PluginSignalStyle {
  /** Stable identity across updates: `pluginId:signalId`. */
  key: string;
  icon: PluginIcon;
  tone: PluginTone;
  /** Tooltip. */
  label: string;
}

/** One category shown in the collapsed notch's status cluster. */
export interface Signal {
  kind: SignalKind;
  /** How many items of this kind; shown as a badge when above 1. */
  count: number;
  /** Set on `plugin` signals only; each one stays separate in the cluster. */
  plugin?: PluginSignalStyle;
}

/** Cluster order: most urgent first, so a narrow notch keeps what matters. */
export const SIGNAL_ORDER: readonly SignalKind[] = ["failed", "approval", "security", "pr", "agent", "due", "plugin", "notification"];

/** Panels reachable from the expanded pill; `plugin:<id>` is a plugin's tab. */
export type TabId = "today" | "board" | "chat" | "agents" | "settings" | `plugin:${string}`;

/** Pages in the Settings rail. `project` edits the project picked in the header. */
export type SettingsSection = "project" | "projects" | "agents" | "accounts" | "plugins" | "general";

/** A short message at the bottom of the panel, optionally undoable. */
export interface Toast {
  id: number;
  message: string;
  tone: "info" | "error";
  /** `Date.now()` when it was raised. */
  at: number;
  /** Optional button: runs `undo`, labelled `actionLabel` (default "Undo"). */
  undo?: () => void;
  actionLabel?: string;
}

/** A GitHub issue open in the detail page. */
export interface IssueRef {
  projectId: string;
  number: number;
}

interface PillStore {
  config: PillConfig;
  configError: string | null;
  hotkeyError: string | null;
  /** Absolute path of `config.json`, shown in Settings → General. */
  configPath: string;
  snapshot: Snapshot;
  /** Page open in the Settings rail. */
  settingsSection: SettingsSection;
  /** Hotkey quick-add is showing its suggestion list or an AI answer, so the notch grows. */
  commandTall: boolean;
  setCommandTall: (on: boolean) => void;
  /** Bumped to close the notch at once, e.g. after opening something outside it. */
  collapseSeq: number;
  collapse: () => void;
  /** Detached window only: true while it fills its monitor (Pill's maximize). */
  filled: boolean;
  setFilled: (on: boolean) => void;
  /** True while another window is dragged; the notch hides for Windows Snap. */
  stepAside: boolean;
  setStepAside: (on: boolean) => void;
  /** Raw wallpaper sample of this window's monitor, for the glass theme. */
  backdrop: Backdrop | null;
  setBackdrop: (backdrop: Backdrop | null) => void;
  /** Whether Windows asks apps for light mode; resolves the `system` appearance. */
  systemLight: boolean;
  setSystemLight: (systemLight: boolean) => void;
  activeTab: TabId;
  /** Project the panel is focused on; `null` means all projects. */
  selectedProjectId: string | null;
  /** True while the hotkey-driven quick-add input is showing. */
  quickAdd: boolean;
  /** Signals per source (tasks, agents, plugins); merged into {@link signals}. */
  signalSources: Readonly<Record<string, readonly Signal[]>>;
  /** Merged, ordered attention signals. */
  signals: readonly Signal[];
  /** Every plugin's state, in Settings order. */
  plugins: readonly PluginState[];
  setPlugins: (plugins: readonly PluginState[]) => void;
  /** What the Windows notifications plugin lists; `null` while it's off or can't read Action Center. */
  notifications: NotificationFeed | null;
  setNotifications: (notifications: NotificationFeed | null) => void;
  /** Reasons the panel must stay open even without hover (inputs, drags, dialogs). */
  holds: ReadonlySet<string>;
  /** Task whose detail sheet is open. */
  detailTaskId: string | null;
  /** GitHub issue whose detail page is open; exclusive with {@link detailTaskId}. */
  detailIssue: IssueRef | null;
  openIssue: (ref: IssueRef | null) => void;
  /** GitHub issues, PR status and sync status per project id. */
  github: Readonly<Record<string, ProjectGithub>>;
  setGithub: (github: Readonly<Record<string, ProjectGithub>>) => void;
  /** Tool uses waiting for the user's approval, oldest first. */
  approvals: readonly Approval[];
  setApprovals: (approvals: readonly Approval[]) => void;
  /** Every agent run, newest first. */
  runs: readonly RunRecord[];
  /** Activity per run id (most recent lines): the saved log once loaded, plus everything received live. */
  logs: Readonly<Record<string, readonly LogLine[]>>;
  /** Runs whose saved log has been merged into {@link logs}. */
  logHistory: ReadonlySet<string>;
  /** Run selected in the Agents tab. */
  selectedRunId: string | null;
  /** Finished runs the user has already looked at, so their alerts clear. */
  seenRuns: ReadonlySet<string>;
  setRuns: (runs: readonly RunRecord[]) => void;
  /** Merges a run's saved log in front of the live lines already received. */
  loadLog: (runId: string, history: readonly LogLine[]) => void;
  appendLog: (line: LogLine) => void;
  selectRun: (runId: string | null) => void;
  markSeen: (runId: string) => void;
  toast: Toast | null;
  openDetail: (taskId: string | null) => void;
  setConfig: (config: PillConfig, error?: string | null) => void;
  setHotkeyError: (error: string | null) => void;
  setConfigPath: (path: string) => void;
  setSettingsSection: (section: SettingsSection) => void;
  setSnapshot: (snapshot: Snapshot) => void;
  setActiveTab: (tab: TabId) => void;
  selectProject: (id: string | null) => void;
  setQuickAdd: (open: boolean) => void;
  setSignals: (source: string, signals: readonly Signal[]) => void;
  hold: (reason: string, on: boolean) => void;
  showToast: (toast: Omit<Toast, "id" | "at">) => void;
  dismissToast: (id: number) => void;
}

let toastSeq = 0;
const LOG_CAP = 400;
const SEEN_KEY = "pill.seenRuns";

function loadSeen(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(SEEN_KEY) ?? "[]") as string[]);
  } catch {
    return new Set();
  }
}

function saveSeen(seen: ReadonlySet<string>) {
  try {
    // Keep the list bounded; old runs drop out of the alert window anyway.
    localStorage.setItem(SEEN_KEY, JSON.stringify([...seen].slice(-500)));
  } catch {
    // Storage can be unavailable; alerts then just stay until the run ages out.
  }
}

/**
 * Merges per-source signals: counts of the same kind add up, empty kinds
 * drop out, and the result follows {@link SIGNAL_ORDER}. Plugin signals keep
 * their own icon, so they stay separate (in source order) at the `plugin`
 * position.
 *
 * @param sources - Signals keyed by source.
 */
export function mergeSignals(sources: Readonly<Record<string, readonly Signal[]>>): Signal[] {
  const totals = new Map<SignalKind, number>();
  const custom: Signal[] = [];
  for (const list of Object.values(sources)) {
    for (const s of list) {
      if (s.kind === "plugin") {
        if (s.plugin && s.count > 0) custom.push(s);
      } else {
        totals.set(s.kind, (totals.get(s.kind) ?? 0) + s.count);
      }
    }
  }
  return SIGNAL_ORDER.flatMap((kind): Signal[] =>
    kind === "plugin" ? custom : (totals.get(kind) ?? 0) > 0 ? [{ kind, count: totals.get(kind)! }] : [],
  );
}

/** Global UI store. */
export const usePillStore = create<PillStore>((set) => ({
  config: DEFAULT_CONFIG,
  configError: null,
  hotkeyError: null,
  configPath: "",
  settingsSection: "projects",
  commandTall: false,
  setCommandTall: (commandTall) => set((s) => (s.commandTall === commandTall ? s : { commandTall })),
  collapseSeq: 0,
  // Holds (focused inputs, open menus) would keep the panel up, so they go too.
  collapse: () => set((s) => ({ collapseSeq: s.collapseSeq + 1, holds: new Set<string>() })),
  filled: false,
  setFilled: (filled) => set({ filled }),
  stepAside: false,
  setStepAside: (stepAside) => set({ stepAside }),
  backdrop: null,
  setBackdrop: (backdrop) => set({ backdrop }),
  systemLight: false,
  setSystemLight: (systemLight) => set({ systemLight }),
  snapshot: { projects: [], inbox: [], inboxError: null },
  activeTab: "today",
  selectedProjectId: null,
  quickAdd: false,
  signalSources: {},
  signals: [],
  plugins: [],
  setPlugins: (plugins) => set({ plugins }),
  notifications: null,
  setNotifications: (notifications) => set({ notifications }),
  holds: new Set(),
  detailTaskId: null,
  detailIssue: null,
  openIssue: (detailIssue) => set({ detailIssue, detailTaskId: null }),
  github: {},
  setGithub: (github) => set({ github }),
  approvals: [],
  setApprovals: (approvals) => set({ approvals }),
  runs: [],
  logs: {},
  selectedRunId: null,
  seenRuns: loadSeen(),
  setRuns: (runs) => set({ runs }),
  logHistory: new Set(),
  loadLog: (runId, history) =>
    set((s) => ({
      logs: { ...s.logs, [runId]: mergeLog(history, s.logs[runId] ?? []).slice(-LOG_CAP) },
      logHistory: new Set(s.logHistory).add(runId),
    })),
  appendLog: (line) =>
    set((s) => {
      const prev = s.logs[line.runId] ?? [];
      return { logs: { ...s.logs, [line.runId]: [...prev, line].slice(-LOG_CAP) } };
    }),
  selectRun: (selectedRunId) => set({ selectedRunId }),
  markSeen: (runId) =>
    set((s) => {
      if (s.seenRuns.has(runId)) return s;
      const seenRuns = new Set(s.seenRuns).add(runId);
      saveSeen(seenRuns);
      return { seenRuns };
    }),
  toast: null,
  openDetail: (detailTaskId) => set({ detailTaskId, detailIssue: null }),
  setConfig: (config, error = null) => set({ config, configError: error }),
  setHotkeyError: (hotkeyError) => set({ hotkeyError }),
  setConfigPath: (configPath) => set({ configPath }),
  setSettingsSection: (settingsSection) => set({ settingsSection }),
  setSnapshot: (snapshot) =>
    set((s) => ({
      snapshot,
      // Drop a selection whose project disappeared (deleted or root removed).
      selectedProjectId: s.selectedProjectId && snapshot.projects.some((p) => p.id === s.selectedProjectId) ? s.selectedProjectId : null,
    })),
  setActiveTab: (activeTab) => set({ activeTab, detailTaskId: null, detailIssue: null }),
  selectProject: (selectedProjectId) => set({ selectedProjectId }),
  setQuickAdd: (quickAdd) => set({ quickAdd }),
  setSignals: (source, list) =>
    set((s) => {
      const signalSources = { ...s.signalSources, [source]: list };
      return { signalSources, signals: mergeSignals(signalSources) };
    }),
  hold: (reason, on) =>
    set((s) => {
      if (s.holds.has(reason) === on) return s;
      const holds = new Set(s.holds);
      if (on) holds.add(reason);
      else holds.delete(reason);
      return { holds };
    }),
  showToast: (toast) => set({ toast: { ...toast, id: ++toastSeq, at: Date.now() } }),
  dismissToast: (id) => set((s) => (s.toast?.id === id ? { toast: null } : s)),
}));

/**
 * Collapsed shape: anything needing attention, or a plugin's live item,
 * earns the slightly larger peek state; otherwise the notch stays at its
 * idle size.
 *
 * @param signals - Current attention signals.
 * @param live - Whether a live item is showing.
 */
export function restingShape(signals: readonly Signal[], live = false): ShapeMode {
  return signals.length > 0 || live ? "peek" : "idle";
}
