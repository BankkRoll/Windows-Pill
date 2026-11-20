import { describe, expect, it } from "vitest";
import type { Project, Snapshot, Task } from "./native";
import { dropOrder, dueCount, locate, reorderSlots, todayGroups } from "./tasks";
import { mergeSignals } from "../store";

const NOW = new Date(2026, 10, 19, 12);

function task(id: string, over: Partial<Task> = {}): Task {
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
    issue: null,
    ...over,
  };
}

function project(id: string, tasks: Task[]): Project {
  return {
    id,
    path: `C:\\dev\\${id}`,
    name: id,
    color: "#fff",
    baseBranch: "main",
    settings: {
      name: "", color: "", baseBranch: "", testCommand: "", lintCommand: "", skipTestSetup: false,
      editor: "auto", agentRules: "", commitTasks: false, planMode: false, provider: "claude", model: "", effort: "",
      ship: "pr", branchPrefix: "", githubIssues: true,
    },
    tasks,
    notes: "",
    hasRemote: true,
    remote: "origin",
    lastActive: 0,
    errors: [],
    manual: false,
  };
}

const snapshot: Snapshot = {
  projects: [
    project("a", [
      task("due-today", { due: "2026-11-19", priority: 2, order: 2 }),
      task("overdue", { due: "2026-11-01", priority: 3, order: 1 }),
      task("doing", { status: "doing", order: 3 }),
      task("future", { due: "2026-12-01" }),
      task("done-overdue", { due: "2026-11-01", status: "done" }),
    ]),
    project("b", [task("later")]),
  ],
  inbox: [task("inbox-due", { status: "inbox", due: "2026-11-19" })],
  inboxError: null,
};

describe("todayGroups", () => {
  it("collects due, overdue and doing tasks in manual order, Inbox last", () => {
    const groups = todayGroups(snapshot, NOW);
    expect(groups.map((g) => g.project?.id ?? "inbox")).toEqual(["a", "inbox"]);
    expect(groups[0]!.tasks.map((t) => t.id)).toEqual(["overdue", "due-today", "doing"]);
  });
});

describe("reorderSlots", () => {
  it("permutes existing slots and only reports changes", () => {
    const a = task("a", { order: 2 });
    const b = task("b", { order: 5 });
    const c = task("c", { order: 9 });
    expect(reorderSlots([a, b, c], [c, a, b])).toEqual([
      { id: "c", order: 2 },
      { id: "a", order: 5 },
      { id: "b", order: 9 },
    ]);
    expect(reorderSlots([a, b, c], [a, b, c])).toEqual([]);
  });

  it("spreads duplicate slots", () => {
    const a = task("a", { order: 1 });
    const b = task("b", { order: 1 });
    expect(reorderSlots([a, b], [b, a])).toEqual([{ id: "a", order: 2 }]);
  });
});

describe("dropOrder", () => {
  const col = [{ order: 1 }, { order: 2 }, { order: 4 }];
  it("places cards between, before and after neighbors", () => {
    expect(dropOrder(col, 0)).toBe(0);
    expect(dropOrder(col, 1)).toBe(1.5);
    expect(dropOrder(col, 2)).toBe(3);
    expect(dropOrder(col, 3)).toBe(5);
    expect(dropOrder([], 0)).toBe(1);
  });
});

describe("dueCount", () => {
  it("counts open tasks due today or earlier", () => {
    expect(dueCount(snapshot, NOW)).toBe(3);
  });
});

describe("locate", () => {
  it("finds tasks in projects and the Inbox", () => {
    expect(locate(snapshot, "later")?.project?.id).toBe("b");
    expect(locate(snapshot, "inbox-due")?.project).toBeNull();
    expect(locate(snapshot, "nope")).toBeNull();
  });
});

describe("mergeSignals", () => {
  it("sums counts per kind across sources in urgency order", () => {
    expect(
      mergeSignals({
        tasks: [{ kind: "due", count: 2 }],
        agents: [{ kind: "agent", count: 1 }, { kind: "failed", count: 1 }],
        plugin: [{ kind: "due", count: 1 }, { kind: "pr", count: 0 }],
      }),
    ).toEqual([
      { kind: "failed", count: 1 },
      { kind: "agent", count: 1 },
      { kind: "due", count: 3 },
    ]);
  });
});
