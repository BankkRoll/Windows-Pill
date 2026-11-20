import { Bot12Regular, Play16Filled } from "@fluentui/react-icons";
import type { DragEvent } from "react";
import { useShowRun, useTaskRun } from "../hooks/useTaskRun";
import { startAgent } from "../lib/actions";
import { dueLabel } from "../lib/format";
import { prFor } from "../lib/github";
import type { Project, Task } from "../lib/native";
import { waitingLabel } from "../lib/tasks";
import { usePillStore } from "../store";
import { PrIndicator } from "./GithubBits";
import { Chip, priorityTone, type ChipTone } from "./ui";

const DUE_TONE: Record<string, ChipTone> = { overdue: "red", today: "amber", soon: "accent", later: "neutral" };

/** Props for {@link TaskCard}. */
export interface TaskCardProps {
  task: Task;
  project: Project | null;
  /** Show the project name (All projects view). */
  showProject: boolean;
  dragging: boolean;
  onDragStart: (e: DragEvent<HTMLDivElement>) => void;
  onDragEnd: () => void;
}

/**
 * A board card: two-line title, chips, and a start-agent button on hover.
 * Click opens the detail sheet (which has the other actions); drag moves it.
 */
export function TaskCard({ task, project, showProject, dragging, onDragStart, onDragEnd }: TaskCardProps) {
  const openDetail = usePillStore((s) => s.openDetail);
  const { run, active, needsYou } = useTaskRun(task.id);
  const showRun = useShowRun();
  const due = task.due && task.status !== "done" ? dueLabel(task.due) : null;
  const pr = usePillStore((s) => prFor(s.github, project?.id ?? null, task.agent.prUrl));
  const issueClosed = task.issue?.closed && task.status !== "done";
  const waiting = waitingLabel(run);
  const blocked = !active && task.status !== "done" ? task.agent.blocked : null;

  return (
    <div
      draggable
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onClick={() => openDetail(task.id)}
      className={`group relative cursor-pointer rounded-[10px] bg-white/6 px-2.5 py-2 transition-[background-color,opacity] hover:bg-white/9 has-[:focus-visible]:bg-white/9 has-[:focus-visible]:ring-1 has-[:focus-visible]:ring-white/30 ${
        dragging ? "opacity-35" : ""
      }`}
    >
      <div>
        {/* The card handles the click; this button makes it reachable and operable by keyboard. */}
        <button type="button" className="block w-full text-left outline-none">
          <span
            className={`line-clamp-2 text-[12.5px] leading-snug ${task.status === "done" ? "text-white/40 line-through" : "text-white/90"}`}
          >
            {task.title}
          </span>
        </button>
        {project && !active && task.status !== "done" && (
          <button
            type="button"
            title="Start agent"
            aria-label="Start agent"
            onClick={(e) => {
              e.stopPropagation();
              void startAgent(project, task);
            }}
            className="surface-menu absolute top-1.5 right-1.5 grid size-6 place-items-center rounded-md text-white/70 opacity-0 shadow-sm ring-1 ring-white/10 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100 hover:text-white focus-visible:opacity-100"
          >
            <Play16Filled className="size-3" />
          </button>
        )}
      </div>
      {(showProject || task.priority || due || task.tags.length > 0 || task.agentReady || active || waiting || task.issue || pr) && (
        <div className="mt-1.5 flex flex-wrap items-center gap-1">
          {showProject && (
            <span className="mr-0.5 inline-flex max-w-24 items-center gap-1 text-[11px] text-white/45">
              {project && <span className="size-1.5 shrink-0 rounded-full" style={{ background: project.color }} />}
              <span className="truncate">{project?.name ?? "Inbox"}</span>
            </span>
          )}
          {task.issue && (
            <span title={task.issue.closed ? "Issue closed on GitHub" : "GitHub issue"}>
              <Chip>#{task.issue.number}</Chip>
            </span>
          )}
          {task.agentReady && (
            <span title="Hands-off: an agent takes this from start to finish" className="inline-flex text-white/40">
              <Bot12Regular className="size-3" aria-label="Hands-off" />
            </span>
          )}
          {task.priority && <Chip tone={priorityTone(task.priority)}>P{task.priority}</Chip>}
          {due && <Chip tone={DUE_TONE[due.tone]}>{due.text}</Chip>}
          {(active || waiting) && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                showRun(task.id);
              }}
            >
              {needsYou ? (
                <Chip tone="amber">Needs you</Chip>
              ) : waiting ? (
                <Chip tone="amber">{waiting}</Chip>
              ) : (
                <Chip tone="accent">Agent working</Chip>
              )}
            </button>
          )}
          {pr && <PrIndicator pr={pr} />}
          {issueClosed && <span className="text-[11px] text-white/45">Issue closed</span>}
          {task.tags.slice(0, 2).map((t) => (
            <Chip key={t}>{t}</Chip>
          ))}
        </div>
      )}
      {blocked && (
        <p className="mt-1 line-clamp-1 text-[11px] text-status-red/85" title={blocked}>
          {blocked}
        </p>
      )}
    </div>
  );
}
