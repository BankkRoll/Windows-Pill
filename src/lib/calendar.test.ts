import { describe, expect, it } from "vitest";
import { addMonths, firstDayOfWeek, monthGrid, parseDay, startOfWeek } from "./calendar";
import { isoDay } from "./format";

describe("parseDay", () => {
  it("reads a stored due date as local midnight", () => {
    expect(parseDay("2026-11-21")?.getTime()).toBe(new Date(2026, 10, 21).getTime());
  });

  it("rejects malformed and impossible days", () => {
    expect(parseDay("2026-2-1")).toBeNull();
    expect(parseDay("2026-02-30")).toBeNull();
    expect(parseDay("")).toBeNull();
  });
});

describe("addMonths", () => {
  it("clamps to the end of a shorter month", () => {
    expect(isoDay(addMonths(new Date(2026, 0, 31), 1))).toBe("2026-02-28");
    expect(isoDay(addMonths(new Date(2028, 0, 31), 1))).toBe("2028-02-29");
  });

  it("crosses year boundaries both ways", () => {
    expect(isoDay(addMonths(new Date(2026, 11, 15), 1))).toBe("2027-01-15");
    expect(isoDay(addMonths(new Date(2026, 0, 15), -1))).toBe("2025-12-15");
  });
});

describe("monthGrid", () => {
  it("starts on the requested weekday and covers six weeks", () => {
    const monday = monthGrid(2026, 10, 1);
    expect(monday).toHaveLength(42);
    expect(isoDay(monday[0]!)).toBe("2026-10-26");
    expect(isoDay(monday[41]!)).toBe("2026-12-06");

    const sunday = monthGrid(2026, 10, 0);
    expect(isoDay(sunday[0]!)).toBe("2026-11-01");
  });
});

describe("startOfWeek", () => {
  it("returns the same day when it already starts the week", () => {
    expect(isoDay(startOfWeek(new Date(2026, 10, 16), 1))).toBe("2026-11-16");
    expect(isoDay(startOfWeek(new Date(2026, 10, 22), 1))).toBe("2026-11-16");
  });
});

describe("firstDayOfWeek", () => {
  it("returns a weekday index", () => {
    const day = firstDayOfWeek("en-GB");
    expect(day).toBeGreaterThanOrEqual(0);
    expect(day).toBeLessThan(7);
  });
});
