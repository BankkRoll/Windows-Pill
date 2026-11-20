import {
  Add12Regular,
  ArrowLeft16Regular,
  BranchFork16Regular,
  Code16Regular,
  Copy16Regular,
  Delete16Regular,
  Dismiss12Regular,
  Folder16Regular,
  Open16Regular,
  Pin12Regular,
  Play16Filled,
} from "@fluentui/react-icons";
import { motion } from "framer-motion";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useKeyboardFocus } from "../hooks/useKeyboardFocus";
import { useRunLog } from "../hooks/useRunLog";
import { useShowRun, useTaskRun } from "../hooks/useTaskRun";
import { attempt, compareAgents, copyPrompt, openEditor, openFolder, openLink, removeTask, startAgent } from "../lib/actions";
import { prFor } from "../lib/github";
import { INHERIT_RUN, inheritedChoice } from "../lib/models";
import {
  ACTIVE_RUN_STATES,
  api,
  isDetached,
  STATUSES,
  type Project,
  type RunChoice,
  type Task,
  type TaskPatch,
  type TaskStatus,
} from "../lib/native";
import { locate, refOf } from "../lib/tasks";
import { usePillStore } from "../store";
import { DatePicker, Select, Toggle } from "./controls";
import { PrDetails } from "./GithubBits";
import { ModelPicker } from "./ModelPicker";
import { RunTranscript } from "./RunTranscript";
import { TestSetup } from "./TestSetup";

const STATUS_LABEL: Record<TaskStatus, string> = {
  inbox: "Inbox",
  todo: "To do",
  doing: "Doing",
  review: "Review",
  done: "Done",
};

const STATUS_DOT: Record<TaskStatus, string> = {
  inbox: "color-mix(in srgb, var(--color-white) 35%, transparent)",
  todo: "color-mix(in srgb, var(--color-white) 60%, transparent)",
  doing: "var(--color-accent)",
  review: "var(--color-status-purple)",
  done: "var(--color-status-green)",
};

const PRIORITY_DOT: Record<number, string> = {
  1: "var(--color-status-red)",
  2: "var(--color-status-amber)",
  3: "color-mix(in srgb, var(--color-white) 50%, transparent)",
};

/**
 * The open task as its own page. It replaces the current tab while open, so
 * nothing shows through. Every field saves as soon as it changes (text on
 * blur or Enter), so there is no Save button to forget.
 */
export function TaskDetail() {
  const taskId = usePillStore((s) => s.detailTaskId);
  const snapshot = usePillStore((s) => s.snapshot);
  const close = usePillStore((s) => s.openDetail);
  const located = taskId ? locate(snapshot, taskId) : null;

  useEffect(() => {
    if (taskId && !located) close(null);
  }, [taskId, located, close]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (e.key === "Escape" && !typing && !e.defaultPrevented) close(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);

  if (!located) return null;
  return <TaskPage key={located.task.id} task={located.task} project={located.project} onClose={() => close(null)} />;
}

function TaskPage({ task, project, onClose }: { task: Task; project: Project | null; onClose: () => void }) {
  const projects = usePillStore((s) => s.snapshot.projects);
  const patch = (p: TaskPatch) => void attempt(() => api.updateTask(refOf(project), task.id, p));

  return (
    <motion.div
      initial={{ opacity: 0, x: 10 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.16, ease: [0.2, 0.8, 0.2, 1] }}
      className="flex h-full flex-col"
    >
      <header className="flex h-9 shrink-0 items-center gap-1.5 px-1">
        <IconButton label="Back (Esc)" onClick={onClose}>
          <ArrowLeft16Regular className="size-4" />
        </IconButton>
        <span className="flex min-w-0 items-center gap-1.5 text-[12px] text-white/45">
          {project && <span className="size-1.5 shrink-0 rounded-full" style={{ background: project.color }} />}
          <span className="truncate">{project?.name ?? "Inbox"}</span>
          <span className="text-white/25">/</span>
          <span className="shrink-0">{STATUS_LABEL[task.status]}</span>
        </span>
        <span className="flex-1" />
        {project && (
          <>
            <IconButton label="Open folder" onClick={() => void openFolder(project)}>
              <Folder16Regular className="size-4" />
            </IconButton>
            <IconButton label="Open in editor" onClick={() => void openEditor(project)}>
              <Code16Regular className="size-4" />
            </IconButton>
            <IconButton label="Copy as agent prompt" onClick={() => void copyPrompt(project, task)}>
              <Copy16Regular className="size-4" />
            </IconButton>
          </>
        )}
        <IconButton
          label="Delete task"
          danger
          onClick={() => {
            onClose();
            void removeTask(project, task);
          }}
        >
          <Delete16Regular className="size-4" />
        </IconButton>
      </header>

      <div className="scroll-fade min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        <div className={`mx-auto flex w-full flex-col gap-3 pt-1 ${isDetached ? "max-w-[680px]" : ""}`}>
          <TitleField task={task} onSave={(title) => patch({ title })} />

          <div className="flex flex-wrap items-center gap-1.5">
            <Select
              label="Status"
              value={task.status}
              options={STATUSES.map((s) => ({
                value: s,
                label: STATUS_LABEL[s],
                dot: STATUS_DOT[s],
              }))}
              onChange={(status) => patch({ status })}
            />
            <Select<number>
              label="Priority"
              value={task.priority ?? 0}
              options={[
                { value: 0, label: "No priority" },
                { value: 1, label: "P1 · Urgent", dot: PRIORITY_DOT[1] },
                { value: 2, label: "P2 · High", dot: PRIORITY_DOT[2] },
                { value: 3, label: "P3 · Normal", dot: PRIORITY_DOT[3] },
              ]}
              onChange={(v) => patch({ priority: v === 0 ? null : (v as 1 | 2 | 3) })}
            />
            <DatePicker label="Due date" placeholder="Due date" value={task.due} onChange={(due) => patch({ due })} />
            <Select
              label="Project"
              value={project?.id ?? ""}
              options={[
                { value: "", label: "Inbox" },
                ...projects.map((p) => ({
                  value: p.id,
                  label: p.name,
                  dot: p.color,
                })),
              ]}
              onChange={(id) => void attempt(() => api.moveTask(refOf(project), task.id, id || null))}
            />
            <TagsField task={task} onSave={(tags) => patch({ tags })} />
          </div>

          <NotesField key={task.updatedAt} task={task} onSave={(notes) => patch({ notes })} />

          {task.pinnedNotes.length > 0 && (
            <section className="flex flex-col gap-1">
              <SectionLabel>Pinned from chat</SectionLabel>
              {task.pinnedNotes.map((n, i) => (
                <div key={i} className="group flex items-start gap-2 rounded-lg bg-white/[0.04] px-2.5 py-1.5">
                  <Pin12Regular className="mt-0.5 size-3 shrink-0 text-white/35" />
                  <span className="line-clamp-3 flex-1 text-[12px] whitespace-pre-wrap text-white/70">{n}</span>
                  <button
                    type="button"
                    aria-label="Unpin"
                    onClick={() =>
                      patch({
                        pinnedNotes: task.pinnedNotes.filter((_, j) => j !== i),
                      })
                    }
                    className="opacity-0 group-hover:opacity-100"
                  >
                    <Dismiss12Regular className="size-3 text-white/50 hover:text-white/90" />
                  </button>
                </div>
              ))}
            </section>
          )}

          {project && <GithubCard project={project} task={task} />}

          {project && task.status !== "done" && <TestSetup project={project} />}

          {project ? <AgentCard project={project} task={task} onAutoChange={(agentReady) => patch({ agentReady })} /> : <InboxHint />}

          <TaskRuns taskId={task.id} />
        </div>
      </div>
    </motion.div>
  );
}

function SectionLabel({ children }: { children: string }) {
  return <span className="px-0.5 text-[11px] font-medium text-white/40">{children}</span>;
}

function IconButton({
  label,
  onClick,
  danger,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className={`grid size-7 shrink-0 place-items-center rounded-lg transition-colors ${
        danger ? "text-white/45 hover:bg-status-red/12 hover:text-status-red" : "text-white/50 hover:bg-white/8 hover:text-white/90"
      }`}
    >
      {children}
    </button>
  );
}

function TitleField({ task, onSave }: { task: Task; onSave: (title: string) => void }) {
  const [value, setValue] = useState(task.title);
  const focus = useKeyboardFocus();
  const ref = useAutosize(value);
  // Escape blurs the field, and the blur would otherwise commit the value
  // the reset hasn't replaced yet.
  const cancelled = useRef(false);
  useEffect(() => setValue(task.title), [task.title]);
  const commit = () => {
    const t = value.replace(/\s+/g, " ").trim();
    if (t && t !== task.title) onSave(t);
    else setValue(task.title);
  };
  return (
    <textarea
      ref={ref}
      rows={1}
      value={value}
      onChange={(e) => setValue(e.target.value)}
      {...focus.props}
      onFocus={() => {
        cancelled.current = false;
        focus.props.onFocus();
      }}
      onBlur={() => {
        focus.props.onBlur();
        if (!cancelled.current) commit();
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          e.currentTarget.blur();
        }
        if (e.key === "Escape") {
          e.preventDefault();
          cancelled.current = true;
          setValue(task.title);
          focus.release(e.currentTarget);
        }
      }}
      aria-label="Title"
      spellCheck={false}
      className="w-full resize-none overflow-hidden rounded-lg bg-transparent px-1 py-0.5 font-display text-[19px] leading-snug font-semibold text-white/95 outline-none hover:bg-white/[0.03] focus:bg-white/[0.05]"
    />
  );
}

function NotesField({ task, onSave }: { task: Task; onSave: (notes: string) => void }) {
  const [value, setValue] = useState(task.notes);
  const focus = useKeyboardFocus();
  const ref = useAutosize(value);
  return (
    <textarea
      ref={ref}
      rows={2}
      value={value}
      onChange={(e) => setValue(e.target.value)}
      {...focus.props}
      onBlur={() => {
        focus.props.onBlur();
        if (value !== task.notes) onSave(value);
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          focus.release(e.currentTarget);
        }
      }}
      placeholder="Add details, steps to reproduce or links. Agents read this."
      aria-label="Description"
      spellCheck={false}
      className="max-h-64 w-full resize-none rounded-lg bg-transparent px-1 py-1 text-[13px] leading-relaxed text-white/80 outline-none placeholder:text-white/30 hover:bg-white/[0.03] focus:bg-white/[0.05]"
    />
  );
}

/** Grows a textarea to fit its text, up to its CSS max-height. */
function useAutosize(value: string) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);
  return ref;
}

function TagsField({ task, onSave }: { task: Task; onSave: (tags: string[]) => void }) {
  const [adding, setAdding] = useState(false);
  const [value, setValue] = useState("");
  const focus = useKeyboardFocus();
  // Escape blurs the input, and the blur would otherwise add what was typed.
  const cancelled = useRef(false);

  const commit = () => {
    const added = value
      .split(/[,\s]+/)
      .map((t) => t.replace(/^[#+]/, "").trim().toLowerCase())
      .filter(Boolean);
    const tags = [...new Set([...task.tags, ...added])];
    if (tags.length !== task.tags.length) onSave(tags);
    setValue("");
  };

  return (
    <>
      {task.tags.map((t) => (
        <span key={t} className="group inline-flex h-7 items-center gap-1 rounded-lg bg-white/7 pr-1.5 pl-2.5 text-[12px] text-white/80">
          {t}
          <button
            type="button"
            aria-label={`Remove tag ${t}`}
            onClick={() => onSave(task.tags.filter((x) => x !== t))}
            className="grid size-4 place-items-center rounded text-white/35 hover:bg-white/10 hover:text-white/85"
          >
            <Dismiss12Regular className="size-2.5" />
          </button>
        </span>
      ))}
      {adding ? (
        <input
          autoFocus
          value={value}
          onChange={(e) => setValue(e.target.value)}
          {...focus.props}
          onFocus={() => {
            cancelled.current = false;
            focus.props.onFocus();
          }}
          onBlur={() => {
            focus.props.onBlur();
            if (cancelled.current) setValue("");
            else commit();
            setAdding(false);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === ",") {
              e.preventDefault();
              commit();
            }
            if (e.key === "Escape") {
              e.preventDefault();
              cancelled.current = true;
              e.currentTarget.blur();
            }
          }}
          placeholder="Tag name"
          aria-label="New tag"
          className="h-7 w-24 rounded-lg bg-white/9 px-2.5 text-[12px] text-white/90 outline-none placeholder:text-white/30"
        />
      ) : (
        <button
          type="button"
          onPointerDown={() => focus.props.onPointerDown()}
          onClick={() => setAdding(true)}
          className="inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[12px] text-white/40 hover:bg-white/6 hover:text-white/80"
        >
          <Add12Regular className="size-3" />
          Tag
        </button>
      )}
    </>
  );
}

/** The linked issue and the PR's review and CI state, when there are any. */
function GithubCard({ project, task }: { project: Project; task: Task }) {
  const pr = usePillStore((s) => prFor(s.github, project.id, task.agent.prUrl));
  if (!task.issue && !pr) return null;
  return (
    <section className="flex flex-col divide-y divide-white/[0.06] rounded-xl bg-white/[0.04] ring-1 ring-white/[0.06]">
      {task.issue && (
        <div className="flex items-center gap-2 px-3 py-2">
          <span className="min-w-0 flex-1 text-[12px] text-white/70">
            GitHub issue #{task.issue.number}
            <span className="text-white/40"> · {task.issue.closed ? "Closed" : "Open"}</span>
          </span>
          <button
            type="button"
            onClick={() => openLink(task.issue!.url)}
            className="inline-flex shrink-0 items-center gap-1 text-[11.5px] text-accent hover:underline"
          >
            <Open16Regular className="size-3.5" /> Open on GitHub
          </button>
        </div>
      )}
      {pr && (
        <div className="flex items-start gap-2 px-3 py-2">
          <div className="min-w-0 flex-1">
            <PrDetails pr={pr} />
          </div>
          <button
            type="button"
            onClick={() => openLink(pr.url)}
            className="inline-flex shrink-0 items-center gap-1 text-[11.5px] text-accent hover:underline"
          >
            <Open16Regular className="size-3.5" /> Pull request
          </button>
        </div>
      )}
    </section>
  );
}

function InboxHint() {
  return (
    <p className="rounded-xl bg-white/[0.03] px-3 py-2.5 text-[12px] text-white/45">Move this task into a project to run an agent on it.</p>
  );
}

/** The line under the hands-off toggle: what it does, or why it's waiting. */
function handsOffHint(task: Task, autopilotOn: boolean): string {
  if (task.agentReady && !autopilotOn) return "Autopilot is off in Settings, so nothing starts on its own.";
  if (task.agentReady && task.agent.blocked && task.status !== "done")
    return "Waiting for you. Run it again, or turn this off and on to let autopilot retry.";
  return "An agent starts it, moves the card along and only notifies you when it needs you.";
}

function AgentCard({ project, task, onAutoChange }: { project: Project; task: Task; onAutoChange: (on: boolean) => void }) {
  const { active, run, needsYou } = useTaskRun(task.id);
  const showRun = useShowRun();
  const config = usePillStore((s) => s.config);
  const [plan, setPlan] = useState(project.settings.planMode);
  const [choice, setChoice] = useState<RunChoice>(INHERIT_RUN);
  const provider = choice.provider ?? project.settings.provider;
  // Each provider runs with what's picked for it here, else its defaults.
  const compareChoices: RunChoice[] = (["claude", "codex"] as const).map((p) =>
    p === provider ? { ...choice, provider: p } : { provider: p, model: "", effort: "" },
  );
  const working = active || run?.state === "awaiting_approval";
  // With a synced status the GitHub card above already links the PR.
  const prSynced = usePillStore((s) => prFor(s.github, project.id, task.agent.prUrl) !== null);

  return (
    <section className="flex flex-col gap-2 rounded-xl bg-white/[0.04] p-3 ring-1 ring-white/[0.06]">
      <div className="flex items-center gap-2">
        <span className="text-[12.5px] font-medium text-white/85">Agent</span>
        {task.agent.branch && (
          <span className="inline-flex min-w-0 items-center gap-1 text-[11px] text-white/40">
            <BranchFork16Regular className="size-3 shrink-0" />
            <code className="truncate">{task.agent.branch}</code>
          </span>
        )}
        <span className="flex-1" />
        {task.agent.prUrl && !prSynced && (
          <button
            type="button"
            onClick={() => openLink(task.agent.prUrl!)}
            className="inline-flex shrink-0 items-center gap-1 text-[11.5px] text-accent hover:underline"
          >
            <Open16Regular className="size-3.5" /> Pull request
          </button>
        )}
      </div>

      {working ? (
        <button
          type="button"
          onClick={() => showRun(task.id)}
          className={`flex items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12px] ${
            needsYou ? "bg-status-amber/12 text-status-amber hover:bg-status-amber/18" : "bg-accent/12 text-accent hover:bg-accent/18"
          }`}
        >
          <span className={`size-1.5 shrink-0 animate-pulse rounded-full ${needsYou ? "bg-status-amber" : "bg-accent"}`} />
          <span className="min-w-0 flex-1 truncate">
            {needsYou
              ? "Waiting for your approval"
              : run?.state === "awaiting_approval"
                ? "Plan ready for review"
                : (run?.activity ?? "Working…")}
          </span>
          <span className="shrink-0 font-semibold">Open</span>
        </button>
      ) : task.status === "done" ? (
        <p className="text-[12px] text-white/40">This task is done.</p>
      ) : (
        <div className="flex flex-wrap items-center gap-1.5">
          <ModelPicker
            variant="chip"
            placement="up"
            provider={provider}
            value={choice}
            inherited={inheritedChoice(config, project, provider)}
            onChange={(c) => setChoice({ ...choice, ...c })}
            onProvider={(p) => setChoice({ provider: p, model: "", effort: "" })}
          />
          <label
            className="inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-lg px-2 text-[12px] text-white/60 hover:bg-white/5"
            title="The agent proposes a plan and waits for your approval before editing"
          >
            <Toggle checked={plan} onChange={setPlan} />
            Plan first
          </label>
          <span className="flex-1" />
          <button
            type="button"
            onClick={() => void compareAgents(project, task, compareChoices)}
            title="Runs Claude Code and Codex on this task in separate worktrees. Pick the better result to ship."
            className="inline-flex h-7 items-center rounded-lg px-2.5 text-[12px] text-white/70 hover:bg-white/8 hover:text-white"
          >
            Compare both
          </button>
          <button
            type="button"
            onClick={() => void startAgent(project, task, plan, choice)}
            className="inline-flex h-7 items-center gap-1.5 rounded-lg bg-accent px-3 text-[12px] font-semibold text-on-accent hover:brightness-110"
          >
            <Play16Filled className="size-3" />
            {run ? "Run again" : "Start agent"}
          </button>
        </div>
      )}

      {!working && task.agent.blocked && task.status !== "done" && (
        <p className="rounded-lg bg-status-red/10 px-2.5 py-1.5 text-[12px] whitespace-pre-wrap text-status-red">{task.agent.blocked}</p>
      )}

      <label className="flex cursor-pointer items-start gap-2 border-t border-white/[0.06] pt-2">
        <Toggle checked={task.agentReady} onChange={onAutoChange} />
        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="text-[12px] text-white/80">Hands-off</span>
          <span className="text-[11.5px] text-white/45">{handsOffHint(task, config.autopilot.enabled)}</span>
        </span>
      </label>
    </section>
  );
}

/**
 * Every agent run on the task as a conversation, newest first, with a
 * switcher when the task was run more than once.
 */
function TaskRuns({ taskId }: { taskId: string }) {
  const all = usePillStore((s) => s.runs);
  const runs = useMemo(() => all.filter((r) => r.taskId === taskId), [all, taskId]);
  const [picked, setPicked] = useState<string | null>(null);
  const run = runs.find((r) => r.id === picked) ?? runs[0];
  const lines = useRunLog(run?.id ?? null);
  const selectRun = usePillStore((s) => s.selectRun);
  const openDetail = usePillStore((s) => s.openDetail);
  const setActiveTab = usePillStore((s) => s.setActiveTab);

  if (!run) return null;

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center gap-1">
        <span className="mr-1 text-[12.5px] font-medium text-white/85">Conversation</span>
        {runs.length > 1 &&
          runs.map((r, i) => (
            <button
              key={r.id}
              type="button"
              onClick={() => setPicked(r.id)}
              title={new Date(r.startedAt).toLocaleString()}
              className={`h-6 rounded-md px-2 text-[11px] tabular-nums ${
                r.id === run.id ? "bg-white/10 text-white/85" : "text-white/45 hover:bg-white/6 hover:text-white/75"
              }`}
            >
              Run {runs.length - i}
            </button>
          ))}
        <span className="flex-1" />
        <button
          type="button"
          onClick={() => {
            selectRun(run.id);
            openDetail(null);
            setActiveTab("agents");
          }}
          className="inline-flex h-6 items-center gap-1 rounded-md px-2 text-[11.5px] text-white/50 hover:bg-white/6 hover:text-white/85"
        >
          <Open16Regular className="size-3.5" /> Open in Agents
        </button>
      </div>
      <div className="flex h-80 min-h-0">
        <RunTranscript key={run.id} runId={run.id} lines={lines} active={ACTIVE_RUN_STATES.includes(run.state)} />
      </div>
    </section>
  );
}
