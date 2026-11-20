import { describe, expect, it } from "vitest";
import type { NotificationFeed, NotificationItem, PluginState } from "./native";
import { cardMode, listedNotes, noteLines, NOTIFICATIONS_PLUGIN, pruneHidden, relativeTime, remainingTotal } from "./notifications";
import rust from "../../src-tauri/src/plugins/notifications.rs?raw";

function note(id: number, patch: Partial<NotificationItem> = {}): NotificationItem {
  return { id, app: "Mail", title: `Message ${id}`, body: "Hi", createdMs: 1_700_000_000_000, icon: null, screenshot: null, ...patch };
}

function feed(ids: number[], total = ids.length): NotificationFeed {
  return { total, notes: ids.map((id) => note(id)) };
}

function plugin(patch: Partial<PluginState> = {}): PluginState {
  return {
    id: "notifications",
    name: "Windows notifications",
    description: "",
    source: "builtin",
    capabilities: ["signals", "widget"],
    enabled: true,
    status: { state: "running", message: null, fix: null },
    command: null,
    folder: null,
    signals: [],
    live: null,
    widget: null,
    approval: null,
    feed: [],
    tab: null,
    nav: [],
    pinned: true,
    ...patch,
  };
}

describe("cardMode", () => {
  it("lists whenever there's a feed, even an empty one", () => {
    expect(cardMode(plugin(), feed([]))).toBe("list");
    expect(cardMode(plugin({ status: { state: "error", message: "x", fix: null } }), feed([1]))).toBe("list");
  });

  it("explains a block only when there's nothing to list", () => {
    const status = { state: "needsPermission", message: "Pill needs your permission to read notifications.", fix: null } as const;
    expect(cardMode(plugin({ status }), null)).toBe("blocked");
    expect(cardMode(plugin({ status: { state: "unsupported", message: "x", fix: null } }), null)).toBe("blocked");
  });

  it("stays out of the way while off or starting", () => {
    expect(cardMode(undefined, feed([1]))).toBe("hidden");
    expect(cardMode(plugin({ enabled: false }), feed([1]))).toBe("hidden");
    expect(cardMode(plugin({ status: { state: "starting", message: null, fix: null } }), null)).toBe("hidden");
  });
});

describe("relativeTime", () => {
  const now = 1_700_000_000_000;

  it("counts minutes, hours and days", () => {
    expect(relativeTime(now - 20_000, now)).toBe("now");
    expect(relativeTime(now - 2 * 60_000, now)).toBe("2m");
    expect(relativeTime(now - 59 * 60_000 - 59_000, now)).toBe("59m");
    expect(relativeTime(now - 60 * 60_000, now)).toBe("1h");
    expect(relativeTime(now - 23 * 3_600_000, now)).toBe("23h");
    expect(relativeTime(now - 3 * 86_400_000, now)).toBe("3d");
  });

  it("switches to a date after a week", () => {
    const label = relativeTime(now - 9 * 86_400_000, now);
    expect(label).not.toMatch(/^\d+[mhd]$/);
    expect(label).toMatch(/\d/);
  });

  it("treats clock skew as now and unknown times as blank", () => {
    expect(relativeTime(now + 5_000, now)).toBe("now");
    expect(relativeTime(0, now)).toBe("");
    expect(relativeTime(Number.NaN, now)).toBe("");
  });
});

describe("noteLines", () => {
  it("uses the title, then the body, then the app name", () => {
    expect(noteLines(note(1))).toEqual({ headline: "Message 1", body: "Hi" });
    expect(noteLines(note(1, { title: "" }))).toEqual({ headline: "Hi", body: "" });
    expect(noteLines(note(1, { title: "", body: "" }))).toEqual({ headline: "Mail", body: "" });
  });
});

describe("optimistic dismiss", () => {
  it("hides dismissed notes and takes them off the total", () => {
    const f = feed([1, 2, 3], 120);
    const hidden = new Set([2]);
    expect(listedNotes(f, hidden).map((n) => n.id)).toEqual([1, 3]);
    expect(remainingTotal(f, hidden)).toBe(119);
    expect(remainingTotal(f, new Set([1, 2, 3]))).toBe(117);
    expect(listedNotes(null, hidden)).toEqual([]);
    expect(remainingTotal(null, hidden)).toBe(0);
  });

  it("forgets dismisses the feed has caught up with", () => {
    const hidden = new Set([1, 2]);
    expect(pruneHidden(hidden, feed([1, 3]))).toEqual(new Set([1]));
    expect(pruneHidden(hidden, feed([1, 2]))).toBe(hidden);
    expect(pruneHidden(hidden, null)).toEqual(new Set());
    const none = new Set<number>();
    expect(pruneHidden(none, feed([1]))).toBe(none);
  });
});

describe("mirrors the core", () => {
  it("uses the plugin id and event the Rust side publishes", () => {
    expect(rust).toContain(`pub const ID: &str = "${NOTIFICATIONS_PLUGIN}";`);
    expect(rust).toContain('pub const NOTIFICATIONS_EVENT: &str = "pill://notifications";');
  });

  it("matches the fields the Rust feed serializes", () => {
    for (const field of ["id", "app", "title", "body", "created_ms", "icon", "screenshot"]) {
      expect(rust).toMatch(new RegExp(`pub ${field}: `));
    }
  });
});
