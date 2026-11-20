import { describe, expect, it } from "vitest";
import { contentGroup, grows, notchPath, shapeRect } from "./shapes";

describe("shapeRect", () => {
  it("attaches the idle notch to the top edge, centered", () => {
    expect(shapeRect("idle", 800)).toEqual({ x: 274, y: 0, width: 252, height: 30, radius: 14 });
  });

  it("centers the expanded panel", () => {
    expect(shapeRect("expanded", 800).x).toBe(30);
  });
});

describe("notchPath", () => {
  const g = { width: 200, height: 30, radius: 14, shoulder: 14, shoulderDepth: 16 };

  it("starts and ends on the screen edge, spanning body plus both shoulders", () => {
    const d = notchPath(g, 10);
    expect(d.startsWith("M10,0")).toBe(true);
    expect(d).toContain(" 238,0 Z");
  });

  it("clamps the shoulder so it never runs past the bottom corner", () => {
    const d = notchPath({ ...g, shoulderDepth: 99 }, 0);
    // Shoulder ends at h - r = 16, and the vertical side has zero length.
    expect(d).toContain("14,16 L14,16");
  });
});

describe("grows", () => {
  it("treats opening as growth and closing as shrink", () => {
    expect(grows("idle", "expanded")).toBe(true);
    expect(grows("expanded", "idle")).toBe(false);
    expect(grows("idle", "peek")).toBe(true);
  });
});

describe("contentGroup", () => {
  it("shares collapsed content between idle and peek", () => {
    expect(contentGroup("idle")).toBe(contentGroup("peek"));
    expect(contentGroup("expanded")).not.toBe(contentGroup("idle"));
  });
});
