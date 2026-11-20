import { describe, expect, it, vi } from "vitest";
import { freshness } from "./freshness";

describe("freshness", () => {
  it("applies a fetch result when no event came in meanwhile", () => {
    const fresh = freshness();
    const apply = vi.fn();
    fresh.guard("runs", apply)(["fetched"]);
    expect(apply).toHaveBeenCalledWith(["fetched"]);
  });

  it("drops a fetch result older than an event", () => {
    const fresh = freshness();
    const apply = vi.fn();
    const done = fresh.guard("runs", apply);
    fresh.touch("runs");
    done(["stale"]);
    expect(apply).not.toHaveBeenCalled();
  });

  it("keeps keys independent", () => {
    const fresh = freshness();
    const apply = vi.fn();
    const done = fresh.guard("snapshot", apply);
    fresh.touch("runs");
    done("fresh");
    expect(apply).toHaveBeenCalledWith("fresh");
  });

  it("only counts events after the fetch started", () => {
    const fresh = freshness();
    fresh.touch("config");
    const apply = vi.fn();
    fresh.guard("config", apply)("fetched");
    expect(apply).toHaveBeenCalledOnce();
  });
});
