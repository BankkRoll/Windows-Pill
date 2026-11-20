/**
 * Plugin feed items on the Today tab, inside the card of the plugin that
 * posted them: dismiss, dismiss all, open link and "Fix with an agent".
 */
import { Bot16Regular, Dismiss16Regular, Open16Regular, Play16Filled } from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useShowRun, useTaskRun } from "../hooks/useTaskRun";
import { attempt, openLink, openPluginPage, startAgent } from "../lib/actions";
import { hostOf } from "../lib/format";
import { pluginApi, type FeedItem, type FeedSeverity, type PluginState, type Project, type Task } from "../lib/native";
import { feedEntries, groupFeed, type FeedEntry, type FeedGroup } from "../lib/plugins";
import { usePillStore } from "../store";
import { PluginIcon } from "./PluginIcon";
import { Chip } from "./ui";

/** Rows shown before "Show all". */
const COLLAPSED = 3;

const SEVERITY: Record<FeedSeverity, { label: string; text: string; dot: string }> = {
  critical: { label: "Critical", text: "text-status-red", dot: "bg-status-red" },
  high: { label: "High", text: "text-status-amber", dot: "bg-status-amber" },
  medium: { label: "Medium", text: "text-white/55", dot: "bg-white/45" },
  low: { label: "Low", text: "text-white/45", dot: "bg-white/30" },
  info: { label: "Info", text: "text-white/45", dot: "bg-white/30" },
};

/**
 * One plugin's feed items (CVE watch's advisories, for one) inside that
 * plugin's Today card, most severe first. Items about the same thing in
 * several projects fold into one row. Each can be dismissed for good, opened
 * in the browser, or turned into a task with "Fix with an agent". Follows the
 * header's project filter.
 *
 * @param props.plugin - Plugin whose feed to list.
 * @returns Nothing when the plugin has no items for the current filter.
 */
export function FeedRows({ plugin }: { plugin: PluginState }) {
  const selected = usePillStore((s) => s.selectedProjectId);
  const [expanded, setExpanded] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const entries = useMemo(() => {
    const all = feedEntries([plugin]);
    return selected ? all.filter((e) => e.item.projectId === selected) : all;
  }, [plugin, selected]);
  const groups = useMemo(() => groupFeed(entries), [entries]);

  useEffect(() => {
    if (!confirmClear) return;
    const id = window.setTimeout(() => setConfirmClear(false), 4000);
    return () => window.clearTimeout(id);
  }, [confirmClear]);

  if (entries.length === 0) return null;
  const shown = expanded ? groups : groups.slice(0, COLLAPSED);

  const clear = () => {
    setConfirmClear(false);
    // With a project picked, only what's on screen goes.
    void attempt(() =>
      selected
        ? Promise.all(entries.map(({ item }) => pluginApi.dismissFeedItem(plugin.id, item.id))).then(() => undefined)
        : pluginApi.clearFeed(plugin.id),
    );
  };

  return (
    <div className="flex flex-col gap-1" aria-label={`${plugin.name} alerts`}>
      <ul className="flex flex-col gap-1">
        <AnimatePresence initial={false}>
          {shown.map((group) =>
            group.entries.length === 1 ? (
              <FeedRow key={group.key} entry={group.first} showProject={!selected} />
            ) : (
              <GroupRow key={group.key} group={group} />
            ),
          )}
        </AnimatePresence>
      </ul>
      <footer className="flex items-center gap-2">
        {groups.length > COLLAPSED && (
          <button
            type="button"
            onClick={() => setExpanded((v) => !v)}
            aria-expanded={expanded}
            className="h-6 rounded-md px-1.5 text-[11.5px] text-white/45 hover:bg-white/6 hover:text-white/80"
          >
            {expanded ? "Show less" : `Show all ${groups.length}`}
          </button>
        )}
        <button
          type="button"
          onClick={() => (confirmClear ? clear() : setConfirmClear(true))}
          className={`ml-auto h-6 rounded-md px-2 text-[11.5px] transition-colors ${
            confirmClear ? "bg-status-red/15 text-status-red hover:bg-status-red/22" : "text-white/45 hover:bg-white/8 hover:text-white/80"
          }`}
        >
          {confirmClear ? `Dismiss ${entries.length}?` : "Dismiss all"}
        </button>
      </footer>
    </div>
  );
}

/** The open task "Fix with an agent" made from this item earlier, if any. */
function useSuggestedTask(item: FeedItem): { project: Project | null; task: Task | null } {
  const project = usePillStore((s) => (item.projectId ? (s.snapshot.projects.find((p) => p.id === item.projectId) ?? null) : null));
  const title = item.task?.title;
  const task = useMemo(
    () => (title ? (project?.tasks.find((t) => t.status !== "done" && t.title.trim() === title) ?? null) : null),
    [project, title],
  );
  return { project, task };
}

/**
 * Dismiss and "Fix with an agent" for one feed entry: the fix makes the
 * suggested task in the item's project and offers to start the agent.
 */
function useEntryActions({ plugin, item }: FeedEntry) {
  const { project, task } = useSuggestedTask(item);
  const dismiss = () => void attempt(() => pluginApi.dismissFeedItem(plugin.id, item.id));
  const fix = async () => {
    const made = await attempt(() => pluginApi.feedTask(plugin.id, item.id));
    if (!made || !project) return false;
    usePillStore.getState().showToast({
      message: made.created ? `Added “${made.task.title}” to ${project.name}` : `“${made.task.title}” is already on the board`,
      tone: "info",
      actionLabel: "Start agent",
      undo: () => void startAgent(project, made.task),
    });
    return true;
  };
  return { project, task, canFix: !!item.task && !!project, dismiss, fix };
}

function FeedRow({ entry, showProject }: { entry: FeedEntry; showProject: boolean }) {
  const { plugin, item } = entry;
  const [open, setOpen] = useState(false);
  const severity = SEVERITY[item.severity];
  const { project, task, canFix, dismiss, fix: makeTask } = useEntryActions(entry);
  const fix = async () => {
    if (await makeTask()) setOpen(true);
  };

  return (
    <motion.li
      layout
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, transition: { duration: 0.16 } }}
      className="group relative shrink-0 overflow-hidden rounded-[10px] ring-1 ring-white/[0.06] transition-colors ring-inset hover:bg-white/[0.04]"
    >
      <div className="flex items-start gap-2.5 px-2.5 py-2">
        <span className="grid h-[18px] w-3 shrink-0 place-items-center" aria-hidden>
          <span className={`size-2 rounded-full ${severity.dot}`} />
        </span>
        <div className={`min-w-0 flex-1 ${open ? "" : "group-hover:pr-20 group-focus-within:pr-20"}`}>
          <button
            type="button"
            onClick={() => setOpen((v) => !v)}
            aria-expanded={open}
            className="block w-full min-w-0 text-left outline-none focus-visible:underline"
          >
            <span
              className={`block text-[12.5px] leading-snug text-white/90 ${open ? "" : "truncate"}`}
              title={open ? undefined : item.title}
            >
              {item.title}
            </span>
            {item.detail && <span className="block truncate text-[11px] text-white/45">{item.detail}</span>}
            {open && item.body && <span className="mt-1 block text-[11.5px] leading-relaxed text-white/60 select-text">{item.body}</span>}
          </button>
          <div className="mt-1 flex min-w-0 items-center gap-1.5">
            <span className={`text-[11px] font-medium ${severity.text}`}>{severity.label}</span>
            {showProject && project && <ProjectChip plugin={plugin} project={project} item={item.id} />}
            {task && <Chip tone="accent">On the board</Chip>}
          </div>
        </div>
      </div>

      {!open && (
        <div className="absolute top-1.5 right-1.5 flex items-center gap-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
          {canFix && !task && <HoverButton icon={Bot16Regular} label="Fix with an agent" onClick={() => void fix()} />}
          {item.url && <HoverButton icon={Open16Regular} label={`Open ${hostOf(item.url)}`} onClick={() => openLink(item.url!)} />}
          <HoverButton icon={Dismiss16Regular} label="Dismiss" onClick={dismiss} />
        </div>
      )}

      {open && (
        <div className="flex flex-wrap items-center gap-1.5 px-2.5 pb-2 pl-[32px]">
          {task && project ? (
            <TaskActions project={project} task={task} />
          ) : (
            canFix && (
              <ActionButton primary onClick={() => void fix()}>
                <Bot16Regular className="size-3.5" /> Fix with an agent
              </ActionButton>
            )
          )}
          {item.url && (
            <ActionButton onClick={() => openLink(item.url!)} title={item.url}>
              <Open16Regular className="size-3.5" /> Open {hostOf(item.url)}
            </ActionButton>
          )}
          {item.actions.map((a) => (
            <ActionButton key={a.id} onClick={() => void attempt(() => pluginApi.feedItemAction(plugin.id, item.id, a.id))}>
              {a.icon && <PluginIcon icon={a.icon} className="size-3" />}
              {a.label}
            </ActionButton>
          ))}
          <ActionButton onClick={dismiss}>
            <Dismiss16Regular className="size-3.5" /> Dismiss
          </ActionButton>
        </div>
      )}
    </motion.li>
  );
}

/**
 * One advisory (or any shared item) that hits several projects: one row,
 * with each project's fix and dismissal inside.
 */
function GroupRow({ group }: { group: FeedGroup }) {
  const [open, setOpen] = useState(false);
  const { item } = group.first;
  const severity = SEVERITY[item.severity];
  const count = group.entries.length;
  const dismissAll = () =>
    void attempt(() => Promise.all(group.entries.map((e) => pluginApi.dismissFeedItem(e.plugin.id, e.item.id))).then(() => undefined));

  return (
    <motion.li
      layout
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, transition: { duration: 0.16 } }}
      className="group relative shrink-0 overflow-hidden rounded-[10px] ring-1 ring-white/[0.06] transition-colors ring-inset hover:bg-white/[0.04]"
    >
      <div className="flex items-start gap-2.5 px-2.5 py-2">
        <span className="grid h-[18px] w-3 shrink-0 place-items-center" aria-hidden>
          <span className={`size-2 rounded-full ${severity.dot}`} />
        </span>
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
          className={`min-w-0 flex-1 text-left outline-none focus-visible:underline ${open ? "" : "group-hover:pr-14 group-focus-within:pr-14"}`}
        >
          <span
            className={`block text-[12.5px] leading-snug text-white/90 ${open ? "" : "truncate"}`}
            title={open ? undefined : item.title}
          >
            {item.title}
          </span>
          {open && item.body && <span className="mt-1 block text-[11.5px] leading-relaxed text-white/60 select-text">{item.body}</span>}
          <span className="mt-1 flex min-w-0 items-center gap-1.5">
            <span className={`text-[11px] font-medium ${severity.text}`}>{severity.label}</span>
            <span className="text-[11px] text-white/45">{count} projects</span>
          </span>
        </button>
      </div>

      {!open && (
        <div className="absolute top-1.5 right-1.5 flex items-center gap-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
          {item.url && <HoverButton icon={Open16Regular} label={`Open ${hostOf(item.url)}`} onClick={() => openLink(item.url!)} />}
          <HoverButton icon={Dismiss16Regular} label={`Dismiss in all ${count} projects`} onClick={dismissAll} />
        </div>
      )}

      {open && (
        <div className="flex flex-col gap-1.5 px-2.5 pb-2 pl-[32px]">
          <ul className="flex flex-col divide-y divide-white/[0.06] rounded-lg bg-white/[0.03] ring-1 ring-white/[0.05]">
            {group.entries.map((entry) => (
              <ProjectLine key={entry.item.id} entry={entry} />
            ))}
          </ul>
          <div className="flex flex-wrap items-center gap-1.5">
            {item.url && (
              <ActionButton onClick={() => openLink(item.url!)} title={item.url}>
                <Open16Regular className="size-3.5" /> Open {hostOf(item.url)}
              </ActionButton>
            )}
            <ActionButton onClick={dismissAll}>
              <Dismiss16Regular className="size-3.5" /> Dismiss in all {count}
            </ActionButton>
          </div>
        </div>
      )}
    </motion.li>
  );
}

/** One project inside a grouped row: its versions, its fix, its dismissal. */
function ProjectLine({ entry }: { entry: FeedEntry }) {
  const { project, task, canFix, dismiss, fix } = useEntryActions(entry);
  return (
    <li className="flex items-center gap-2 px-2 py-1.5">
      {project && <ProjectChip plugin={entry.plugin} project={project} item={entry.item.id} />}
      <span className="min-w-0 flex-1 truncate text-[11px] text-white/45" title={entry.item.detail ?? undefined}>
        {entry.item.detail}
      </span>
      {task && project ? (
        <TaskActions project={project} task={task} />
      ) : (
        canFix && (
          <ActionButton onClick={() => void fix()}>
            <Bot16Regular className="size-3.5" /> Fix
          </ActionButton>
        )
      )}
      <button
        type="button"
        title="Dismiss for this project"
        aria-label="Dismiss for this project"
        onClick={dismiss}
        className="grid size-6 shrink-0 place-items-center rounded-md text-white/40 hover:bg-white/8 hover:text-white/80"
      >
        <Dismiss16Regular className="size-3.5" />
      </button>
    </li>
  );
}

/**
 * A project's badge on an alert. Opens that project's page in the plugin's
 * tab when the plugin has one; otherwise focuses Pill on the project.
 */
function ProjectChip({ plugin, project, item }: { plugin: PluginState; project: Project; item: string }) {
  const selectProject = usePillStore((s) => s.selectProject);
  const page = plugin.tab ? `${plugin.tab.title} for ${project.name}` : null;
  return (
    <button
      type="button"
      onClick={() => (plugin.tab ? openPluginPage(plugin.id, project.id, item) : selectProject(project.id))}
      title={page ? `Open ${page}` : `Show only ${project.name}`}
      className="shrink-0 rounded-md outline-none transition-[filter] hover:brightness-125 focus-visible:ring-1 focus-visible:ring-white/40"
    >
      <Chip dot={project.color}>{project.name}</Chip>
    </button>
  );
}

/** Start or follow the agent on the task made from an item, or open the task. */
function TaskActions({ project, task }: { project: Project; task: Task }) {
  const { active, needsYou } = useTaskRun(task.id);
  const showRun = useShowRun();
  const openDetail = usePillStore((s) => s.openDetail);
  return (
    <>
      {active ? (
        <ActionButton onClick={() => showRun(task.id)}>{needsYou ? "Agent needs you" : "Agent working"}</ActionButton>
      ) : (
        <ActionButton primary onClick={() => void startAgent(project, task)}>
          <Play16Filled className="size-3" /> Start agent
        </ActionButton>
      )}
      <ActionButton onClick={() => openDetail(task.id)}>Open task</ActionButton>
    </>
  );
}

function ActionButton({
  primary = false,
  title,
  onClick,
  children,
}: {
  primary?: boolean;
  title?: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className={`inline-flex h-6 items-center gap-1 rounded-md px-2 text-[11.5px] transition-colors ${
        primary ? "bg-accent/18 font-medium text-accent hover:bg-accent/26" : "bg-white/7 text-white/80 hover:bg-white/12 hover:text-white"
      }`}
    >
      {children}
    </button>
  );
}

function HoverButton({ icon: Icon, label, onClick }: { icon: typeof Dismiss16Regular; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className="surface-menu grid size-6 place-items-center rounded-md text-white/60 shadow-sm ring-1 ring-white/10 hover:text-white"
    >
      <Icon className="size-3.5" />
    </button>
  );
}
