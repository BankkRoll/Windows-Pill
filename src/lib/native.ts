/**
 * Typed bridge to the Rust core.
 *
 * Every IPC call and event name lives here so components never touch raw
 * `invoke`/`listen` strings. When the UI runs in a plain browser (`pnpm dev`
 * without Tauri), calls become no-ops so the pill can still be styled.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { HitRect } from "./shapes";

/** True when running inside the Tauri webview. */
export const isTauri: boolean = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** View a detached window should show. Mirrors Rust `detached::Nav`. */
export interface DetachedNav {
  tab: string | null;
  projectId: string | null;
  /** Run to select in the Agents tab, e.g. from a notification. */
  runId?: string | null;
  /** Task whose page to open when there is no run. */
  taskId?: string | null;
}

/**
 * Set by the core's initialization script when this webview is the detached
 * window rather than the overlay; `null` in the overlay.
 */
export const detachedNav: DetachedNav | null =
  typeof window !== "undefined" ? ((window as { __PILL_DETACHED__?: DetachedNav }).__PILL_DETACHED__ ?? null) : null;

/** True in the detached window, which is a normal activating app window. */
export const isDetached = detachedNav !== null;

/** Limits for headless Claude Code runs. Mirrors Rust `AgentLimits`. */
export interface AgentLimits {
  maxParallelRuns: number;
  maxTurns: number;
  timeoutMinutes: number;
  /** Follow-up turns a run gets to fix failed tests or lint before it fails. */
  fixAttempts: number;
}

/** When hands-off tasks start automatically. Mirrors Rust `Autopilot`. */
export interface Autopilot {
  enabled: boolean;
  /** Local `HH:MM`; with `windowEnd`, limits automatic runs to a window. */
  windowStart: string | null;
  windowEnd: string | null;
}

/** Global settings. Mirrors Rust `config::Config`. */
export interface PillConfig {
  version: number;
  /** Folders or whole drives scanned for git repos. */
  roots: string[];
  /** Project folders added by hand. */
  projects: string[];
  hotkey: string;
  /** Monitor index; `null` follows the primary monitor. */
  monitor: number | null;
  launchAtStartup: boolean;
  /** Accent color as `#RRGGBB`. */
  accent: string;
  hoverOpenDelayMs: number;
  hoverCloseDelayMs: number;
  agent: AgentLimits;
  autopilot: Autopilot;
  /** Default model and effort per provider. */
  models: ModelDefaults;
  /** Surface style for the notch and the detached window. */
  theme: Theme;
  /** Light or dark palette, applied to every surface style. */
  appearance: Appearance;
  glassTint: GlassTint;
  openOn: OpenOn;
  /** Keep the notch above other windows. */
  onTop: boolean;
  glassSource: GlassSource;
  /** Pill gradient id, used by the `pill` source and as `auto`'s fallback. */
  glassGradient: string;
  /** Picture used by the `image` source; kept while another source is picked. */
  glassImage: string | null;
  /** Plugin toggles and approvals. */
  plugins: PluginSettings;
}

/** Which plugins run, and how they're arranged. Mirrors Rust `config::PluginSettings`. */
export interface PluginSettings {
  /** Built-in id to on/off; missing ids use the plugin's default. */
  builtin: Record<string, boolean>;
  /** External id to the exact command line the user approved. */
  external: Record<string, string>;
  /** External ids in the user's order. The core already sorts plugin states by it. */
  order: string[];
  /** Tab plugins whose tab only opens from the header's overflow menu. */
  menuTabs: string[];
}

/** Surface styles. `glass` is Mica on the detached window and matching wallpaper glass on the notch. */
export type Theme = "black" | "graphite" | "glass";

/** Light or dark palette; `system` follows the Windows app mode. Mirrors Rust `config::Appearance`. */
export type Appearance = "dark" | "light" | "system";

/** Tiny RGB sample of a whole monitor's wallpaper. Mirrors Rust `backdrop::Backdrop`. */
export interface Backdrop {
  width: number;
  height: number;
  /** Row-major RGB triples. */
  pixels: number[];
  /** Monitor `[x, y, width, height]` in physical pixels. */
  monitor: [number, number, number, number];
  /** Monitor scale factor. */
  scale: number;
  /** The picked image this was made from, or `null` for the live desktop. */
  image: string | null;
}

/**
 * What the glass theme blurs: the wallpaper (`auto` falls back to a Pill
 * gradient when it's too dark or flat), a Pill gradient, or a picture.
 */
export type GlassSource = "auto" | "wallpaper" | "pill" | "image";

/** A wallpaper that ships with Windows, read from `%WINDIR%\Web`. */
export interface StockWallpaper {
  path: string;
  /** Folder and file name, for tooltips. */
  name: string;
}

/** What opens the panel from the notch. */
export type OpenOn = "hover" | "click";

/** Strength of the glass tint; lighter shows more wallpaper. */
export type GlassTint = "light" | "medium" | "dark";

/** A model and effort; empty strings defer to the next level (chat → project → global → CLI). */
export interface ModelChoice {
  model: string;
  effort: string;
}

/** Provider, model and effort for one chat or run; `provider: null` uses the project's. */
export interface RunChoice extends ModelChoice {
  provider: AiProvider | null;
}

/** Lines and files changed against the base branch. */
export interface DiffStat {
  files: number;
  insertions: number;
  deletions: number;
}

/** Global model defaults. Mirrors Rust `models::ModelDefaults`. */
export interface ModelDefaults {
  claude: ModelChoice;
  codex: ModelChoice;
}

/** One selectable model. Mirrors Rust `models::ModelInfo`. */
export interface ModelInfo {
  id: string;
  label: string;
  description: string;
  /** Accepted efforts, lowest first. */
  efforts: string[];
  defaultEffort: string | null;
}

/** Models per provider, as each installed CLI reports them. */
export interface ModelCatalog {
  claude: ModelInfo[];
  /** What Claude Code's "Default" currently resolves to, as it describes it. */
  claudeDefault: string | null;
  claudeError: string | null;
  codex: ModelInfo[];
  codexError: string | null;
}

/** Board column. */
export type TaskStatus = "inbox" | "todo" | "doing" | "review" | "done";

/** Board columns in display order. */
export const STATUSES: readonly TaskStatus[] = ["inbox", "todo", "doing", "review", "done"];

/** Links between a task and its latest agent run. */
export interface AgentLink {
  lastRunId: string | null;
  branch: string | null;
  prUrl: string | null;
  /** Why the latest run stopped short; autopilot leaves the task alone while set. */
  blocked: string | null;
}

/** The GitHub issue a task was made from. */
export interface IssueLink {
  number: number;
  /** Web page of the issue. */
  url: string;
  /** Set by the GitHub sync once the issue is closed on GitHub. */
  closed: boolean;
}

/** A task as stored in `tasks.json`. */
export interface Task {
  id: string;
  title: string;
  notes: string;
  status: TaskStatus;
  /** 1 (high) to 3 (low). */
  priority: 1 | 2 | 3 | null;
  tags: string[];
  /** Local date `YYYY-MM-DD`. */
  due: string | null;
  order: number;
  /**
   * Hands-off: autopilot starts an agent on it and the agent owns it from
   * there. Named `agentReady` on disk for compatibility.
   */
  agentReady: boolean;
  pinnedNotes: string[];
  createdAt: string;
  updatedAt: string;
  agent: AgentLink;
  /** Set when the task was made from a GitHub issue. */
  issue: IssueLink | null;
  /**
   * Who wrote the task's text when it wasn't the user. Agents get such a
   * title fenced as untrusted and run with a tighter tool allowlist.
   */
  origin?: "issue" | "plugin" | "unknown";
}

/** Editor launched by "Open in editor". */
export type EditorChoice = "auto" | "code" | "cursor" | "explorer";

/** AI coding CLI that does the work. */
export type AiProvider = "claude" | "codex";

/**
 * What a finished agent run does with its branch: open a draft PR, push the
 * branch only, or keep the commits on the local branch.
 */
export type ShipMode = "pr" | "push" | "local";

/** Per-project settings from `.pill/project.json`. Empty strings mean "default". */
export interface ProjectSettings {
  name: string;
  color: string;
  baseBranch: string;
  testCommand: string;
  lintCommand: string;
  editor: EditorChoice;
  agentRules: string;
  commitTasks: boolean;
  planMode: boolean;
  /** AI CLI used for this project's agents and chats. */
  provider: AiProvider;
  /** Model override; empty uses the global default. */
  model: string;
  /** Effort override; empty uses the global default. */
  effort: string;
  /** What a finished run does with its branch. */
  ship: ShipMode;
  /** Start of agent branch names; empty uses `agent/`. */
  branchPrefix: string;
  /** Show open GitHub issues assigned to the signed-in gh user on the board. */
  githubIssues: boolean;
  /** The user said not now to setting up a test command; Pill stops asking. */
  skipTestSetup: boolean;
}

/** A discovered project with resolved display values. */
export interface Project {
  id: string;
  path: string;
  name: string;
  color: string;
  baseBranch: string;
  settings: ProjectSettings;
  tasks: Task[];
  /** Contents of `.pill/notes.md`. */
  notes: string;
  hasRemote: boolean;
  /** Remote agents fetch from and push to: `origin`, else the first one; `null` when local only. */
  remote: string | null;
  /** Unix seconds of the latest commit or task change. */
  lastActive: number;
  errors: string[];
  manual: boolean;
}

/** Every project and the Inbox, as sent by the core on each change. */
export interface Snapshot {
  /** Most recently active first. */
  projects: Project[];
  inbox: Task[];
  inboxError: string | null;
}

/** Fields for a new task. */
export interface NewTask {
  title: string;
  notes?: string;
  status?: TaskStatus;
  priority?: 1 | 2 | 3 | null;
  tags?: string[];
  due?: string | null;
  agentReady?: boolean;
}

/** Partial update; `null` clears a nullable field. */
export type TaskPatch = Partial<
  Pick<Task, "title" | "notes" | "status" | "priority" | "tags" | "due" | "order" | "agentReady" | "pinnedNotes">
>;

/** Lifecycle of an agent run. */
export type RunState =
  | "queued"
  | "preparing"
  | "running"
  | "awaiting_approval"
  | "verifying"
  | "shipping"
  | "failed"
  | "ready"
  | "pr_open"
  | "pushed"
  | "committed"
  | "stopped"
  | "interrupted"
  /** Finished without changing code; the reply is in {@link RunRecord.answer}. */
  | "answered";

/** States of a run that finished and shipped its work, by any {@link ShipMode}. */
export const SHIPPED_RUN_STATES: readonly RunState[] = ["pr_open", "pushed", "committed"];

/** States in which a worker owns the run. */
export const ACTIVE_RUN_STATES: readonly RunState[] = ["queued", "preparing", "running", "verifying", "shipping"];

/** An agent run, mirroring `run.json`. */
export interface RunRecord {
  id: string;
  taskId: string;
  projectId: string;
  taskTitle: string;
  state: RunState;
  branch: string;
  worktree: string;
  claudeSessionId: string | null;
  startedAt: string;
  endedAt: string | null;
  tests: { passed: boolean | null; summary: string | null };
  prUrl: string | null;
  error: string | null;
  planMode: boolean;
  provider: AiProvider;
  plan: string | null;
  /** The agent's final reply when it finished without changing code. */
  answer: string | null;
  /** Test command Pill found and ran because the project has none set. */
  testsDetected: string | null;
  /** When the latest attempt (retry, approved plan) began; its duration counts from here. */
  resumedAt: string | null;
  /** Whether the run's worktree is still on disk. */
  worktreePresent: boolean;
  filesChanged: string[];
  activity: string | null;
  turns: number | null;
  costUsd: number | null;
  /** Model the CLI reported or was given. */
  model: string | null;
  effort: string | null;
  /** Model and effort picked at start; empty inherits. */
  choice: ModelChoice;
  /** Shared by runs started together to compare setups on one task. */
  compareGroup: string | null;
  diff: DiffStat | null;
  /** Started on a hands-off task; the agent owns the task. */
  handsOff: boolean;
  /** Automatic fix turns used after failed checks. */
  fixAttempts: number;
  /** Ref the branch started from, e.g. `origin/main`; absent on older runs. */
  baseRef?: string | null;
}

/** One line in a run's activity log. */
export type RunEvent =
  | { kind: "step"; text: string }
  | { kind: "text"; text: string }
  | { kind: "tool"; name: string; detail: string }
  | { kind: "output"; text: string }
  | { kind: "result"; ok: boolean; text: string }
  /** What the user told the agent mid-run: retry feedback, plan approval. */
  | { kind: "user"; text: string };

/** A {@link RunEvent} tagged with its run and time. */
export type LogLine = RunEvent & { runId: string; at: string };

/** A command found in a project's files, and the file it came from. */
export interface FoundCommand {
  command: string;
  /** e.g. `package.json`. */
  source: string;
}

/** Test and lint commands found in a project's files. */
export interface DetectedChecks {
  test: FoundCommand | null;
  lint: FoundCommand | null;
}

/** One preflight check. */
export interface Check {
  name: string;
  ok: boolean;
  message: string;
}

/** A project's remote, described without credentials. */
export interface RemoteInfo {
  /** Remote name, usually `origin`. */
  name: string;
  /** Short form such as `github.com/owner/repo`, or a path for a local remote. */
  display: string;
  /** Repository web page, when the host is known to serve one. */
  webUrl: string | null;
  /** Whether the host looks like GitHub, where `gh` can open PRs. */
  github: boolean;
}

/** Read-only repository facts for a project's settings page. */
export interface RepoInfo {
  /** False for a plain folder; every other field is then empty. */
  isRepo: boolean;
  /** Checked-out branch; `null` when HEAD is detached. */
  branch: string | null;
  /** Upstream of the checked-out branch, e.g. `origin/main`. */
  upstream: string | null;
  ahead: number;
  behind: number;
  /** Changed, staged and untracked files in the working folder. */
  changes: number;
  /** The repository has no commits yet. */
  unborn: boolean;
  /** What an empty base branch setting resolves to, if anything. */
  defaultBranch: string | null;
  remote: RemoteInfo | null;
  /** Local branches, most recently committed first. */
  localBranches: string[];
  /** Branches on the remote without the `origin/` part, most recent first. */
  remoteBranches: string[];
  /** GitHub CLI status; only checked when there is a remote. */
  gh: Check | null;
}

/** An issue label. */
export interface GhLabel {
  name: string;
  /** Hex color without `#`, as GitHub stores it. */
  color: string;
}

/** An open issue assigned to the user. Mirrors Rust `github::Issue`. */
export interface GhIssue {
  number: number;
  title: string;
  url: string;
  /** Markdown written by the issue's author. Untrusted: render it sanitized only. */
  body: string;
  labels: GhLabel[];
  updatedAt: string;
}

/** Lifecycle of a pull request. */
export type PrState = "open" | "merged" | "closed";

/** A PR's review decision. `none` means no review rule applies and nobody reviewed. */
export type PrReview = "approved" | "changes_requested" | "review_required" | "none";

/** Combined CI result of a PR's head commit; `none` when it has no checks. */
export type PrChecks = "passing" | "failing" | "pending" | "none";

/** Review and CI state of one pull request. Mirrors Rust `github::PrStatus`. */
export interface PrStatus {
  number: number;
  url: string;
  state: PrState;
  draft: boolean;
  review: PrReview;
  checks: PrChecks;
  /** Checks that succeeded; skipped and neutral ones count as passed. */
  passed: number;
  failed: number;
  pending: number;
}

/** Where a project's GitHub sync stands. */
export type SyncKind = "waiting" | "ok" | "not_github" | "no_gh" | "signed_out" | "rate_limited" | "error";

/** A project's GitHub sync status. */
export interface SyncStatus {
  kind: SyncKind;
  /** Short sentence; empty when the kind says it all. */
  message: string;
  /** When the last successful sync finished. */
  syncedAt: string | null;
  /** When a failed or paused sync is tried again. */
  retryAt: string | null;
}

/** Everything known about one project on GitHub. Mirrors Rust `github::ProjectGithub`. */
export interface ProjectGithub {
  /** `owner/repo`, when the remote is on GitHub. */
  repo: string | null;
  status: SyncStatus;
  /** True while a sync for this project is running. */
  syncing: boolean;
  /** Open issues assigned to the user; empty when the project hides them. */
  issues: GhIssue[];
  /** Status of every polled PR, keyed by its URL. */
  prs: Record<string, PrStatus>;
}

/** Data the UI loads once at startup. */
export interface Bootstrap {
  config: PillConfig;
  /** Set when `config.json` exists but couldn't be parsed. */
  configError: string | null;
  configPath: string;
  /** Set when the global hotkey couldn't be registered. */
  hotkeyError: string | null;
  snapshot: Snapshot;
  /** Whether Windows asks apps for light mode, for the `system` appearance. */
  systemLight: boolean;
}

/** Defaults used when running outside Tauri. Kept in sync with Rust defaults. */
export const DEFAULT_CONFIG: PillConfig = {
  version: 1,
  roots: [],
  projects: [],
  hotkey: "Ctrl+Shift+Space",
  monitor: null,
  launchAtStartup: false,
  accent: "#2F7BFF",
  hoverOpenDelayMs: 120,
  hoverCloseDelayMs: 350,
  agent: { maxParallelRuns: 2, maxTurns: 80, timeoutMinutes: 45, fixAttempts: 1 },
  autopilot: { enabled: true, windowStart: null, windowEnd: null },
  models: { claude: { model: "", effort: "" }, codex: { model: "", effort: "" } },
  theme: "glass",
  appearance: "dark",
  glassTint: "medium",
  openOn: "hover",
  onTop: true,
  glassSource: "auto",
  glassGradient: "blue",
  glassImage: null,
  plugins: { builtin: {}, external: {}, order: [], menuTabs: [] },
};

/**
 * Loads config and startup state from the core.
 *
 * @returns Bootstrap data; browser builds get defaults.
 */
export async function bootstrap(): Promise<Bootstrap> {
  if (!isTauri) {
    return {
      config: DEFAULT_CONFIG,
      configError: null,
      configPath: "",
      hotkeyError: null,
      snapshot: { projects: [], inbox: [], inboxError: null },
      systemLight: window.matchMedia("(prefers-color-scheme: light)").matches,
    };
  }
  return invoke<Bootstrap>("bootstrap");
}

/** Where a task lives: a project id, or `null` for the Inbox. */
export type ProjectRef = string | null;

/**
 * Task and project operations. Each resolves after the change is on disk;
 * the UI then receives a fresh {@link Snapshot} through {@link EVENTS.snapshot}.
 * Errors reject with a human-readable message.
 */
export const api = {
  openDetached: (tab: string | null, projectId: string | null) => invoke<void>("open_detached", { tab, projectId }),
  toggleFill: () => invoke<boolean>("toggle_fill"),
  normalizeMaximize: () => invoke<boolean>("normalize_maximize"),
  restoreForDrag: (ratio: number) => invoke<void>("restore_for_drag", { ratio }),
  sampleBackdrop: () => invoke<Backdrop | null>("sample_backdrop"),
  getBackdrop: () => invoke<Backdrop | null>("get_backdrop"),
  stockWallpapers: () => invoke<StockWallpaper[]>("stock_wallpapers"),
  /** Small JPEG of an image file as a `data:` URL; rejects files Pill won't decode. */
  imageThumbnail: (path: string) => invoke<string>("image_thumbnail", { path }),
  modelCatalog: (refresh = false) => invoke<ModelCatalog>("model_catalog", { refresh }),
  createTask: (projectId: ProjectRef, task: NewTask) => invoke<Task>("create_task", { projectId, task }),
  updateTask: (projectId: ProjectRef, taskId: string, patch: TaskPatch) => invoke<Task>("update_task", { projectId, taskId, patch }),
  moveTask: (fromProjectId: ProjectRef, taskId: string, toProjectId: ProjectRef) =>
    invoke<Task>("move_task", { fromProjectId, taskId, toProjectId }),
  deleteTask: (projectId: ProjectRef, taskId: string) => invoke<void>("delete_task", { projectId, taskId }),
  saveProjectSettings: (projectId: string, settings: ProjectSettings) => invoke<Project>("save_project_settings", { projectId, settings }),
  setRoots: (roots: string[]) => invoke<PillConfig>("set_roots", { roots }),
  addProject: (path: string) => invoke<PillConfig>("add_project", { path }),
  removeProject: (path: string) => invoke<PillConfig>("remove_project", { path }),
  rescan: () => invoke<void>("rescan"),
  listDrives: () => (isTauri ? invoke<string[]>("list_drives") : Promise.resolve([])),
  setHotkey: (hotkey: string) => invoke<PillConfig>("set_hotkey", { hotkey }),
  updateSettings: (patch: SettingsPatch) => invoke<PillConfig>("update_settings", { patch }),
  listMonitors: () => (isTauri ? invoke<MonitorInfo[]>("list_monitors") : Promise.resolve([])),
  cliStatus: () => (isTauri ? invoke<Check[]>("cli_status") : Promise.resolve([])),
  listApprovals: () => (isTauri ? invoke<Approval[]>("list_approvals") : Promise.resolve([])),
  answerApproval: (id: string, answer: ApprovalAnswer) => invoke<void>("answer_approval", { id, answer }),
  setViewing: (viewing: Viewing | null) => (isTauri ? invoke<void>("set_viewing", { viewing }) : Promise.resolve()),
  listRuns: () => (isTauri ? invoke<RunRecord[]>("list_runs") : Promise.resolve([])),
  runLog: (runId: string) => invoke<LogLine[]>("run_log", { runId }),
  startAgent: (projectId: string, taskId: string, plan?: boolean, choice?: RunChoice) =>
    invoke<RunRecord>("start_agent", { projectId, taskId, plan, choice }),
  compareAgents: (projectId: string, taskId: string, choices: RunChoice[]) =>
    invoke<RunRecord[]>("compare_agents", { projectId, taskId, choices }),
  pickRun: (runId: string) => invoke<void>("pick_run", { runId }),
  stopAgent: (runId: string) => invoke<void>("stop_agent", { runId }),
  retryAgent: (runId: string, feedback: string) => invoke<void>("retry_agent", { runId, feedback }),
  approvePlan: (runId: string, plan: string) => invoke<void>("approve_plan", { runId, plan }),
  cleanupRun: (runId: string) => invoke<void>("cleanup_run", { runId }),
  /** Deletes a finished run's worktree, unshipped branch and history. The task stays. */
  deleteRun: (runId: string) => invoke<void>("delete_run", { runId }),
  /** Deletes every failed, stopped or interrupted run (in one project, or all); returns how many. */
  clearFailedRuns: (projectId: string | null) => invoke<number>("clear_failed_runs", { projectId }),
  /** The prompt the run's agent was given, follow-ups included. */
  runPrompt: (runId: string) => invoke<string | null>("run_prompt", { runId }),
  /** Test and lint commands found in the project's files (package.json, Cargo.toml…). */
  detectChecks: (projectId: string) => invoke<DetectedChecks>("detect_checks", { projectId }),
  preflight: (projectId: string) => invoke<Check[]>("preflight", { projectId }),
  projectGitInfo: (projectId: string) => invoke<RepoInfo>("project_git_info", { projectId }),
  openWorktree: (runId: string) => invoke<string>("open_worktree", { runId }),
  saveNotes: (projectId: string, text: string, base: string) => invoke<void>("save_notes", { projectId, text, base }),
  openInEditor: (projectId: string) => invoke<string>("open_in_editor", { projectId }),
  /** Opens the project's folder in Explorer. */
  openProjectFolder: (projectId: string) => invoke<void>("open_project_folder", { projectId }),
  detectEditors: () =>
    isTauri ? invoke<{ cursor: boolean; code: boolean }>("detect_editors") : Promise.resolve({ cursor: false, code: false }),
  renderTaskPrompt: (projectId: string, taskId: string) => invoke<string>("render_task_prompt", { projectId, taskId }),
  githubSnapshot: () =>
    isTauri ? invoke<Record<string, ProjectGithub>>("github_snapshot") : Promise.resolve({} as Record<string, ProjectGithub>),
  /** Syncs one project now, or (with `null`) every project that hasn't synced in the last minute. */
  githubRefresh: (projectId: string | null) => (isTauri ? invoke<void>("github_refresh", { projectId }) : Promise.resolve()),
  /** Adds an assigned issue to the board, or returns the task already linked to it. */
  trackIssue: (projectId: string, number: number, status?: TaskStatus) => invoke<Task>("track_issue", { projectId, number, status }),
  /** Turns an issue into a task (or finds its task) and starts an agent on it. */
  startIssue: (projectId: string, number: number, plan?: boolean, choice?: RunChoice) =>
    invoke<RunRecord>("start_issue", { projectId, number, plan, choice }),
};

/**
 * Normalizes anything thrown by `invoke` into a message for display.
 *
 * @param e - Rejection value.
 */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

/**
 * Tells the core where the visible shape is, so only that area takes clicks.
 *
 * @param rect - Shape rectangle, or `null` to make the window fully click-through.
 */
export function setHitRect(rect: HitRect | null): void {
  if (!isTauri) return;
  void invoke("set_hit_rect", { rect });
}

/** Lets the pill take keyboard focus (for inputs), remembering the previous window. */
export function focusWindow(): Promise<void> {
  // The detached window activates on its own; this only applies to the overlay.
  return isTauri && !isDetached ? invoke("focus_window") : Promise.resolve();
}

/** Returns focus to whichever window had it before {@link focusWindow}. */
export function releaseFocus(): Promise<void> {
  return isTauri && !isDetached ? invoke("release_focus") : Promise.resolve();
}

/**
 * Subscribes to a core event, handling the async `listen` registration so the
 * returned cleanup is safe to call immediately (e.g. in a React effect).
 *
 * @param event - Event name.
 * @param handler - Called with each payload.
 * @returns A synchronous unsubscribe function.
 */
export function subscribe<T>(event: string, handler: (payload: T) => void): () => void {
  if (!isTauri) return () => {};
  let disposed = false;
  let unlisten: (() => void) | undefined;
  void listen<T>(event, (e) => handler(e.payload)).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  });
  return () => {
    disposed = true;
    unlisten?.();
  };
}

/** A tool use waiting for the user. Mirrors Rust `approvals::Approval`. */
export interface Approval {
  id: string;
  projectId: string;
  chatId: string | null;
  runId: string | null;
  taskId: string | null;
  /** Chat or task title. */
  origin: string;
  provider: AiProvider;
  tool: string;
  summary: string;
  detail: string | null;
  /** Rule "Always allow" saves, e.g. `Bash(npm install *)`. */
  rule: string | null;
  /** Whether "for this chat/run" is on offer. */
  session: boolean;
  createdAt: string;
}

/**
 * What a Pill surface is showing, reported so the core skips notifications
 * about it. Mirrors Rust `notify::Viewing`.
 */
export interface Viewing {
  /** Run open in the Agents tab. */
  runId: string | null;
  /** Task open in the task page, or the task of the run being shown. */
  taskId: string | null;
}

/** An answer to an {@link Approval}. */
export type ApprovalAnswer =
  { kind: "once" } | { kind: "session" } | { kind: "always"; shared: boolean } | { kind: "deny"; message: string | null };

/** Event names emitted by the core, with the payload each carries. */
export const EVENTS = {
  /** {@link Approval}[]: tool uses waiting for the user. */
  approvals: "pill://approvals",
  /** `boolean`: cursor entered/left the visible shape. */
  hover: "pill://hover",
  /** {@link Snapshot}: projects or tasks changed. */
  snapshot: "pill://snapshot",
  /** {@link PillConfig}: settings changed. */
  config: "pill://config",
  /** {@link RunRecord}[]: any run changed. */
  runs: "pill://runs",
  /** {@link LogLine}: a run logged activity. */
  runLog: "pill://run-log",
  /** {@link ChatUpdate}: a chat reply progressed or finished. */
  chat: "pill://chat",
  /** No payload: the hotkey or tray asked for quick-add. */
  quickAdd: "pill://quick-add",
  /** {@link DetachedNav}: the open detached window should switch view. */
  detachedNav: "pill://detached-nav",
  /** `boolean`: another window is being dragged, so the notch steps aside for Snap. */
  stepAside: "pill://step-aside",
  /** `{ activity }`: what the command bar's AI is doing. */
  assistant: "pill://assistant",
  /** {@link Backdrop} | null: the wallpaper behind the notch changed. */
  backdrop: "pill://backdrop",
  /** `Record<projectId, ProjectGithub>`: issues, PR status or sync status changed. */
  github: "pill://github",
  /** {@link PluginState}[]: a plugin's output or status changed. */
  plugins: "pill://plugins",
  /** {@link PageEvent}: a plugin sent a page for its tab. */
  pluginPage: "pill://plugin-page",
  /** {@link NotificationFeed} | null: Action Center changed, or the notifications plugin stopped. */
  notifications: "pill://notifications",
  /** {@link VoiceUpdate}: dictation progress. */
  voice: "pill://voice",
  /** `boolean`: Windows switched apps to light (`true`) or dark mode. */
  systemAppearance: "pill://system-appearance",
} as const;

/** Global settings the Settings tab can change in one call. */
export interface SettingsPatch {
  monitor?: number | null;
  launchAtStartup?: boolean;
  accent?: string;
  hoverOpenDelayMs?: number;
  hoverCloseDelayMs?: number;
  agent?: AgentLimits;
  autopilot?: Autopilot;
  models?: ModelDefaults;
  theme?: Theme;
  appearance?: Appearance;
  glassTint?: GlassTint;
  openOn?: OpenOn;
  onTop?: boolean;
  glassSource?: GlassSource;
  glassGradient?: string;
  /** JPEG or PNG path; the core rejects missing files and ones over 40 MB. */
  glassImage?: string | null;
}

/** A connected display. */
export interface MonitorInfo {
  index: number;
  name: string;
  width: number;
  height: number;
  scale: number;
  primary: boolean;
}

/** What Claude may do in a chat. */
export type ChatMode = "read" | "edit";

/** A task Claude proposed in a chat. */
export interface ProposedTask {
  title: string;
  notes: string;
  priority: 1 | 2 | 3 | null;
}

/** One chat message. */
export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  text: string;
  at: string;
  tools: string[];
  proposals: ProposedTask[];
  error: string | null;
  /** Model that wrote this reply. */
  model: string | null;
  /** CLI that wrote this reply. */
  provider: AiProvider | null;
}

/** A project conversation with Claude Code. */
export interface Chat {
  id: string;
  projectId: string;
  title: string;
  mode: ChatMode;
  provider: AiProvider;
  sessionId: string | null;
  worktree: string | null;
  branch: string | null;
  prUrl: string | null;
  /** Provider, model and effort picked for this chat. */
  choice: RunChoice;
  createdAt: string;
  updatedAt: string;
  messages: ChatMessage[];
  busy: boolean;
}

/** A reply streaming in. */
export interface ChatUpdate {
  chatId: string;
  message: ChatMessage;
  busy: boolean;
}

/** Chat operations. */
export const chatApi = {
  list: (projectId: string) => invoke<Chat[]>("list_chats", { projectId }),
  create: (projectId: string, mode: ChatMode) => invoke<Chat>("create_chat", { projectId, mode }),
  send: (projectId: string, chatId: string, text: string, choice: RunChoice | null) =>
    invoke<Chat>("send_chat", { projectId, chatId, text, choice }),
  setMode: (projectId: string, chatId: string, mode: ChatMode) => invoke<Chat>("set_chat_mode", { projectId, chatId, mode }),
  stop: (chatId: string) => invoke<void>("stop_chat", { chatId }),
  remove: (projectId: string, chatId: string) => invoke<void>("delete_chat", { projectId, chatId }),
  ship: (projectId: string, chatId: string) => invoke<string>("ship_chat", { projectId, chatId }),
};

// -----------------------------------------------------------------------------
// Plugins
// -----------------------------------------------------------------------------

/** Icon names plugins may use. Mirrors Rust `plugins::protocol::Icon`. */
export type PluginIcon =
  | "alert"
  | "bell"
  | "bot"
  | "branch"
  | "bug"
  | "calendar"
  | "chat"
  | "check"
  | "clock"
  | "cloud"
  | "code"
  | "download"
  | "error"
  | "flash"
  | "globe"
  | "heart"
  | "info"
  | "mail"
  | "music"
  | "pause"
  | "person"
  | "play"
  | "rocket"
  | "shield"
  | "star"
  | "timer"
  | "warning"
  | "wrench";

/** Colors plugins may use. Mirrors Rust `plugins::protocol::Tone`. */
export type PluginTone = "neutral" | "accent" | "green" | "amber" | "red" | "purple";

/** What a plugin may show (`signals` … `tab`) or receive (`projects`). */
export type PluginCapability = "signals" | "live" | "widget" | "feed" | "tab" | "projects";

/** How urgent a feed item is. Mirrors Rust `plugins::protocol::Severity`. */
export type FeedSeverity = "critical" | "high" | "medium" | "low" | "info";

/** An entry in Pill's notification feed. Mirrors Rust `plugins::protocol::FeedItem`. */
export interface FeedItem {
  /** Unique within the plugin; dismissing it hides this id for good. */
  id: string;
  severity: FeedSeverity;
  icon: PluginIcon | null;
  title: string;
  body: string;
  /** One short line of facts, e.g. `lodash 4.17.20 → 4.17.21`. */
  detail: string | null;
  /** An `https://` link, checked by the core. */
  url: string | null;
  /** Project the item is about. */
  projectId: string | null;
  /** Set when the item suggests a task; "Fix with an agent" creates it. */
  task: { title: string } | null;
  /** Plugin-defined buttons. */
  actions: WidgetAction[];
}

/** What "Fix with an agent" made. Mirrors Rust `commands::FeedTaskResult`. */
export interface FeedTaskResult {
  projectId: string;
  task: Task;
  /** False when an open task with the same title already existed. */
  created: boolean;
}

/** A plugin's entry in the status cluster. */
export interface PluginSignal {
  id: string;
  /** Set by built-ins that feed one of Pill's own kinds instead of drawing their own icon. */
  kind: "notification" | null;
  icon: PluginIcon;
  tone: PluginTone;
  count: number;
  /** Tooltip. */
  label: string;
}

/** What a plugin shows inside the collapsed notch. */
export type LiveItem =
  | { kind: "text"; icon: PluginIcon | null; text: string; tone: PluginTone }
  | {
      kind: "media";
      /** Small `data:image/jpeg` URL, or `null` when the player has none. */
      artwork: string | null;
      playing: boolean;
      title: string;
    };

/** A row on a card widget. */
export interface WidgetRow {
  icon: PluginIcon | null;
  text: string;
  detail: string | null;
  tone: PluginTone;
}

/** A button on a card widget. */
export interface WidgetAction {
  id: string;
  label: string;
  icon: PluginIcon | null;
}

/** The now playing card. */
export interface MediaWidget {
  /** Player name, e.g. "Spotify". */
  app: string;
  title: string;
  artist: string;
  album: string;
  artwork: string | null;
  playing: boolean;
  canPlayPause: boolean;
  canNext: boolean;
  canPrevious: boolean;
}

/** A card on the Today tab. */
export type PluginWidget = { kind: "card"; title: string; rows: WidgetRow[]; actions: WidgetAction[] } | ({ kind: "media" } & MediaWidget);

/** Windows Settings pages Pill can open. Mirrors Rust `system_settings::SystemPage`. */
export type SystemPage = "notifications" | "microphone" | "speech" | "sound";

/** A one-click way out of a plugin status. */
export type PluginFix =
  { kind: "settings"; page: SystemPage; label: string } | { kind: "action"; id: string; label: string } | { kind: "restart" };

/** Coarse plugin state. */
export type PluginStatusState =
  | "off"
  | "starting"
  | "running"
  | "needsPermission"
  | "unsupported"
  | "error"
  /** External plugin whose command changed since it was approved. */
  | "changed"
  /** External plugin with a broken manifest. */
  | "invalid";

/** Everything the UI knows about one plugin. Mirrors Rust `plugins::PluginState`. */
export interface PluginState {
  id: string;
  name: string;
  description: string;
  source: "builtin" | "external";
  capabilities: PluginCapability[];
  enabled: boolean;
  status: { state: PluginStatusState; message: string | null; fix: PluginFix | null };
  /** External plugins: the exact command line that runs. */
  command: string | null;
  /** External plugins: what turning it on approves (the command line, plus `+projects` and the like on their own lines). */
  approval: string | null;
  /** External plugins: the plugin's folder. */
  folder: string | null;
  signals: PluginSignal[];
  live: LiveItem | null;
  widget: PluginWidget | null;
  /** Feed items the user hasn't dismissed. */
  feed: FeedItem[];
  /** The plugin's header tab, when it has the `tab` capability. */
  tab: PluginTabSpec | null;
  /** Badges and order for the tab's project sidebar. */
  nav: NavItem[];
  /** Tab plugins: whether the tab sits in the header rather than only in its overflow menu. */
  pinned: boolean;
}

/** A plugin's tab in the header. Mirrors Rust `plugins::page::TabSpec`. */
export interface PluginTabSpec {
  title: string;
  icon: PluginIcon;
}

/** A small colored label. Mirrors Rust `plugins::page::Chip`. */
export interface PageChip {
  text: string;
  tone: PluginTone;
}

/** A project's sidebar entry as the plugin describes it. Mirrors Rust `plugins::page::NavItem`. */
export interface NavItem {
  /** `null` for the "All projects" entry. */
  project: string | null;
  badge: PageChip | null;
}

/** A button on a plugin page. Mirrors Rust `plugins::page::Button`. */
export type PageButton = {
  id: string;
  label: string;
  icon: PluginIcon | null;
  primary: boolean;
} & (
  | { kind: "plugin" }
  /** Pill adds the task to the page's project; the notes stay in the core. */
  | { kind: "task"; task: { title: string } }
  | { kind: "link"; url: string }
);

/** One row of a list block. Mirrors Rust `plugins::page::Row`. */
export interface PageRow {
  /** Sent as `item` with the row's buttons. */
  id: string | null;
  title: string;
  detail: string | null;
  /** Paragraphs shown when the row is opened. */
  body: string | null;
  severity: FeedSeverity | null;
  tone: PluginTone;
  icon: PluginIcon | null;
  chips: PageChip[];
  /** An `https://` link, checked by the core. */
  url: string | null;
  /** A project id: clicking the row shows that project's page. */
  open: string | null;
  actions: PageButton[];
}

/** One block of a plugin page. Mirrors Rust `plugins::page::Block`. */
export type PageBlock =
  | { type: "header"; title: string; subtitle: string | null; icon: PluginIcon | null; tone: PluginTone; actions: PageButton[] }
  | { type: "stats"; items: { label: string; value: string; detail: string | null; tone: PluginTone }[] }
  | { type: "section"; title: string; count: number | null; collapsed: boolean; blocks: PageBlock[] }
  | { type: "list"; rows: PageRow[] }
  | { type: "table"; columns: { label: string; align: "start" | "end" }[]; rows: string[][] }
  | { type: "text"; text: string; muted: boolean }
  | { type: "keyvalue"; items: { label: string; value: string }[] }
  | { type: "notice"; tone: PluginTone; icon: PluginIcon | null; title: string | null; text: string; actions: PageButton[] }
  | { type: "empty"; icon: PluginIcon | null; title: string; text: string | null; actions: PageButton[] };

/** A validated plugin page. Mirrors Rust `plugins::page::Page`. */
export interface PluginPage {
  blocks: PageBlock[];
}

/** The last page a plugin sent for one project. Mirrors Rust `plugins::PageState`. */
export interface PageState {
  /** `null` when the plugin answered with an error instead. */
  page: PluginPage | null;
  error: string | null;
  /** Unix milliseconds. */
  receivedAt: number;
}

/** A page update. Mirrors Rust `plugins::PageEvent`. */
export interface PageEvent {
  pluginId: string;
  /** `null` for the overview page. */
  project: string | null;
  state: PageState;
}

/** Plugin operations. */
export const pluginApi = {
  states: () => (isTauri ? invoke<PluginState[]>("plugin_states") : Promise.resolve([])),
  /** `approval` must be the {@link PluginState.approval} the user was shown when turning an external plugin on. */
  setEnabled: (id: string, enabled: boolean, approval: string | null = null) =>
    invoke<PluginState[]>("set_plugin_enabled", { id, enabled, command: approval }),
  dismissFeedItem: (pluginId: string, itemId: string) => invoke<void>("dismiss_feed_item", { pluginId, itemId }),
  /** Dismisses every plugin's feed items. */
  /** Dismisses every feed item of one plugin, or of all plugins when omitted. */
  clearFeed: (pluginId?: string) => invoke<void>("clear_feed", { pluginId: pluginId ?? null }),
  feedItemAction: (pluginId: string, itemId: string, action: string) => invoke<void>("feed_item_action", { pluginId, itemId, action }),
  /** Creates the item's suggested task in its project, or finds the open one made before. */
  feedTask: (pluginId: string, itemId: string) => invoke<FeedTaskResult>("feed_task", { pluginId, itemId }),
  /** Tells the core this window shows a plugin's page; resolves to the cached copy. */
  openView: (pluginId: string, projectId: string | null) =>
    isTauri ? invoke<PageState | null>("open_plugin_view", { pluginId, projectId }) : Promise.resolve(null),
  closeView: (pluginId: string) => (isTauri ? invoke<void>("close_plugin_view", { pluginId }) : Promise.resolve()),
  /** Sends a page button to its plugin; `itemId` names the row for a row's button. */
  pageAction: (pluginId: string, projectId: string | null, itemId: string | null, action: string) =>
    invoke<void>("plugin_page_action", { pluginId, projectId, itemId, action }),
  /** Creates the task a page button suggests in the page's project, or finds the open one made before. */
  pageTask: (pluginId: string, projectId: string, itemId: string | null, action: string) =>
    invoke<FeedTaskResult>("plugin_page_task", { pluginId, projectId, itemId, action }),
  rescan: () => invoke<PluginState[]>("rescan_plugins"),
  /** Saves the external plugins' order; ids of plugins that are gone are dropped. */
  setOrder: (ids: readonly string[]) => invoke<PluginState[]>("set_plugin_order", { ids }),
  /** Shows a plugin's tab in the header (`true`) or only in the header's overflow menu. */
  setPinned: (id: string, pinned: boolean) => invoke<PluginState[]>("set_plugin_pinned", { id, pinned }),
  action: (id: string, action: string) => invoke<void>("plugin_action", { id, action }),
  restart: (id: string) => invoke<void>("restart_plugin", { id }),
  folder: () => invoke<string>("plugins_folder"),
  /** Opens the plugins folder in Explorer, creating it if needed. */
  openFolder: () => invoke<void>("open_plugins_folder"),
  openSystemSettings: (page: SystemPage) => invoke<void>("open_system_settings", { page }),
};

/** One notification in Action Center. Mirrors Rust `plugins::notifications::Note`. */
export interface NotificationItem {
  /** Action Center's id; what dismissing names. */
  id: number;
  /** The sending app's display name. */
  app: string;
  title: string;
  body: string;
  /** Creation time, Unix ms. */
  createdMs: number;
  /** Small PNG `data:` URL of the app's icon. */
  icon: string | null;
  /** For Snipping Tool notifications, a JPEG `data:` URL of the screenshot they announce. */
  screenshot: string | null;
}

/** What the Windows notifications card lists. Mirrors Rust `plugins::notifications::Feed`. */
export interface NotificationFeed {
  /** Every notification in Action Center, including ones not listed. */
  total: number;
  /** Newest first. */
  notes: NotificationItem[];
}

/** The built-in Windows notifications plugin's own operations. */
export const notificationsApi = {
  /** `null` while the plugin is off or can't read Action Center. */
  feed: () => (isTauri ? invoke<NotificationFeed | null>("notifications_feed") : Promise.resolve(null)),
  /** Dismisses one notification, in Action Center too. */
  dismiss: (id: number) => invoke<void>("dismiss_notification", { id }),
  /** Clears every notification, in Action Center too. */
  clear: () => invoke<void>("clear_notifications"),
  /** Opens the screenshot matched to a Snipping Tool notification. */
  openScreenshot: (id: number) => invoke<void>("open_notification_screenshot", { id }),
};

// -----------------------------------------------------------------------------
// Now playing
// -----------------------------------------------------------------------------

/** Id of the built-in Now playing plugin. Mirrors Rust `plugins::media::ID`. */
export const NOW_PLAYING_ID = "now-playing";

/** A player's repeat mode. Mirrors Rust `plugins::media::Repeat`. */
export type RepeatMode = "none" | "list" | "track";

/** Playback position anchored in time. Mirrors Rust `plugins::media::Timeline`. */
export interface MediaTimeline {
  /** Position at `updatedAt`. */
  positionMs: number;
  durationMs: number;
  /** Unix ms at which `positionMs` was true. */
  updatedAt: number;
  /** Playback speed; 1 is normal. */
  rate: number;
}

/** One player in the session switcher. Mirrors Rust `plugins::media::MediaSession`. */
export interface MediaSession {
  id: number;
  app: string;
  /** Small `data:image/png` app icon, when Windows has one. */
  icon: string | null;
  title: string;
  /** Artist, or the channel for a browser tab; empty when unknown. */
  artist: string;
  playing: boolean;
}

/** What the Now playing card needs beyond its widget. Mirrors Rust `plugins::media::NowPlaying`. */
export interface NowPlaying {
  /** Session the card shows and controls. */
  session: number;
  /** Every playing or paused session. */
  sessions: MediaSession[];
  /** Artwork width over height; 1 when there's none. */
  artworkAspect: number;
  /** `null` hides the seek bar (no timeline, or the player doesn't allow seeking). */
  timeline: MediaTimeline | null;
  /** `null` when the player can't shuffle. */
  shuffle: boolean | null;
  /** `null` when the player can't repeat. */
  repeat: RepeatMode | null;
  /** The player's own mixer volume; `null` while it has no audio session. */
  volume: AppVolume | null;
}

/** An app's volume in the Windows mixer. Mirrors Rust `plugins::volume::AppVolume`. */
export interface AppVolume {
  /** 0–1: the app's own slider, not the system volume. */
  level: number;
  muted: boolean;
}

/** Now playing's controls beyond the widget's buttons. Every call goes through the plugin's action channel. */
export const mediaApi = {
  /** Event carrying {@link NowPlaying} or `null`. */
  event: "pill://media",
  state: () => (isTauri ? invoke<NowPlaying | null>("media_state") : Promise.resolve(null)),
  seek: (ms: number) => pluginApi.action(NOW_PLAYING_ID, `seek:${Math.max(0, Math.round(ms))}`),
  toggleShuffle: () => pluginApi.action(NOW_PLAYING_ID, "shuffle"),
  cycleRepeat: () => pluginApi.action(NOW_PLAYING_ID, "repeat"),
  pickSession: (id: number) => pluginApi.action(NOW_PLAYING_ID, `session:${id}`),
  /** Sets the player's mixer volume (0–1) and unmutes it. */
  setVolume: (level: number) => pluginApi.action(NOW_PLAYING_ID, `volume:${Math.round(Math.min(1, Math.max(0, level)) * 100)}`),
  toggleMute: () => pluginApi.action(NOW_PLAYING_ID, "mute"),
};

// -----------------------------------------------------------------------------
// Voice
// -----------------------------------------------------------------------------

/** Dictation progress. Mirrors Rust `voice::VoiceUpdate`. */
export type VoiceUpdate = { state: "listening" } | { state: "hearing"; text: string };

/** Why dictation ended without text. Mirrors Rust `voice::VoiceError`. */
export interface VoiceError {
  code: "busy" | "micDenied" | "noMic" | "noRecognizer" | "nothingHeard" | "cancelled" | "failed";
  message: string;
  /** Windows Settings page that fixes it. */
  fix: SystemPage | null;
}

/** Offline dictation for the command bar. */
export const voiceApi = {
  /** Resolves with the transcript; rejects with a {@link VoiceError}. */
  listen: () => invoke<string>("voice_listen"),
  /** Ends dictation; `commit` keeps the text heard so far. */
  stop: (commit: boolean) => invoke<void>("voice_stop", { commit }),
};
