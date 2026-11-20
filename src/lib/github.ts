/**
 * Pure helpers over the GitHub sync data: which issues still need a card,
 * how a PR's review and CI read in a few words, and the sync status line.
 */

import { formatTime } from "./format";
import type { GhIssue, Project, ProjectGithub, PrStatus, SyncStatus } from "./native";

/** An assigned issue shown on the board, with the project it belongs to. */
export interface IssueItem {
  project: Project;
  issue: GhIssue;
}

/**
 * Issues assigned to the user that no task in the project is linked to yet.
 * An issue with a task shows as that task instead, never twice.
 *
 * @param project - Project whose tasks may link issues.
 * @param github - That project's sync data, if any.
 */
export function untrackedIssues(project: Project, github: ProjectGithub | undefined): GhIssue[] {
  if (!github || !project.settings.githubIssues) return [];
  const linked = new Set(project.tasks.flatMap((t) => (t.issue ? [t.issue.number] : [])));
  return github.issues.filter((i) => !linked.has(i.number));
}

/**
 * Issue cards for the board: one project, or every project when
 * `selectedId` is `null`. Most recently updated first.
 *
 * @param projects - Every project.
 * @param github - Sync data by project id.
 * @param selectedId - Project the board is focused on.
 */
export function boardIssues(
  projects: readonly Project[],
  github: Readonly<Record<string, ProjectGithub>>,
  selectedId: string | null,
): IssueItem[] {
  return projects
    .filter((p) => selectedId === null || p.id === selectedId)
    .flatMap((project) => untrackedIssues(project, github[project.id]).map((issue) => ({ project, issue })))
    .toSorted((a, b) => b.issue.updatedAt.localeCompare(a.issue.updatedAt));
}

/**
 * The synced status of a PR, if the sync has seen it.
 *
 * @param github - Sync data by project id.
 * @param projectId - Project the PR belongs to.
 * @param url - PR URL as stored on the task or run.
 */
export function prFor(
  github: Readonly<Record<string, ProjectGithub>>,
  projectId: string | null,
  url: string | null,
): PrStatus | null {
  if (!projectId || !url) return null;
  return github[projectId]?.prs[url] ?? null;
}

/** Icon for a PR's CI state: a check, a cross, a dot while running, or nothing. */
export type CheckMark = "pass" | "fail" | "pending" | null;

/** How a PR reads at a glance. */
export interface PrSignal {
  /** CI icon; `null` once the PR is merged or closed, or when it has no checks. */
  mark: CheckMark;
  /** CI in words, for tooltips and the detail views. */
  checks: string;
  /** The one thing worth saying next to the icon, if anything. */
  label: string | null;
}

/**
 * CI state in words.
 *
 * @example
 * checksText({ passed: 12, failed: 2, pending: 0, ... }); // "2 of 14 checks failed"
 */
export function checksText(pr: PrStatus): string {
  const total = pr.passed + pr.failed + pr.pending;
  if (total === 0) return "No checks";
  if (pr.failed > 0) return `${pr.failed} of ${total} checks failed`;
  if (pr.pending > 0) return pr.pending === total ? `${total} ${total === 1 ? "check" : "checks"} running` : `${pr.pending} of ${total} checks running`;
  return total === 1 ? "Check passed" : `All ${total} checks passed`;
}

/**
 * The review decision in words, or `null` when there is nothing to say.
 *
 * @param pr - PR status.
 */
export function reviewText(pr: PrStatus): string | null {
  switch (pr.review) {
    case "approved":
      return "Approved";
    case "changes_requested":
      return "Changes requested";
    case "review_required":
      return "Review required";
    default:
      return null;
  }
}

/**
 * A PR's state for compact indicators. Merged and closed PRs say so and
 * drop the CI icon; open ones show CI and only the review decisions that
 * need attention or settle it ("Changes requested", "Approved").
 *
 * @param pr - PR status.
 */
export function prSignal(pr: PrStatus): PrSignal {
  const checks = checksText(pr);
  if (pr.state === "merged") return { mark: null, checks, label: "Merged" };
  if (pr.state === "closed") return { mark: null, checks, label: "PR closed" };
  const mark: CheckMark = pr.checks === "passing" ? "pass" : pr.checks === "failing" ? "fail" : pr.checks === "pending" ? "pending" : null;
  const label =
    pr.review === "changes_requested" ? "Changes requested" : pr.review === "approved" ? "Approved" : pr.draft ? "Draft" : null;
  return { mark, checks, label };
}

/**
 * How long ago an RFC 3339 time was, in words.
 *
 * @param iso - Past time.
 * @param now - Reference time.
 */
export function ago(iso: string, now: number = Date.now()): string {
  const minutes = Math.floor((now - Date.parse(iso)) / 60_000);
  if (!Number.isFinite(minutes) || minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.floor(hours / 24);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

/**
 * The sync status line for project settings.
 *
 * @param status - The project's sync status.
 * @param syncing - Whether a sync is running now.
 * @param now - Reference time.
 */
export function syncText(status: SyncStatus, syncing: boolean, now: number = Date.now()): string {
  if (syncing) return "Syncing…";
  const retry = status.retryAt ? ` Trying again at ${formatTime(new Date(status.retryAt))}.` : "";
  switch (status.kind) {
    case "waiting":
      return "Not synced yet";
    case "ok":
      return status.syncedAt ? `${status.message} · synced ${ago(status.syncedAt, now)}` : status.message;
    case "not_github":
      return "The remote isn't on GitHub";
    case "no_gh":
      return "Needs the GitHub CLI. Install it and run gh auth login.";
    case "signed_out":
      return "gh isn't signed in. Run gh auth login in a terminal.";
    case "rate_limited":
      return `GitHub's rate limit was reached.${retry}`;
    case "error":
      return `${status.message.replace(/\.$/, "")}.${retry}`;
  }
}

/**
 * A label's color as CSS, or `undefined` when GitHub sent something odd.
 *
 * @param color - Hex color without `#`.
 */
export function labelColor(color: string): string | undefined {
  return /^[0-9a-f]{6}$/i.test(color) ? `#${color}` : undefined;
}
