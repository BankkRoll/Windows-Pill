import { describe, expect, it } from "vitest";
import { ago, boardIssues, checksText, labelColor, prFor, prSignal, reviewText, syncText, untrackedIssues } from "./github";
import type { GhIssue, Project, ProjectGithub, PrStatus, Task } from "./native";

const NOW = Date.parse("2026-11-19T12:00:00Z");

function issue(number: number, updatedAt = "2026-11-19T10:00:00Z"): GhIssue {
  return { number, title: `Issue ${number}`, url: `https://github.com/o/r/issues/${number}`, body: "", labels: [], updatedAt };
}

function task(id: string, issueNumber: number | null): Task {
  return {
    id,
    title: id,
    notes: "",
    status: "todo",
    priority: null,
    tags: [],
    due: null,
    order: 0,
    agentReady: false,
    pinnedNotes: [],
    createdAt: "",
    updatedAt: "",
    agent: { lastRunId: null, branch: null, prUrl: null, blocked: null },
    issue: issueNumber === null ? null : { number: issueNumber, url: "", closed: false },
  };
}

function project(id: string, tasks: Task[], githubIssues = true): Project {
  return { id, name: id, tasks, settings: { githubIssues } } as unknown as Project;
}

function gh(issues: GhIssue[], prs: Record<string, PrStatus> = {}): ProjectGithub {
  return {
    repo: "o/r",
    status: { kind: "ok", message: "", syncedAt: null, retryAt: null },
    syncing: false,
    issues,
    prs,
  };
}

function pr(over: Partial<PrStatus> = {}): PrStatus {
  return {
    number: 1,
    url: "https://github.com/o/r/pull/1",
    state: "open",
    draft: false,
    review: "none",
    checks: "none",
    passed: 0,
    failed: 0,
    pending: 0,
    ...over,
  };
}

describe("untrackedIssues", () => {
  it("hides issues that already have a task", () => {
    const p = project("a", [task("t1", 2), task("t2", null)]);
    expect(untrackedIssues(p, gh([issue(1), issue(2), issue(3)])).map((i) => i.number)).toEqual([1, 3]);
  });

  it("shows nothing when the project hides issues or hasn't synced", () => {
    expect(untrackedIssues(project("a", [], false), gh([issue(1)]))).toEqual([]);
    expect(untrackedIssues(project("a", []), undefined)).toEqual([]);
  });
});

describe("boardIssues", () => {
  const a = project("a", []);
  const b = project("b", [task("t", 5)]);
  const data = { a: gh([issue(1, "2026-11-18T00:00:00Z")]), b: gh([issue(5), issue(6, "2026-11-19T11:00:00Z")]) };

  it("merges every project, newest first", () => {
    expect(boardIssues([a, b], data, null).map((i) => `${i.project.id}#${i.issue.number}`)).toEqual(["b#6", "a#1"]);
  });

  it("filters to the selected project", () => {
    expect(boardIssues([a, b], data, "a").map((i) => i.issue.number)).toEqual([1]);
  });
});

describe("prFor", () => {
  it("finds a PR by project and URL", () => {
    const status = pr();
    const data = { a: gh([], { [status.url]: status }) };
    expect(prFor(data, "a", status.url)).toBe(status);
    expect(prFor(data, "a", null)).toBeNull();
    expect(prFor(data, "b", status.url)).toBeNull();
    expect(prFor(data, null, status.url)).toBeNull();
  });
});

describe("checksText", () => {
  it("describes CI counts", () => {
    expect(checksText(pr())).toBe("No checks");
    expect(checksText(pr({ passed: 12, failed: 2 }))).toBe("2 of 14 checks failed");
    expect(checksText(pr({ passed: 2, pending: 1 }))).toBe("1 of 3 checks running");
    expect(checksText(pr({ pending: 3 }))).toBe("3 checks running");
    expect(checksText(pr({ pending: 1 }))).toBe("1 check running");
    expect(checksText(pr({ passed: 1 }))).toBe("Check passed");
    expect(checksText(pr({ passed: 9 }))).toBe("All 9 checks passed");
  });
});

describe("prSignal", () => {
  it("shows CI and the review decisions that matter on open PRs", () => {
    expect(prSignal(pr({ checks: "passing", passed: 3, review: "review_required" }))).toEqual({
      mark: "pass",
      checks: "All 3 checks passed",
      label: null,
    });
    expect(prSignal(pr({ checks: "failing", failed: 1, review: "changes_requested" }))).toMatchObject({
      mark: "fail",
      label: "Changes requested",
    });
    expect(prSignal(pr({ checks: "pending", pending: 1, review: "approved" }))).toMatchObject({ mark: "pending", label: "Approved" });
    expect(prSignal(pr({ draft: true }))).toMatchObject({ mark: null, label: "Draft" });
  });

  it("drops CI once a PR is merged or closed", () => {
    expect(prSignal(pr({ state: "merged", checks: "passing", passed: 1 }))).toMatchObject({ mark: null, label: "Merged" });
    expect(prSignal(pr({ state: "closed", checks: "failing", failed: 1 }))).toMatchObject({ mark: null, label: "PR closed" });
  });

  it("names every review decision", () => {
    expect(reviewText(pr({ review: "approved" }))).toBe("Approved");
    expect(reviewText(pr({ review: "changes_requested" }))).toBe("Changes requested");
    expect(reviewText(pr({ review: "review_required" }))).toBe("Review required");
    expect(reviewText(pr())).toBeNull();
  });
});

describe("sync status", () => {
  it("says how long ago", () => {
    expect(ago("2026-11-19T11:59:30Z", NOW)).toBe("just now");
    expect(ago("2026-11-19T11:55:00Z", NOW)).toBe("5 min ago");
    expect(ago("2026-11-19T09:00:00Z", NOW)).toBe("3 h ago");
    expect(ago("2026-11-18T09:00:00Z", NOW)).toBe("yesterday");
    expect(ago("2026-11-15T09:00:00Z", NOW)).toBe("4 days ago");
    expect(ago("not a date", NOW)).toBe("just now");
  });

  it("explains each state in a short line", () => {
    const base = { message: "", syncedAt: null, retryAt: null };
    expect(syncText({ ...base, kind: "ok", message: "2 open issues assigned to you", syncedAt: "2026-11-19T11:58:00Z" }, false, NOW)).toBe(
      "2 open issues assigned to you · synced 2 min ago",
    );
    expect(syncText({ ...base, kind: "ok" }, true, NOW)).toBe("Syncing…");
    expect(syncText({ ...base, kind: "waiting" }, false, NOW)).toBe("Not synced yet");
    expect(syncText({ ...base, kind: "signed_out" }, false, NOW)).toContain("gh auth login");
    expect(syncText({ ...base, kind: "no_gh" }, false, NOW)).toContain("GitHub CLI");
    expect(syncText({ ...base, kind: "not_github" }, false, NOW)).toBe("The remote isn't on GitHub");
    expect(syncText({ ...base, kind: "error", message: "error connecting to api.github.com." }, false, NOW)).toBe(
      "error connecting to api.github.com.",
    );
    const limited = syncText({ ...base, kind: "rate_limited", retryAt: "2026-11-19T12:30:00Z" }, false, NOW);
    expect(limited).toMatch(/^GitHub's rate limit was reached\. Trying again at .+\.$/);
  });
});

describe("labelColor", () => {
  it("accepts only six-digit hex", () => {
    expect(labelColor("0dd8ac")).toBe("#0dd8ac");
    expect(labelColor("red")).toBeUndefined();
    expect(labelColor("0dd8ac;background:url(x)")).toBeUndefined();
  });
});
