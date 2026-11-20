import { describe, expect, it } from "vitest";
import { fuzzyMatch, parseQuickAdd, resolveDue } from "./quickadd";

// Thursday, 19 November 2026.
const THU = new Date(2026, 10, 19);

describe("resolveDue", () => {
  it.each([
    ["today", "2026-11-19"],
    ["tomorrow", "2026-11-20"],
    ["tmr", "2026-11-20"],
    ["thu", "2026-11-19"],
    ["fri", "2026-11-20"],
    ["friday", "2026-11-20"],
    ["mon", "2026-11-23"],
    ["wed", "2026-11-25"],
    ["+3d", "2026-11-22"],
    ["+2w", "2026-12-03"],
    ["2026-12-31", "2026-12-31"],
  ])("%s → %s", (word, expected) => {
    expect(resolveDue(word, THU)).toBe(expected);
  });

  it("rejects impossible or unknown dates", () => {
    expect(resolveDue("2026-02-30", THU)).toBeNull();
    expect(resolveDue("someday", THU)).toBeNull();
    expect(resolveDue("frog", THU)).toBeNull();
  });
});

describe("parseQuickAdd", () => {
  it("extracts every token from anywhere in the line", () => {
    const p = parseQuickAdd("!1 Fix bonus #outlaw round @fri +bug +Bug", THU);
    expect(p.title).toBe("Fix bonus round");
    expect(p.projectQuery).toBe("outlaw");
    expect(p.priority).toBe(1);
    expect(p.due).toBe("2026-11-20");
    expect(p.tags).toEqual(["bug"]);
  });

  it("keeps unparseable @words in the title and flags them", () => {
    const p = parseQuickAdd("email @bob about it", THU);
    expect(p.title).toBe("email @bob about it");
    expect(p.invalidDue).toBe("@bob");
    expect(p.due).toBeNull();
  });

  it("leaves plain text alone", () => {
    const p = parseQuickAdd("  just a task  ", THU);
    expect(p).toMatchObject({ title: "just a task", projectQuery: null, priority: null, due: null });
  });

  it("ignores out-of-range priorities", () => {
    expect(parseQuickAdd("x !4", THU).priority).toBeNull();
  });
});

describe("fuzzyMatch", () => {
  const projects = [{ name: "pill" }, { name: "outlaw-slots" }, { name: "Outlaw" }, { name: "web-app" }];

  it("prefers exact, then prefix, then word prefix", () => {
    expect(fuzzyMatch("outlaw", projects)?.name).toBe("Outlaw");
    expect(fuzzyMatch("outl", projects)?.name).toBe("outlaw-slots");
    expect(fuzzyMatch("slots", projects)?.name).toBe("outlaw-slots");
    expect(fuzzyMatch("app", projects)?.name).toBe("web-app");
  });

  it("falls back to subsequence and returns null on no match", () => {
    expect(fuzzyMatch("wpp", projects)?.name).toBe("web-app");
    expect(fuzzyMatch("zzz", projects)).toBeNull();
    expect(fuzzyMatch("", projects)).toBeNull();
  });
});
