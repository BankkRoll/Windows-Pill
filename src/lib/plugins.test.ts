import { describe, expect, it } from "vitest";
import { PLUGIN_ICONS } from "../components/PluginIcon";
import { mergeSignals, restingShape } from "../store";
import type { FeedItem, PluginState } from "./native";
import {
  activeLive,
  collapsedLayout,
  feedEntries,
  foldList,
  groupFeed,
  MAX_TODAY_EXTENSIONS,
  pluginSignals,
  pluginWidgets,
  todayCards,
} from "./plugins";
import rust from "../../src-tauri/src/plugins/protocol.rs?raw";
import doc from "../../plugins/README.md?raw";

function plugin(id: string, patch: Partial<PluginState> = {}): PluginState {
  return {
    id,
    name: id.toUpperCase(),
    description: "",
    source: "external",
    capabilities: ["signals", "live", "widget"],
    enabled: true,
    status: { state: "running", message: null, fix: null },
    command: "node plugin.js",
    approval: "node plugin.js",
    folder: null,
    signals: [],
    live: null,
    widget: null,
    feed: [],
    tab: null,
    nav: [],
    pinned: true,
    ...patch,
  };
}

function item(id: string, severity: FeedItem["severity"]): FeedItem {
  return { id, severity, icon: "shield", title: id, body: "", detail: null, url: null, projectId: "p_1", task: null, actions: [] };
}

describe("pluginSignals", () => {
  it("merges the notification count and keeps custom icons separate", () => {
    const list = [
      plugin("notifications", {
        signals: [{ id: "count", kind: "notification", icon: "bell", tone: "neutral", count: 3, label: "3 notifications" }],
      }),
      plugin("ci", { signals: [{ id: "red", kind: null, icon: "warning", tone: "red", count: 2, label: "" }] }),
      plugin("off", { enabled: false, signals: [{ id: "x", kind: null, icon: "star", tone: "neutral", count: 1, label: "x" }] }),
    ];
    const signals = pluginSignals(list);
    expect(signals).toEqual([
      { kind: "notification", count: 3 },
      { kind: "plugin", count: 2, plugin: { key: "ci:red", icon: "warning", tone: "red", label: "CI" } },
    ]);
    const merged = mergeSignals({ plugins: signals, agents: [{ kind: "failed", count: 1 }], tasks: [{ kind: "due", count: 1 }] });
    expect(merged.map((s) => s.plugin?.key ?? s.kind)).toEqual(["failed", "due", "ci:red", "notification"]);
  });
});

describe("todayCards", () => {
  it("puts built-ins first and gives feed-only plugins a card", () => {
    const card = { kind: "card" as const, title: "CVE watch", rows: [], actions: [] };
    const media = plugin("now-playing", { source: "builtin", widget: { kind: "card", title: "Now playing", rows: [], actions: [] } });
    const cve = plugin("cve", { source: "external", widget: card, feed: [item("a", "high")] });
    const ci = plugin("ci", { source: "external", feed: [item("b", "low")] });
    const quiet = plugin("quiet", { source: "external" });
    const off = plugin("off", { source: "external", enabled: false, widget: card });
    const { native, extensions } = todayCards([cve, media, ci, quiet, off]);
    expect(native.map((w) => w.plugin.id)).toEqual(["now-playing"]);
    expect(extensions.map((e) => [e.plugin.id, e.widget !== null])).toEqual([
      ["cve", true],
      ["ci", false],
    ]);
  });

  it("keeps the user's order and folds third-party cards past the limit", () => {
    const ids = ["e", "c", "a", "d", "b"];
    const list = ids.map((id) => plugin(id, { feed: [item(`${id}-1`, "info")] }));
    const { extensions, more } = todayCards(list);
    expect(extensions.map((e) => e.plugin.id)).toEqual(ids.slice(0, MAX_TODAY_EXTENSIONS));
    expect(more.map((e) => e.plugin.id)).toEqual(ids.slice(MAX_TODAY_EXTENSIONS));
    expect(todayCards(list.slice(0, MAX_TODAY_EXTENSIONS)).more).toEqual([]);
  });
});

describe("foldList", () => {
  const list = ["a", "b", "c", "d", "e", "f"];

  it("shows short lists whole and folds the tail of long ones", () => {
    expect(foldList(list.slice(0, 4), 4)).toEqual({ shown: ["a", "b", "c", "d"], hidden: [] });
    expect(foldList(list, 4)).toEqual({ shown: ["a", "b", "c", "d"], hidden: ["e", "f"] });
  });

  it("never folds away a kept entry", () => {
    expect(foldList(list, 3, { keep: (x) => x === "e" })).toEqual({ shown: ["a", "b", "c", "e"], hidden: ["d", "f"] });
  });

  it("skips a fold that would hide fewer than minHidden entries", () => {
    expect(foldList(list.slice(0, 5), 4, { minHidden: 2 }).hidden).toEqual([]);
    expect(foldList(list, 4, { minHidden: 2 }).hidden).toEqual(["e", "f"]);
    expect(foldList(list, 4, { minHidden: 2, keep: (x) => x === "f" }).hidden).toEqual([]);
  });
});

describe("feed", () => {
  const list = [
    plugin("cve", { feed: [item("low", "low"), item("crit", "critical"), item("high", "high")] }),
    plugin("ci", { feed: [item("ci-high", "high"), item("ci-info", "info")] }),
    plugin("off", { enabled: false, feed: [item("hidden", "critical")] }),
  ];

  it("lists enabled plugins' items most severe first, keeping order within a severity", () => {
    expect(feedEntries(list).map((e) => `${e.plugin.id}:${e.item.id}`)).toEqual([
      "cve:crit",
      "cve:high",
      "ci:ci-high",
      "cve:low",
      "ci:ci-info",
    ]);
  });

  it("groups one advisory across projects into a single row, most widespread first", () => {
    const adv = (id: string, projectId: string, url: string | null, title = "Prototype pollution"): FeedItem => ({
      ...item(id, "high"),
      title,
      url,
      projectId,
    });
    const cve = plugin("cve", {
      feed: [
        adv("a1", "p_1", "https://osv.dev/GHSA-1"),
        adv("b1", "p_1", "https://osv.dev/GHSA-2"),
        adv("a2", "p_2", "https://osv.dev/GHSA-1"),
      ],
    });
    const ci = plugin("ci", { feed: [adv("c1", "p_1", null, "Build red"), adv("c2", "p_2", null, "Build red")] });
    const groups = groupFeed(feedEntries([cve, ci]));
    expect(groups.map((g) => g.entries.map((e) => e.item.id))).toEqual([["a1", "a2"], ["c1", "c2"], ["b1"]]);
  });

  it("raises one security signal for critical and high items", () => {
    const signals = pluginSignals(list);
    expect(signals[0]).toEqual({ kind: "security", count: 3 });
    expect(pluginSignals([plugin("cve", { feed: [item("m", "medium")] })])).toEqual([]);
    const merged = mergeSignals({
      plugins: signals,
      agents: [
        { kind: "failed", count: 1 },
        { kind: "pr", count: 1 },
      ],
    });
    expect(merged.map((s) => s.kind)).toEqual(["failed", "security", "pr"]);
  });
});

describe("activeLive and widgets", () => {
  it("picks the first enabled plugin with a live item", () => {
    const list = [
      plugin("a", { enabled: false, live: { kind: "text", icon: null, text: "off", tone: "neutral" } }),
      plugin("b"),
      plugin("c", {
        live: { kind: "text", icon: "timer", text: "18:42", tone: "accent" },
        widget: { kind: "card", title: "C", rows: [], actions: [] },
      }),
      plugin("d", { live: { kind: "text", icon: null, text: "later", tone: "neutral" } }),
    ];
    expect(activeLive(list)?.plugin.id).toBe("c");
    expect(activeLive([])).toBeNull();
    expect(pluginWidgets(list).map((w) => w.plugin.id)).toEqual(["c"]);
  });

  it("a live item earns the peek shape", () => {
    expect(restingShape([], false)).toBe("idle");
    expect(restingShape([], true)).toBe("peek");
  });
});

describe("collapsedLayout", () => {
  const s = (n: number) => Array.from({ length: n }, () => ({ kind: "due" as const, count: 1 }));
  const media = { kind: "media" as const, artwork: null, playing: true, title: "Song" };
  const text = { kind: "text" as const, icon: null, text: "Deploying", tone: "neutral" as const };

  it("keeps the date while there is room", () => {
    expect(collapsedLayout(s(2), null).showDate).toBe(true);
    expect(collapsedLayout(s(3), null).showDate).toBe(false);
    expect(collapsedLayout(s(1), media).showDate).toBe(true);
    expect(collapsedLayout(s(2), media).showDate).toBe(false);
    expect(collapsedLayout(s(0), text).showDate).toBe(true);
    expect(collapsedLayout(s(1), text).showDate).toBe(false);
  });

  it("folds extra signals into a count", () => {
    expect(collapsedLayout(s(5), null)).toMatchObject({ overflow: 0 });
    expect(collapsedLayout(s(6), null).signals).toHaveLength(4);
    expect(collapsedLayout(s(6), null).overflow).toBe(2);
    expect(collapsedLayout(s(3), text).signals).toHaveLength(1);
    expect(collapsedLayout(s(3), text).overflow).toBe(2);
    expect(collapsedLayout(s(4), media).overflow).toBe(0);
  });
});

describe("plugin icon vocabulary", () => {
  it("matches the core's list and the docs", () => {
    const block = rust.slice(rust.indexOf("pub const ICON_NAMES"), rust.indexOf("];", rust.indexOf("pub const ICON_NAMES")));
    const names = [...block.matchAll(/"([a-z-]+)"/g)].map((m) => m[1]);
    expect(names.length).toBeGreaterThan(20);
    expect(Object.keys(PLUGIN_ICONS).sort()).toEqual([...names].sort());
    const icons = doc.slice(doc.indexOf("### Icons"), doc.indexOf("### Tones"));
    for (const n of names) expect(icons).toContain(`\`${n}\``);
  });
});
