import { beforeEach, describe, expect, it } from "vitest";
import type { PageState, PluginState, Project } from "./native";
import { headerTabs, MAX_PLUGIN_TABS, navEntries, overviewBadge, pageKey, pluginTabId, pluginTabs, tabPlugin, usePageStore } from "./pages";
import rust from "../../src-tauri/src/plugins/page.rs?raw";

function plugin(id: string, patch: Partial<PluginState> = {}): PluginState {
  return {
    id,
    name: id,
    description: "",
    source: "external",
    capabilities: ["tab", "projects"],
    enabled: true,
    status: { state: "running", message: null, fix: null },
    command: "node plugin.mjs",
    approval: "node plugin.mjs",
    folder: null,
    signals: [],
    live: null,
    widget: null,
    feed: [],
    tab: { title: id, icon: "shield" },
    nav: [],
    pinned: true,
    ...patch,
  };
}

function project(id: string, name: string): Project {
  return { id, name } as Project;
}

describe("plugin tabs", () => {
  it("map to tab ids and back", () => {
    expect(pluginTabId("cve-watch")).toBe("plugin:cve-watch");
    expect(tabPlugin("plugin:cve-watch")).toBe("cve-watch");
    expect(tabPlugin("today")).toBeNull();
    expect(tabPlugin("plugin:")).toBeNull();
  });

  it("come from enabled plugins with a tab, in Settings order", () => {
    const list = [
      plugin("a"),
      plugin("off", { enabled: false }),
      plugin("none", { tab: null }),
      ...Array.from({ length: MAX_PLUGIN_TABS + 1 }, (_, i) => plugin(`x${i}`)),
    ];
    expect(pluginTabs(list).map((p) => p.id)).toEqual(["a", "x0", "x1", "x2", "x3"]);
  });
});

describe("headerTabs", () => {
  const tabs = pluginTabs(["a", "b", "c", "d", "e"].map((id) => plugin(id)));
  const ids = ({ shown, overflow }: ReturnType<typeof headerTabs>) => [shown.map((p) => p.id), overflow.map((p) => p.id)];

  it("fills the header in order and lists the rest in the menu", () => {
    expect(ids(headerTabs(tabs, null))).toEqual([
      ["a", "b", "c"],
      ["d", "e"],
    ]);
    expect(ids(headerTabs(tabs.slice(0, 3), null))).toEqual([["a", "b", "c"], []]);
  });

  it("puts an overflowed active tab in the last slot", () => {
    expect(ids(headerTabs(tabs, "e"))).toEqual([
      ["a", "b", "e"],
      ["c", "d"],
    ]);
    expect(ids(headerTabs(tabs, "b"))).toEqual([
      ["a", "b", "c"],
      ["d", "e"],
    ]);
    expect(ids(headerTabs(tabs, "gone"))).toEqual([
      ["a", "b", "c"],
      ["d", "e"],
    ]);
  });

  it("keeps unpinned tabs in the menu unless they are open", () => {
    const some = tabs.map((p) => (p.id === "a" || p.id === "c" ? { ...p, pinned: false } : p));
    expect(ids(headerTabs(some, null))).toEqual([
      ["b", "d", "e"],
      ["a", "c"],
    ]);
    const few = tabs.slice(0, 3).map((p) => (p.id === "b" ? p : { ...p, pinned: false }));
    expect(ids(headerTabs(few, "c"))).toEqual([["b", "c"], ["a"]]);
  });
});

describe("navEntries", () => {
  const projects = [project("p_1", "Api"), project("p_2", "Shop"), project("p_3", "Web")];

  it("lists the plugin's order first, then the rest, ignoring unknown ids", () => {
    const nav = [
      { project: null, badge: { text: "3 critical", tone: "red" as const } },
      { project: "p_3", badge: { text: "2 critical", tone: "red" as const } },
      { project: "p_gone", badge: null },
      { project: "p_1", badge: { text: "1 high", tone: "amber" as const } },
      { project: "p_3", badge: null },
    ];
    const entries = navEntries(projects, nav);
    expect(entries.map((e) => e.project?.id)).toEqual(["p_3", "p_1", "p_2"]);
    expect(entries[0]!.badge?.text).toBe("2 critical");
    expect(entries[2]!.badge).toBeNull();
    expect(overviewBadge(nav)?.text).toBe("3 critical");
    expect(overviewBadge([])).toBeNull();
  });

  it("filters by name", () => {
    expect(navEntries(projects, [], " sh ").map((e) => e.project?.name)).toEqual(["Shop"]);
    expect(navEntries(projects, [], "zzz")).toEqual([]);
  });
});

describe("page store", () => {
  const state = (receivedAt: number): PageState => ({ page: { blocks: [] }, error: null, receivedAt });
  beforeEach(() => usePageStore.setState({ pages: {} }));

  it("keeps the newest copy per plugin and project", () => {
    const { put } = usePageStore.getState();
    put("cve", "p_1", state(2));
    put("cve", "p_1", state(1));
    put("cve", null, state(5));
    const pages = usePageStore.getState().pages;
    expect(pages[pageKey("cve", "p_1")]!.receivedAt).toBe(2);
    expect(pages[pageKey("cve", null)]!.receivedAt).toBe(5);
    expect(pageKey("cve", null)).not.toBe(pageKey("cve", "p_1"));
  });
});

describe("block vocabulary", () => {
  it("matches the core's block types", () => {
    const start = rust.indexOf("pub enum Block {");
    const body = rust.slice(start, rust.indexOf("\n}", start));
    const names = [...body.matchAll(/^ {4}([A-Z]\w+) \{/gm)].map((m) => m[1]!.toLowerCase());
    expect(names).toEqual(["header", "stats", "section", "list", "table", "text", "keyvalue", "notice", "empty"]);
  });
});
