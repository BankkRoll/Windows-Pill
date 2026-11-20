import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { applyDetachedNav, DetachedWindow } from "./components/DetachedWindow";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { Pill } from "./components/Pill";
import { Tooltips } from "./components/Tooltips";
import "./index.css";
import {
  api,
  bootstrap,
  detachedNav,
  EVENTS,
  isDetached,
  notificationsApi,
  pluginApi,
  subscribe,
  type Approval,
  type Backdrop,
  type ChatUpdate,
  type LogLine,
  type NotificationFeed,
  type PageEvent,
  type PillConfig,
  type PluginState,
  type ProjectGithub,
  type RunRecord,
  type Snapshot,
} from "./lib/native";
import { freshness } from "./lib/freshness";
import { usePageStore } from "./lib/pages";
import { pluginSignals } from "./lib/plugins";
import { useChatStore } from "./chatStore";
import { dueCount, runSignals } from "./lib/tasks";
import { usePillStore } from "./store";
import { startBackgroundChecks } from "./lib/updates";
import { applyPalette, resolvePalette } from "./lib/theme";

// NOTE: The webview's default context menu (Reload, Inspect…) makes no sense
// on an always-on-top overlay; keep it only for development.
if (import.meta.env.PROD) {
  document.addEventListener("contextmenu", (e) => e.preventDefault());
}

const store = usePillStore;
// Events can land while the startup fetches are still in flight; the event
// is the newer state, so a fetch result never overwrites one.
const fresh = freshness();

function refreshTaskSignals() {
  const count = dueCount(store.getState().snapshot);
  store.getState().setSignals("tasks", count ? [{ kind: "due", count }] : []);
}

store.subscribe((s, prev) => {
  if (s.snapshot !== prev.snapshot) refreshTaskSignals();
});
// Due dates roll over at midnight even when no file changes.
window.setInterval(refreshTaskSignals, 60_000);

subscribe<Snapshot>(EVENTS.snapshot, (snapshot) => {
  fresh.touch("snapshot");
  store.getState().setSnapshot(snapshot);
});
subscribe<PillConfig>(EVENTS.config, (config) => {
  fresh.touch("config");
  store.getState().setConfig(config);
});
// Quick-add belongs to the notch; the detached window would only open a
// second input.
if (!isDetached) subscribe<null>(EVENTS.quickAdd, () => store.getState().setQuickAdd(true));
if (isDetached) document.documentElement.classList.add("detached");

/** Mirrors the theme, palette and accent onto <html>, where the CSS tokens hang. */
function applyTheme() {
  const { config, systemLight } = store.getState();
  const palette = resolvePalette(config.appearance, systemLight);
  applyPalette(document.documentElement, { theme: config.theme, palette, glassTint: config.glassTint }, config.accent);
}
store.subscribe((s, prev) => {
  const a = s.config;
  const b = prev.config;
  if (
    a.theme !== b.theme ||
    a.glassTint !== b.glassTint ||
    a.appearance !== b.appearance ||
    a.accent !== b.accent ||
    s.systemLight !== prev.systemLight
  ) {
    applyTheme();
  }
});
subscribe<boolean>(EVENTS.systemAppearance, (light) => {
  fresh.touch("systemLight");
  store.getState().setSystemLight(light);
});

// The notch window lives as long as Pill does; it alone checks in the background.
if (!isDetached) {
  startBackgroundChecks();
  subscribe<boolean>(EVENTS.stepAside, (on) => store.getState().setStepAside(on));
  subscribe<Backdrop | null>(EVENTS.backdrop, (b) => {
    fresh.touch("backdrop");
    store.getState().setBackdrop(b);
  });
  void api
    .getBackdrop()
    .then(fresh.guard<Backdrop | null>("backdrop", (b) => store.getState().setBackdrop(b)))
    .catch(() => {});
}
subscribe<RunRecord[]>(EVENTS.runs, (runs) => {
  fresh.touch("runs");
  store.getState().setRuns(runs);
});
subscribe<Approval[]>(EVENTS.approvals, (list) => {
  fresh.touch("approvals");
  store.getState().setApprovals(list);
});
void api
  .listApprovals()
  .then(fresh.guard<Approval[]>("approvals", (list) => store.getState().setApprovals(list)))
  .catch(() => {});
subscribe<LogLine>(EVENTS.runLog, (line) => store.getState().appendLog(line));
subscribe<ChatUpdate>(EVENTS.chat, (update) => useChatStore.getState().apply(update));
subscribe<Record<string, ProjectGithub>>(EVENTS.github, (github) => {
  fresh.touch("github");
  store.getState().setGithub(github);
});

function applyPlugins(list: PluginState[]) {
  const s = store.getState();
  s.setPlugins(list);
  s.setSignals("plugins", pluginSignals(list));
}
subscribe<PluginState[]>(EVENTS.plugins, (list) => {
  fresh.touch("plugins");
  applyPlugins(list);
});
subscribe<PageEvent>(EVENTS.pluginPage, (e) => usePageStore.getState().put(e.pluginId, e.project, e.state));
void pluginApi
  .states()
  .then(fresh.guard("plugins", applyPlugins))
  .catch(() => {});
subscribe<NotificationFeed | null>(EVENTS.notifications, (feed) => {
  fresh.touch("notifications");
  store.getState().setNotifications(feed);
});
void notificationsApi
  .feed()
  .then(fresh.guard<NotificationFeed | null>("notifications", (feed) => store.getState().setNotifications(feed)))
  .catch(() => {});

function refreshRunSignals() {
  const { runs, seenRuns, approvals } = store.getState();
  const c = runSignals(runs, seenRuns);
  store.getState().setSignals("agents", [
    { kind: "agent", count: c.agent },
    { kind: "failed", count: c.failed },
    { kind: "pr", count: c.pr },
    { kind: "approval", count: c.approval + approvals.length },
  ]);
}
store.subscribe((s, prev) => {
  if (s.runs !== prev.runs || s.seenRuns !== prev.seenRuns || s.approvals !== prev.approvals) refreshRunSignals();
});
window.setInterval(refreshRunSignals, 60_000);

const root = createRoot(document.getElementById("root")!);

const bootLight = fresh.guard<boolean>("systemLight", (light) => store.getState().setSystemLight(light));
const bootConfig = fresh.guard<[PillConfig, string | null]>("config", ([config, error]) => store.getState().setConfig(config, error));
const bootSnapshot = fresh.guard<Snapshot>("snapshot", (snapshot) => store.getState().setSnapshot(snapshot));

bootstrap()
  .then(({ config, configError, configPath, hotkeyError, snapshot, systemLight }) => {
    const s = store.getState();
    bootLight(systemLight);
    s.setConfigPath(configPath);
    bootConfig([config, configError]);
    s.setHotkeyError(hotkeyError);
    bootSnapshot(snapshot);
    applyTheme();
    void api
      .listRuns()
      .then(fresh.guard<RunRecord[]>("runs", (runs) => store.getState().setRuns(runs)))
      .catch(() => {});
    void api
      .githubSnapshot()
      .then(fresh.guard<Record<string, ProjectGithub>>("github", (github) => store.getState().setGithub(github)))
      .catch(() => {});
    if (detachedNav) applyDetachedNav(detachedNav);
  })
  .catch((e: unknown) => console.error("bootstrap failed", e))
  .finally(() =>
    root.render(
      <StrictMode>
        <ErrorBoundary fallback={isDetached ? "view" : "notch"}>{isDetached ? <DetachedWindow /> : <Pill />}</ErrorBoundary>
        <Tooltips />
      </StrictMode>,
    ),
  );
