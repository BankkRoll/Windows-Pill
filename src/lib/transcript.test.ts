import { describe, expect, it } from "vitest";
import type { LogLine, RunEvent } from "./native";
import { mergeLog, toolSummary, transcriptBlocks } from "./transcript";

const line = (e: RunEvent): LogLine => ({ ...e, runId: "r_1", at: "" });

describe("transcriptBlocks", () => {
  it("folds tool calls between messages into one block", () => {
    const blocks = transcriptBlocks([
      line({ kind: "step", text: "Creating worktree" }),
      line({ kind: "tool", name: "Read", detail: "src/a.ts" }),
      line({ kind: "tool", name: "Edit", detail: "src/a.ts" }),
      line({ kind: "text", text: "Fixed the off-by-one." }),
      line({ kind: "tool", name: "Bash", detail: "pnpm test" }),
      line({ kind: "result", ok: true, text: "Tests passed" }),
    ]);
    expect(blocks).toEqual([
      { kind: "step", text: "Creating worktree" },
      { kind: "tools", tools: ["Read src/a.ts", "Edit src/a.ts"] },
      { kind: "message", text: "Fixed the off-by-one." },
      { kind: "tools", tools: ["Bash pnpm test"] },
      { kind: "result", ok: true, text: "Tests passed" },
    ]);
  });

  it("keeps a step between tool calls as its own block", () => {
    const blocks = transcriptBlocks([
      line({ kind: "tool", name: "Read", detail: "a" }),
      line({ kind: "step", text: "Verifying" }),
      line({ kind: "tool", name: "Read", detail: "b" }),
    ]);
    expect(blocks.map((b) => b.kind)).toEqual(["tools", "step", "tools"]);
  });
});

describe("toolSummary", () => {
  it("counts calls by tool", () => {
    expect(toolSummary(["Read a", "Read b", "Grep x", "Edit a"])).toBe("Read 2 · Grep · Edit");
  });
});

describe("mergeLog", () => {
  const at = (e: RunEvent, time: string): LogLine => ({ ...e, runId: "r_1", at: time });
  const a = at({ kind: "step", text: "Creating worktree" }, "10:00:00");
  const b = at({ kind: "tool", name: "Read", detail: "a.ts" }, "10:00:01");
  const c = at({ kind: "text", text: "Done." }, "10:00:02");

  it("puts the earlier transcript before the live tail without repeating lines", () => {
    expect(mergeLog([a, b], [b, c])).toEqual([a, b, c]);
  });

  it("takes the history alone when nothing arrived live", () => {
    expect(mergeLog([a, b, c], [])).toEqual([a, b, c]);
  });

  it("keeps live lines when the history couldn't be loaded", () => {
    expect(mergeLog([], [b, c])).toEqual([b, c]);
  });

  it("is idempotent when the history is loaded twice", () => {
    expect(mergeLog([a, b], mergeLog([a, b], [b, c]))).toEqual([a, b, c]);
  });

  it("keeps a line that really was logged twice", () => {
    expect(mergeLog([b], [b, b])).toEqual([b, b]);
  });
});
