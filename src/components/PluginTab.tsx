/**
 * A plugin's header tab: a full page with Pill's project sidebar on the left
 * ("All projects", then each project with the plugin's badge) and the
 * plugin's page for the selected entry on the right.
 *
 * The sidebar and the header's project picker are one selection: picking a
 * project in either shows that project everywhere, so the pop-out window,
 * quick-add and the other tabs follow along. Plugins without the
 * `projects` capability have only an overview page and no sidebar.
 *
 * While a page is on screen the core tells the plugin (`view`), and the
 * plugin answers with the page and may push updates; the last copy of each
 * page is cached, so reopening shows it at once. The Chat tab's project
 * sidebar shows the same project pages through {@link PluginPageView}.
 */
import { ArrowClockwise16Regular, Search12Regular } from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useKeyboardFocus } from "../hooks/useKeyboardFocus";
import { attempt } from "../lib/actions";
import { pluginApi, type PageChip, type PluginState, type Project } from "../lib/native";
import { hasProjectPages, navEntries, overviewBadge, pageKey, usePageStore } from "../lib/pages";
import { usePillStore } from "../store";
import { PageChipView, PluginBlocks } from "./PluginBlocks";
import { PluginIcon } from "./PluginIcon";

/** Above this many projects the sidebar gets a search field. */
const SEARCH_FROM = 8;
/** How long to wait for a first page before saying the plugin is slow. */
const SLOW_MS = 12_000;

/**
 * The tab of plugin `pluginId`; renders nothing if the plugin went away.
 *
 * @param props.pluginId - Plugin id.
 */
export function PluginTab({ pluginId }: { pluginId: string }) {
  const plugin = usePillStore((s) => s.plugins.find((p) => p.id === pluginId && p.enabled && p.tab) ?? null);
  const selectedId = usePillStore((s) => s.selectedProjectId);
  const project = usePillStore((s) => s.snapshot.projects.find((p) => p.id === s.selectedProjectId) ?? null);
  if (!plugin) return null;
  const perProject = hasProjectPages(plugin);

  return (
    <div className="flex h-full min-h-0 gap-2 pb-1">
      {perProject && <Sidebar plugin={plugin} selectedId={selectedId} />}
      <div className="scroll-fade min-h-0 min-w-0 flex-1 overflow-y-auto px-1 pt-1 pb-3">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={perProject ? (project?.id ?? "*") : "*"}
            initial={{ opacity: 0, y: 3 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.12 }}
          >
            <PluginPageView plugin={plugin} project={perProject ? project : null} />
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}

/** Whether the plugin's process is up, so it can answer. */
function isRunning(plugin: PluginState): boolean {
  return plugin.status.state === "running" || plugin.status.state === "starting";
}

/**
 * One of a plugin's pages: asks for it while it is on screen (`view`) and
 * shows it from the shared cache, or an honest loading, error or stopped
 * state. Used by the header tab and by each project's Chat sidebar.
 *
 * @param props.plugin - A plugin with a tab.
 * @param props.project - The page's project; `null` for the overview.
 */
export function PluginPageView({ plugin, project }: { plugin: PluginState; project: Project | null }) {
  const projectId = project?.id ?? null;
  const running = isRunning(plugin);
  const state = usePageStore((s) => s.pages[pageKey(plugin.id, projectId)]);
  const put = usePageStore((s) => s.put);
  const [slow, setSlow] = useState(false);

  useEffect(() => {
    if (!running) return;
    let alive = true;
    void attempt(() => pluginApi.openView(plugin.id, projectId)).then((cached) => {
      if (alive && cached) put(plugin.id, projectId, cached);
    });
    return () => {
      alive = false;
      void pluginApi.closeView(plugin.id).catch(() => {});
    };
  }, [plugin.id, projectId, running, put]);

  useEffect(() => {
    setSlow(false);
    if (state || !running) return;
    const id = window.setTimeout(() => setSlow(true), SLOW_MS);
    return () => window.clearTimeout(id);
  }, [state, running, plugin.id, projectId]);

  if (!running) {
    const stopped = plugin.status.state === "error" || plugin.status.state === "changed" || plugin.status.state === "invalid";
    return (
      <Placeholder
        icon="pause"
        title={stopped ? `${plugin.name} stopped` : `${plugin.name} isn't running`}
        text={plugin.status.message ?? "Turn it on in Settings → Plugins."}
        action={
          plugin.status.fix?.kind === "restart"
            ? { label: "Restart", run: () => void attempt(() => pluginApi.restart(plugin.id)) }
            : undefined
        }
      />
    );
  }
  if (state?.page) return <PluginBlocks blocks={state.page.blocks} plugin={plugin} project={project} />;
  if (state?.error) return <Placeholder icon="warning" title={`${plugin.name} couldn't show this page`} text={state.error} />;
  return slow ? (
    <Placeholder
      icon="clock"
      title={`No answer from ${plugin.name} yet`}
      text="It may still be busy. The page appears here as soon as it arrives."
    />
  ) : (
    <Loading />
  );
}

function Loading() {
  return (
    <div aria-busy className="flex flex-col gap-3" aria-label="Loading">
      <span className="flex flex-col gap-1.5 px-1 pt-0.5">
        <span className="h-3.5 w-40 animate-pulse rounded bg-white/8" />
        <span className="h-2.5 w-56 animate-pulse rounded bg-white/6" />
      </span>
      <span className="h-[62px] animate-pulse rounded-xl bg-white/[0.035]" />
      <span className="h-36 animate-pulse rounded-xl bg-white/[0.03]" />
    </div>
  );
}

function Placeholder({
  icon,
  title,
  text,
  action,
}: {
  icon: "pause" | "warning" | "clock";
  title: string;
  text: string;
  action?: { label: string; run: () => void };
}) {
  return (
    <div className="flex flex-col items-center gap-1.5 px-6 py-12 text-center">
      <PluginIcon icon={icon} className="mb-1 size-5 text-white/30" />
      <p className="text-[13px] font-medium text-white/80">{title}</p>
      <p className="max-w-80 text-[11.5px] leading-relaxed text-white/45 select-text">{text}</p>
      {action && (
        <button
          type="button"
          onClick={action.run}
          className="mt-2 inline-flex h-7 items-center gap-1.5 rounded-lg bg-white/8 px-2.5 text-[12px] text-white/80 hover:bg-white/12"
        >
          <ArrowClockwise16Regular className="size-3.5" /> {action.label}
        </button>
      )}
    </div>
  );
}

/**
 * "All projects" and every project, in the plugin's order with its badges.
 * Arrow keys move between entries.
 */
function Sidebar({ plugin, selectedId }: { plugin: PluginState; selectedId: string | null }) {
  const projects = usePillStore((s) => s.snapshot.projects);
  const select = usePillStore((s) => s.selectProject);
  const [query, setQuery] = useState("");
  const focus = useKeyboardFocus();
  const list = useRef<HTMLUListElement>(null);
  const entries = useMemo(() => navEntries(projects, plugin.nav, query), [projects, plugin.nav, query]);

  const onKeyDown = (e: KeyboardEvent<HTMLUListElement>) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Home" && e.key !== "End") return;
    const items = [...(list.current?.querySelectorAll<HTMLButtonElement>("button[data-nav]") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : at + (e.key === "ArrowDown" ? 1 : -1);
    items[Math.max(0, Math.min(items.length - 1, next))]?.focus();
    e.preventDefault();
  };

  return (
    <nav aria-label={`${plugin.tab?.title ?? plugin.name} projects`} className="flex w-44 shrink-0 flex-col gap-1 pt-1 pb-2">
      {projects.length > SEARCH_FROM && (
        <label className="flex h-7 shrink-0 items-center gap-1.5 rounded-lg bg-white/[0.06] px-2">
          <Search12Regular className="size-3 shrink-0 text-white/40" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            {...focus.props}
            onKeyDown={(e) => {
              if (e.key === "Enter" && entries[0]?.project) select(entries[0].project.id);
              if (e.key === "Escape") {
                setQuery("");
                focus.release(e.currentTarget);
              }
            }}
            placeholder="Filter projects"
            aria-label="Filter projects"
            spellCheck={false}
            className="min-w-0 flex-1 bg-transparent text-[12px] text-white/90 outline-none placeholder:text-white/35"
          />
        </label>
      )}
      <ul ref={list} onKeyDown={onKeyDown} className="scroll-fade flex min-h-0 flex-1 flex-col gap-px overflow-y-auto">
        {!query && (
          <NavButton label="All projects" badge={overviewBadge(plugin.nav)} active={selectedId === null} onClick={() => select(null)} />
        )}
        {!query && entries.length > 0 && <li aria-hidden className="mx-2 my-1 h-px shrink-0 bg-white/8" />}
        {entries.map(({ project, badge }) =>
          project ? (
            <NavButton
              key={project.id}
              label={project.name}
              color={project.color}
              badge={badge}
              active={project.id === selectedId}
              onClick={() => select(project.id)}
            />
          ) : null,
        )}
        {query && entries.length === 0 && <li className="px-2 py-1.5 text-[11.5px] text-white/40">No project matches</li>}
      </ul>
    </nav>
  );
}

function NavButton({
  label,
  color,
  badge,
  active,
  onClick,
}: {
  label: string;
  color?: string;
  badge: PageChip | null;
  active: boolean;
  onClick: () => void;
}) {
  const ref = useRef<HTMLLIElement>(null);
  // NOTE: the project can be picked from elsewhere (an alert's badge, the
  // header picker), so bring it into view rather than leave it scrolled off.
  useEffect(() => {
    if (active) ref.current?.scrollIntoView({ block: "nearest" });
  }, [active]);
  return (
    <li ref={ref} className="shrink-0">
      <button
        type="button"
        data-nav
        aria-current={active ? "page" : undefined}
        onClick={onClick}
        className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[12px] outline-none transition-colors focus-visible:ring-1 focus-visible:ring-white/40 ${
          active ? "bg-white/10 text-white/92" : "text-white/70 hover:bg-white/6 hover:text-white/85"
        }`}
      >
        <span
          aria-hidden
          className="size-2 shrink-0 rounded-full"
          style={{ background: color ?? "conic-gradient(#2F7BFF, #30D158, #FFB340, #FF453A, #2F7BFF)" }}
        />
        <span className={`min-w-0 flex-1 truncate ${active ? "font-medium" : ""}`}>{label}</span>
        {badge && <PageChipView chip={badge} />}
      </button>
    </li>
  );
}
