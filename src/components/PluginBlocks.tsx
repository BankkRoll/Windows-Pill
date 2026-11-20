/**
 * Renders a plugin page's typed blocks with Pill's own components: headers,
 * stat tiles, foldable sections, expandable lists, tables, text, key/value
 * pairs, notices and empty states, plus their buttons.
 *
 * SECURITY: everything here is plugin data. Text renders as text, icons and
 * tones come from fixed maps, and links are `https://` URLs the core checked.
 * Task buttons never carry their notes to the UI; the core looks the button
 * up in its own copy of the page and fences the notes before writing them.
 */
import { Bot16Regular, ChevronRight12Regular, Open16Regular, Play16Filled } from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useShowRun, useTaskRun } from "../hooks/useTaskRun";
import { attempt, openLink, startAgent } from "../lib/actions";
import { hostOf } from "../lib/format";
import {
  pluginApi,
  type FeedSeverity,
  type PageBlock,
  type PageButton,
  type PageChip,
  type PageRow,
  type PluginState,
  type PluginTone,
  type Project,
  type Task,
} from "../lib/native";
import { pageKey, usePageStore } from "../lib/pages";
import { usePillStore } from "../store";
import { PluginIcon } from "./PluginIcon";

/** The page being drawn: whose it is and which project it is about. */
interface PageScope {
  plugin: PluginState;
  /** `null` on the overview page. */
  project: Project | null;
  /** Id of the row to bring into view, when the page was opened from an alert. */
  focus: string | null;
}

/** How long a focused row stays highlighted. */
const FOCUS_MS = 1600;

/** Whether `row` is somewhere inside `blocks`, so its section should unfold. */
function holds(blocks: readonly PageBlock[], row: string): boolean {
  return blocks.some((b) => (b.type === "list" ? b.rows.some((r) => r.id === row) : b.type === "section" && holds(b.blocks, row)));
}

const Scope = createContext<PageScope | null>(null);

function useScope(): PageScope {
  const scope = useContext(Scope);
  if (!scope) throw new Error("PluginBlocks must be rendered with a page scope");
  return scope;
}

// NOTE: colour is kept for meaning only: a dot, a word, a number. Nothing
// gets a tinted tile behind it, so a page full of findings still reads calm.
const TONES: Record<PluginTone, { text: string; dot: string; chip: string; icon: string }> = {
  neutral: { text: "text-white/55", dot: "bg-white/30", chip: "text-white/55 ring-white/12", icon: "text-white/45" },
  accent: { text: "text-accent", dot: "bg-accent", chip: "text-accent ring-accent/30", icon: "text-accent" },
  green: { text: "text-status-green", dot: "bg-status-green", chip: "text-status-green ring-status-green/30", icon: "text-status-green" },
  amber: { text: "text-status-amber", dot: "bg-status-amber", chip: "text-status-amber ring-status-amber/30", icon: "text-status-amber" },
  red: { text: "text-status-red", dot: "bg-status-red", chip: "text-status-red ring-status-red/30", icon: "text-status-red" },
  purple: {
    text: "text-status-purple",
    dot: "bg-status-purple",
    chip: "text-status-purple ring-status-purple/30",
    icon: "text-status-purple",
  },
};

const SEVERITY: Record<FeedSeverity, { label: string; tone: PluginTone }> = {
  critical: { label: "Critical", tone: "red" },
  high: { label: "High", tone: "amber" },
  medium: { label: "Medium", tone: "neutral" },
  low: { label: "Low", tone: "neutral" },
  info: { label: "Info", tone: "neutral" },
};

/**
 * Draws a plugin page.
 *
 * @param props.blocks - The page's blocks, as the core validated them.
 * @param props.plugin - The plugin that sent them.
 * @param props.project - The page's project; `null` for the overview.
 */
export function PluginBlocks({ blocks, plugin, project }: { blocks: readonly PageBlock[]; plugin: PluginState; project: Project | null }) {
  const key = pageKey(plugin.id, project?.id ?? null);
  const focus = usePageStore((s) => (s.focus?.key === key ? s.focus.row : null));
  const scope = useMemo(() => ({ plugin, project, focus }), [plugin, project, focus]);
  return (
    <Scope.Provider value={scope}>
      <div className="flex flex-col gap-4">
        {blocks.map((block, i) => (
          <BlockView key={i} block={block} />
        ))}
      </div>
    </Scope.Provider>
  );
}

function BlockView({ block }: { block: PageBlock }) {
  switch (block.type) {
    case "header":
      return <HeaderBlock block={block} />;
    case "stats":
      return <StatsBlock block={block} />;
    case "section":
      return <SectionBlock block={block} />;
    case "list":
      return (
        <ul className="flex flex-col divide-y divide-white/[0.06] overflow-hidden rounded-xl bg-white/[0.025] ring-1 ring-white/[0.06] ring-inset">
          {block.rows.map((row, i) => (
            <RowView key={row.id ?? `row-${i}`} row={row} />
          ))}
        </ul>
      );
    case "table":
      return <TableBlock block={block} />;
    case "text":
      return (
        <div
          className={`flex flex-col gap-2 px-1 text-[12px] leading-relaxed select-text ${block.muted ? "text-white/45" : "text-white/70"}`}
        >
          {block.text.split("\n\n").map((para, i) => (
            <p key={i} className="whitespace-pre-line">
              {para}
            </p>
          ))}
        </div>
      );
    case "keyvalue":
      return (
        <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 px-1 text-[11.5px]">
          {block.items.map((pair, i) => (
            <div key={i} className="contents">
              <dt className="text-white/45">{pair.label}</dt>
              <dd className="truncate text-white/80 select-text" title={pair.value}>
                {pair.value}
              </dd>
            </div>
          ))}
        </dl>
      );
    case "notice":
      return <NoticeBlock block={block} />;
    case "empty":
      return (
        <div className="flex flex-col items-center gap-1.5 px-6 py-8 text-center">
          <PluginIcon icon={block.icon ?? "info"} className="mb-1 size-5 text-white/30" />
          <p className="text-[13px] font-medium text-white/80">{block.title}</p>
          {block.text && <p className="max-w-80 text-[11.5px] leading-relaxed text-white/45">{block.text}</p>}
          {block.actions.length > 0 && <Buttons buttons={block.actions} item={null} row={null} className="mt-2 justify-center" />}
        </div>
      );
  }
}

function HeaderBlock({ block }: { block: Extract<PageBlock, { type: "header" }> }) {
  return (
    <header className="flex items-start gap-3 px-1 pt-0.5">
      <div className="min-w-0 flex-1">
        <h2 className="truncate font-display text-[16px] leading-tight font-semibold text-white/92">{block.title}</h2>
        {block.subtitle && (
          <p className="mt-1 flex min-w-0 items-center gap-1.5 text-[11.5px] text-white/50">
            {block.tone !== "neutral" && <span aria-hidden className={`size-1.5 shrink-0 rounded-full ${TONES[block.tone].dot}`} />}
            <span className="truncate">{block.subtitle}</span>
          </p>
        )}
      </div>
      {block.actions.length > 0 && <Buttons buttons={block.actions} item={null} row={null} />}
    </header>
  );
}

function StatsBlock({ block }: { block: Extract<PageBlock, { type: "stats" }> }) {
  return (
    <div className="flex divide-x divide-white/[0.06] rounded-xl bg-white/[0.025] py-2.5 ring-1 ring-white/[0.06] ring-inset">
      {block.items.map((stat, i) => (
        <div key={i} className="flex min-w-0 flex-1 flex-col gap-0.5 px-3.5">
          <span className="text-[18px] leading-tight font-semibold whitespace-nowrap text-white/90 tabular-nums">{stat.value}</span>
          <span className="flex min-w-0 items-center gap-1.5 text-[11px] text-white/50">
            {stat.tone !== "neutral" && <span aria-hidden className={`size-1.5 shrink-0 rounded-full ${TONES[stat.tone].dot}`} />}
            <span className="truncate">{stat.label}</span>
          </span>
          {stat.detail && <span className="truncate text-[10.5px] text-white/35">{stat.detail}</span>}
        </div>
      ))}
    </div>
  );
}

function SectionBlock({ block }: { block: Extract<PageBlock, { type: "section" }> }) {
  const { focus } = useScope();
  const [open, setOpen] = useState(!block.collapsed);
  useEffect(() => {
    if (focus && holds(block.blocks, focus)) setOpen(true);
  }, [focus, block.blocks]);
  return (
    <section aria-label={block.title} className="flex flex-col gap-1.5">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="group flex h-6 items-center gap-1.5 rounded-md px-1 text-left text-[10.5px] font-medium tracking-wide uppercase outline-none focus-visible:ring-1 focus-visible:ring-white/40"
      >
        <span className="truncate text-white/45 group-hover:text-white/70">{block.title}</span>
        {block.count !== null && <span className="text-white/30 tabular-nums">{block.count}</span>}
        <ChevronRight12Regular
          className={`size-3 shrink-0 text-white/30 transition-transform duration-150 group-hover:text-white/55 ${open ? "rotate-90" : ""}`}
        />
      </button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ duration: 0.16, ease: "easeOut" }}
            className="overflow-hidden"
          >
            <div className="flex flex-col gap-2">
              {block.blocks.map((child, i) => (
                <BlockView key={i} block={child} />
              ))}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </section>
  );
}

function RowView({ row }: { row: PageRow }) {
  const { focus } = useScope();
  const [open, setOpen] = useState(false);
  const [lit, setLit] = useState(false);
  const ref = useRef<HTMLLIElement>(null);
  const focused = row.id !== null && row.id === focus;
  const severity = row.severity ? SEVERITY[row.severity] : null;
  const tone = severity?.tone ?? row.tone;
  const selectProject = usePillStore((s) => s.selectProject);
  const target = usePillStore((s) => (row.open ? (s.snapshot.projects.find((p) => p.id === row.open) ?? null) : null));
  // A row that opens a project navigates; one with a body folds; the rest keep their buttons in view.
  const folds = !target && row.body !== null;
  const links = row.url
    ? [{ id: "url:link", label: `Open ${hostOf(row.url)}`, icon: null, primary: false, kind: "link", url: row.url } as const]
    : [];
  const buttons: PageButton[] = [...row.actions, ...links];
  // NOTE: a lone button sits beside the title instead of costing the row another line.
  const aside = !folds && buttons.length === 1;

  useEffect(() => {
    if (!focused) return;
    if (folds) setOpen(true);
    setLit(true);
    // NOTE: wait for the enclosing section to finish unfolding, or the scroll
    // lands where the row was before the page grew.
    const scroll = window.setTimeout(() => {
      ref.current?.scrollIntoView({ block: "center", behavior: "smooth" });
      usePageStore.getState().setFocus(null);
    }, 220);
    const fade = window.setTimeout(() => setLit(false), FOCUS_MS);
    return () => {
      window.clearTimeout(scroll);
      window.clearTimeout(fade);
    };
  }, [focused, folds]);

  const summary = (
    <>
      <span className={`block text-[12.5px] leading-snug text-white/90 ${open ? "" : "truncate"}`} title={open ? undefined : row.title}>
        {row.title}
      </span>
      {row.detail && <span className={`block text-[11px] text-white/45 ${open ? "" : "truncate"}`}>{row.detail}</span>}
      {row.chips.length > 0 && (
        <span className="mt-1 flex min-w-0 flex-wrap items-center gap-1">
          {row.chips.map((chip, i) => (
            <ChipView key={i} chip={chip} />
          ))}
        </span>
      )}
    </>
  );

  return (
    <li ref={ref} className={`transition-colors duration-500 ${lit ? "bg-white/[0.07]" : "hover:bg-white/[0.03]"}`}>
      <div className="flex items-start gap-2.5 px-3 py-2.5">
        <span className="grid h-[18px] w-3.5 shrink-0 place-items-center" aria-hidden>
          {severity || !row.icon ? (
            <span className={`size-2 rounded-full ${TONES[tone].dot}`} />
          ) : (
            <PluginIcon icon={row.icon} className={`size-3.5 ${TONES[tone].icon}`} />
          )}
        </span>
        {target ? (
          <button
            type="button"
            onClick={() => selectProject(target.id)}
            title={`Show ${target.name}`}
            className="min-w-0 flex-1 rounded-sm text-left outline-none focus-visible:ring-1 focus-visible:ring-white/40"
          >
            {summary}
          </button>
        ) : folds ? (
          <button
            type="button"
            onClick={() => setOpen((v) => !v)}
            aria-expanded={open}
            className="min-w-0 flex-1 rounded-sm text-left outline-none focus-visible:ring-1 focus-visible:ring-white/40"
          >
            {summary}
          </button>
        ) : (
          <div className="min-w-0 flex-1">{summary}</div>
        )}
        {(severity || aside) && (
          <span className="flex shrink-0 flex-col items-end gap-1.5">
            {severity && <span className={`mt-px text-[11px] font-medium ${TONES[severity.tone].text}`}>{severity.label}</span>}
            {aside && <Buttons buttons={buttons} item={row.id} row={row} className="justify-end" />}
          </span>
        )}
        {(folds || target) && (
          <ChevronRight12Regular
            aria-hidden
            className={`mt-1 size-3 shrink-0 text-white/30 transition-transform duration-150 ${open ? "rotate-90" : ""}`}
          />
        )}
      </div>
      <AnimatePresence initial={false}>
        {folds && open && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ duration: 0.16, ease: "easeOut" }}
            className="overflow-hidden"
          >
            <div className="flex flex-col gap-2 px-3 pb-3 pl-[36px]">
              <div className="flex flex-col gap-1.5 text-[11.5px] leading-relaxed text-white/60 select-text">
                {row.body!.split("\n\n").map((para, i) => (
                  <p key={i} className="whitespace-pre-line">
                    {para}
                  </p>
                ))}
              </div>
              {buttons.length > 0 && <Buttons buttons={buttons} item={row.id} row={row} />}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      {!folds && !aside && buttons.length > 0 && <Buttons buttons={buttons} item={row.id} row={row} className="px-3 pb-2.5 pl-[36px]" />}
    </li>
  );
}

function TableBlock({ block }: { block: Extract<PageBlock, { type: "table" }> }) {
  const align = (i: number) => (block.columns[i]?.align === "end" ? "text-right tabular-nums" : "text-left");
  return (
    <div className="overflow-x-auto rounded-xl bg-white/[0.025] ring-1 ring-white/[0.06] ring-inset">
      <table className="w-full border-collapse text-[11.5px]">
        <thead>
          <tr>
            {block.columns.map((c, i) => (
              <th
                key={i}
                scope="col"
                className={`px-3 py-2 text-[10.5px] font-medium tracking-wide whitespace-nowrap text-white/40 uppercase ${align(i)}`}
              >
                {c.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {block.rows.map((cells, r) => (
            <tr key={r} className="border-t border-white/[0.05]">
              {cells.map((cell, i) => (
                <td key={i} className={`max-w-56 truncate px-3 py-1.5 text-white/75 select-text ${align(i)}`} title={cell}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function NoticeBlock({ block }: { block: Extract<PageBlock, { type: "notice" }> }) {
  const tone = TONES[block.tone];
  return (
    <div className="flex items-start gap-2.5 rounded-xl bg-white/[0.035] px-3 py-2.5 ring-1 ring-white/[0.06] ring-inset" role="note">
      <PluginIcon icon={block.icon ?? "info"} className={`mt-px size-4 shrink-0 ${tone.icon}`} />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        {block.title && <p className="text-[12.5px] font-medium text-white/85">{block.title}</p>}
        <p className="text-[11.5px] leading-relaxed text-white/65 select-text">{block.text}</p>
        {block.actions.length > 0 && <Buttons buttons={block.actions} item={null} row={null} className="mt-1" />}
      </div>
    </div>
  );
}

function ChipView({ chip }: { chip: PageChip }) {
  return (
    <span
      className={`inline-flex h-[18px] max-w-44 shrink-0 items-center rounded-[5px] px-1.5 text-[10.5px] leading-none whitespace-nowrap ring-1 ring-inset ${TONES[chip.tone].chip}`}
    >
      <span className="truncate">{chip.text}</span>
    </span>
  );
}

// -----------------------------------------------------------------------------
// Buttons
// -----------------------------------------------------------------------------

function Buttons({
  buttons,
  item,
  row,
  className = "",
}: {
  buttons: readonly PageButton[];
  item: string | null;
  row: PageRow | null;
  className?: string;
}) {
  return (
    <div className={`flex flex-wrap items-center gap-1.5 ${className}`}>
      {buttons.map((b) => (
        <PageButtonView key={b.id} button={b} item={item} row={row} />
      ))}
    </div>
  );
}

function PageButtonView({ button, item, row }: { button: PageButton; item: string | null; row: PageRow | null }) {
  const { plugin, project } = useScope();
  const [busy, setBusy] = useState(false);
  const icon = button.icon ? <PluginIcon icon={button.icon} className="size-3" /> : null;
  // NOTE: a list repeats its row buttons down the page; only page-level buttons get the accent.
  const primary = button.primary && row === null;
  if (button.kind === "task") return project ? <TaskButton button={button} item={item} project={project} primary={primary} /> : null;
  if (button.kind === "link") {
    return (
      <ActionButton primary={primary} title={button.url} onClick={() => openLink(button.url)}>
        {icon ?? <Open16Regular className="size-3.5" />} {button.label}
      </ActionButton>
    );
  }
  const run = async () => {
    setBusy(true);
    await attempt(() => pluginApi.pageAction(plugin.id, project?.id ?? null, item, button.id));
    setBusy(false);
  };
  return (
    <ActionButton primary={primary} disabled={busy} title={row ? `${button.label}: ${row.title}` : undefined} onClick={() => void run()}>
      {icon}
      {button.label}
    </ActionButton>
  );
}

/**
 * A task button: adds the suggested task to the page's project and offers
 * to start an agent; once the task is on the board, follows it instead.
 */
function TaskButton({
  button,
  item,
  project,
  primary,
}: {
  button: Extract<PageButton, { kind: "task" }>;
  item: string | null;
  project: Project;
  primary: boolean;
}) {
  const { plugin } = useScope();
  const title = button.task.title;
  const task = usePillStore(
    (s) => s.snapshot.projects.find((p) => p.id === project.id)?.tasks.find((t) => t.status !== "done" && t.title.trim() === title) ?? null,
  );
  const [busy, setBusy] = useState(false);
  if (task) {
    return (
      <>
        <span className="inline-flex h-6 items-center rounded-md bg-accent/12 px-2 text-[11.5px] font-medium text-accent">
          On the board
        </span>
        <TaskActions project={project} task={task} />
      </>
    );
  }
  const add = async () => {
    setBusy(true);
    const made = await attempt(() => pluginApi.pageTask(plugin.id, project.id, item, button.id));
    setBusy(false);
    if (!made) return;
    usePillStore.getState().showToast({
      message: made.created ? `Added “${made.task.title}” to ${project.name}` : `“${made.task.title}” is already on the board`,
      tone: "info",
      actionLabel: "Start agent",
      undo: () => void startAgent(project, made.task),
    });
  };
  return (
    <ActionButton primary={primary} disabled={busy} title={title} onClick={() => void add()}>
      {button.icon ? <PluginIcon icon={button.icon} className="size-3" /> : <Bot16Regular className="size-3.5" />}
      {button.label}
    </ActionButton>
  );
}

/** Start or follow the agent on a task a page suggested, or open the task. */
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
  disabled = false,
  title,
  onClick,
  children,
}: {
  primary?: boolean;
  disabled?: boolean;
  title?: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      disabled={disabled}
      onClick={onClick}
      className={`inline-flex h-6 items-center gap-1 rounded-md px-2 text-[11.5px] whitespace-nowrap outline-none transition-colors focus-visible:ring-1 focus-visible:ring-white/40 disabled:opacity-50 ${
        primary ? "bg-accent/18 font-medium text-accent hover:bg-accent/26" : "bg-white/7 text-white/80 hover:bg-white/12 hover:text-white"
      }`}
    >
      {children}
    </button>
  );
}

/**
 * A plugin-supplied badge as muted text behind a tone dot, for sidebar entries.
 *
 * @param props.chip - Text and tone.
 */
export function PageChipView({ chip }: { chip: PageChip }) {
  return (
    <span className="inline-flex min-w-0 shrink-0 items-center gap-1.5 text-[11px] text-white/45 tabular-nums">
      {chip.tone !== "neutral" && <span aria-hidden className={`size-1.5 shrink-0 rounded-full ${TONES[chip.tone].dot}`} />}
      <span className="truncate">{chip.text}</span>
    </span>
  );
}
