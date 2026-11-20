import { describe, expect, it } from "vitest";
import { CHECK_INTERVAL_MS, describeUpdateError, progressPercent, shouldCheck } from "./updates";

describe("shouldCheck", () => {
  const now = 1_800_000_000_000;

  it("checks when there is no usable record", () => {
    expect(shouldCheck(null, now)).toBe(true);
    expect(shouldCheck(Number.NaN, now)).toBe(true);
    expect(shouldCheck(now + 60_000, now)).toBe(true);
  });

  it("checks at most once a day", () => {
    expect(shouldCheck(now - 60_000, now)).toBe(false);
    expect(shouldCheck(now - CHECK_INTERVAL_MS + 1, now)).toBe(false);
    expect(shouldCheck(now - CHECK_INTERVAL_MS, now)).toBe(true);
  });
});

describe("progressPercent", () => {
  it("rounds and clamps", () => {
    expect(progressPercent(0, 200)).toBe(0);
    expect(progressPercent(101, 200)).toBe(51);
    expect(progressPercent(500, 200)).toBe(100);
  });

  it("is unknown without a size", () => {
    expect(progressPercent(10, null)).toBeNull();
    expect(progressPercent(10, 0)).toBeNull();
  });
});

describe("describeUpdateError", () => {
  it("explains being offline", () => {
    expect(describeUpdateError("error sending request for url (https://github.com/…)")).toMatch(/Couldn't reach GitHub/);
    expect(describeUpdateError(new Error("operation timed out"))).toMatch(/Couldn't reach GitHub/);
  });

  it("explains a bad signature and a missing release", () => {
    expect(describeUpdateError("signature verification failed")).toMatch(/signature check/);
    expect(describeUpdateError("Could not fetch a valid release JSON from the remote")).toMatch(/No release information/);
  });

  it("passes other messages through", () => {
    expect(describeUpdateError("disk full")).toBe("disk full");
  });
});
