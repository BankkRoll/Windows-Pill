import { describe, expect, it } from "vitest";
import { formatDuration } from "../components/tabs/AgentsTab";
import type { RunRecord, RunState } from "./native";
import { runSignals, waitingLabel } from "./tasks";
import { displayedRun, sameViewing, viewingOf, type ViewState } from "./viewing";

const NOW = Date.parse("2026-11-19T12:00:00Z");

function run(id: string, state: RunState, endedAt: string | null = null, over: Partial<RunRecord> = {}): RunRecord {
  return {
    id,
    taskId: `t_${id}`,
    projectId: "p",
    taskTitle: id,
    state,
    branch: "agent/x",
    worktree: "C:\\wt",
    claudeSessionId: null,
    startedAt: "2026-11-19T11:00:00Z",
    endedAt,
    tests: { passed: null, summary: null },
    prUrl: null,
    error: null,
    planMode: false,
    provider: "claude",
    plan: null,
    answer: null,
    testsDetected: null,
    resumedAt: null,
    worktreePresent: true,
    filesChanged: [],
    activity: null,
    turns: null,
    costUsd: null,
    model: null,
    effort: null,
    choice: { model: "", effort: "" },
    compareGroup: null,
    diff: null,
    handsOff: false,
    fixAttempts: 0,
    ...over,
  };
}

describe("runSignals", () => {
  const runs = [
    run("a", "running"),
    run("b", "queued"),
    run("c", "failed", "2026-11-19T11:30:00Z"),
    run("d", "pr_open", "2026-11-19T11:40:00Z"),
    run("e", "awaiting_approval"),
    run("old", "failed", "2026-11-17T11:00:00Z"),
  ];

  it("counts active, fresh failures, fresh PRs and pending approvals", () => {
    expect(runSignals(runs, new Set(), NOW)).toEqual({ agent: 2, failed: 1, pr: 1, approval: 1 });
  });

  it("counts pushed and locally committed runs as ready for review", () => {
    const shipped = [run("p", "pushed", "2026-11-19T11:50:00Z"), run("l", "committed", "2026-11-19T11:55:00Z")];
    expect(runSignals(shipped, new Set(), NOW).pr).toBe(2);
  });

  it("clears finished runs once seen", () => {
    expect(runSignals(runs, new Set(["c", "d"]), NOW)).toEqual({ agent: 2, failed: 0, pr: 0, approval: 1 });
  });
});

describe("formatDuration", () => {
  it.each([
    [4_200, "4s"],
    [65_000, "1m 05s"],
    [3_725_000, "1h 02m"],
    [-5, "0s"],
  ])("%d ms → %s", (ms, expected) => {
    expect(formatDuration(ms)).toBe(expected);
  });
});

describe("waitingLabel", () => {
  it("names the runs that stopped to wait for the user", () => {
    expect(waitingLabel(run("a", "awaiting_approval"))).toBe("Plan ready");
    expect(waitingLabel(run("b", "ready"))).toBe("Pick a version");
    expect(waitingLabel(run("c", "running"))).toBeNull();
    expect(waitingLabel(run("d", "failed"))).toBeNull();
    expect(waitingLabel(null)).toBeNull();
  });
});

describe("viewing", () => {
  const runs = [run("new", "running", null, { projectId: "p2" }), run("old", "failed", "2026-11-19T11:30:00Z")];
  const base: ViewState = { activeTab: "agents", detailTaskId: null, runs, selectedProjectId: null, selectedRunId: null };

  it("shows the selected run, else the newest one the filter lets through", () => {
    expect(displayedRun(runs, null, "old")?.id).toBe("old");
    expect(displayedRun(runs, null, null)?.id).toBe("new");
    expect(displayedRun(runs, "p", "new")?.id).toBe("old");
    expect(displayedRun(runs, "nope", null)).toBeNull();
  });

  it("reports the run on screen in the Agents tab", () => {
    expect(viewingOf({ ...base, selectedRunId: "old" })).toEqual({ runId: "old", taskId: "t_old" });
    expect(viewingOf({ ...base, runs: [] })).toBeNull();
  });

  it("reports an open task page over any tab", () => {
    expect(viewingOf({ ...base, activeTab: "board", detailTaskId: "t_9" })).toEqual({ runId: null, taskId: "t_9" });
    expect(viewingOf({ ...base, detailTaskId: "t_9" })).toEqual({ runId: null, taskId: "t_9" });
  });

  it("reports nothing for tabs that show no run or task", () => {
    expect(viewingOf({ ...base, activeTab: "board" })).toBeNull();
    expect(viewingOf({ ...base, activeTab: "today" })).toBeNull();
  });

  it("compares views by run and task", () => {
    expect(sameViewing(null, null)).toBe(true);
    expect(sameViewing({ runId: "a", taskId: "t" }, { runId: "a", taskId: "t" })).toBe(true);
    expect(sameViewing({ runId: "a", taskId: "t" }, null)).toBe(false);
    expect(sameViewing({ runId: "a", taskId: "t" }, { runId: "b", taskId: "t" })).toBe(false);
  });
});
