import { describe, expect, it } from "vitest";
import { matchScore, rankCommands, type Command } from "./commands";

const cmd = (title: string, keywords = ""): Command => ({ id: title, group: "Pill", title, keywords, run: () => {} });

describe("matchScore", () => {
  it("prefers word starts over substrings over scattered letters", () => {
    const start = matchScore("boa", "Go to Board");
    const sub = matchScore("oar", "Go to Board");
    const scattered = matchScore("gbd", "Go to Board");
    expect(start).toBeGreaterThan(sub);
    expect(sub).toBeGreaterThan(scattered);
    expect(scattered).toBeGreaterThan(0);
  });

  it("returns 0 when letters are missing", () => {
    expect(matchScore("xyz", "Board")).toBe(0);
  });
});

describe("rankCommands", () => {
  it("ranks the best match first and drops non-matches", () => {
    const ranked = rankCommands("set", [cmd("Board"), cmd("Settings"), cmd("Reset filters")]);
    expect(ranked.map((c) => c.title)).toEqual(["Settings", "Reset filters"]);
  });

  it("matches keywords", () => {
    expect(rankCommands("kanban", [cmd("Board", "kanban columns"), cmd("Today")]).map((c) => c.title)).toEqual(["Board"]);
  });
});
