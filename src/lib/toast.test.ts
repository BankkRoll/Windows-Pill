import { describe, expect, it } from "vitest";
import { toastTimeLeft, TOAST_MS } from "./toast";

describe("toastTimeLeft", () => {
  it("counts from when the toast was raised", () => {
    expect(toastTimeLeft({ tone: "info", at: 10_000 }, 10_000)).toBe(TOAST_MS.info);
    expect(toastTimeLeft({ tone: "info", at: 10_000 }, 11_500)).toBe(TOAST_MS.info - 1_500);
  });

  it("gives errors longer", () => {
    expect(toastTimeLeft({ tone: "error", at: 0 }, 5_000)).toBeGreaterThan(0);
    expect(toastTimeLeft({ tone: "info", at: 0 }, 5_000)).toBeLessThanOrEqual(0);
  });

  it("is stale once its time has passed", () => {
    expect(toastTimeLeft({ tone: "error", at: 0 }, 60_000)).toBeLessThanOrEqual(0);
  });
});
