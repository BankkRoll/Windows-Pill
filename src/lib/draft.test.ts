import { describe, expect, it } from "vitest";
import { rebaseDraft, sameContent } from "./draft";

describe("sameContent", () => {
  it("compares by value", () => {
    expect(sameContent({ a: 1, b: [2] }, { a: 1, b: [2] })).toBe(true);
    expect(sameContent({ a: 1 }, { a: 2 })).toBe(false);
  });
});

describe("rebaseDraft", () => {
  const saved = { name: "Pill", testCommand: "pnpm test" };

  it("keeps the draft when a snapshot brings the same settings again", () => {
    const draft = { ...saved, name: "Pill (editing)" };
    expect(rebaseDraft(draft, saved, { ...saved })).toBe(draft);
  });

  it("keeps the same draft object when nothing changed at all", () => {
    const draft = { ...saved };
    expect(rebaseDraft(draft, saved, { ...saved })).toBe(draft);
  });

  it("adopts an outside change when the draft has no edits", () => {
    const next = { ...saved, testCommand: "pnpm vitest" };
    expect(rebaseDraft({ ...saved }, saved, next)).toBe(next);
  });

  it("never overwrites unsaved edits with an outside change", () => {
    const draft = { ...saved, name: "Mine" };
    expect(rebaseDraft(draft, saved, { ...saved, testCommand: "cargo test" })).toBe(draft);
  });
});
