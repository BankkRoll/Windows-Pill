import { Checkmark12Filled, Delete16Regular, Play16Filled } from "@fluentui/react-icons";
import { useState } from "react";
import { useShowRun, useTaskRun } from "../hooks/useTaskRun";
import { removeTask, setStatus, startAgent } from "../lib/actions";
import { dueLabel } from "../lib/format";
import type { Project, Task } from "../lib/native";
import { usePillStore } from "../store";
import { Chip, priorityTone, type ChipTone } from "./ui";

const DUE_TONE: Record<string, ChipTone> = { overdue: "red", today: "amber", soon: "accent", later: "neutral" };

const RING: Record<string, string> = {
  1: "border-status-red/80",
  2: "border-status-amber/80",
  3: "border-white/35",
  none: "border-white/30",
};

/** Props for {@link TaskRow}. */
export interface TaskRowProps {
  task: Task;
  project: Project | null;
}

/**
 * One task line: check circle tinted by priority, title (click for details),
 * tags, due chip, and a delete action on hover.
 */
export function TaskRow({ task, project }: TaskRowProps) {
  const [checking, setChecking] = useState(false);
  const openDetail = usePillStore((s) => s.openDetail);
  const { active, needsYou } = useTaskRun(task.id);
  const showRun = useShowRun();
  const due = task.due ? dueLabel(task.due) : null;
  const done = task.status === "done" || checking;

  return (
    <div
      onClick={() => openDetail(task.id)}
      className="group relative flex h-8 cursor-pointer items-center gap-2.5 rounded-lg pr-1.5 pl-2 hover:bg-white/6"
    >
      <button
        type="button"
        aria-label={done ? "Completed" : "Mark complete"}
        onClick={(e) => {
          e.stopPropagation();
          if (task.status === "done") return;
          setChecking(true);
          void setStatus(project, task, "done").finally(() => setChecking(false));
        }}
        className={`grid size-4 shrink-0 place-items-center rounded-full border-[1.5px] transition-colors ${
          done ? "border-status-green bg-status-green" : `${RING[task.priority ?? "none"]} hover:bg-white/10`
        }`}
      >
        {done && <Checkmark12Filled className="size-2.5 text-black" />}
      </button>

      {/* The row handles the click; this button makes it reachable and operable by keyboard. */}
      <button
        type="button"
        className={`-mx-1 min-w-0 flex-1 truncate rounded px-1 text-left text-[13px] outline-none focus-visible:ring-1 focus-visible:ring-white/40 ${done ? "text-white/38 line-through" : "text-white/92"}`}
        title={task.title}
      >
        {task.title}
      </button>

      <div className="flex shrink-0 items-center gap-1">
        {task.tags.slice(0, 2).map((t) => (
          <Chip key={t}>{t}</Chip>
        ))}
        {task.priority === 1 && <Chip tone={priorityTone(1)}>P1</Chip>}
        {due && <Chip tone={DUE_TONE[due.tone]}>{due.text}</Chip>}
        {active ? (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              showRun(task.id);
            }}
          >
            {needsYou ? <Chip tone="amber">Needs you</Chip> : <Chip tone="accent">Agent working</Chip>}
          </button>
        ) : (
          task.status === "doing" && <Chip tone="accent">Doing</Chip>
        )}
        {project && !active && task.status !== "done" && (
          <button
            type="button"
            aria-label="Start agent"
            title="Start agent"
            onClick={(e) => {
              e.stopPropagation();
              void startAgent(project, task);
            }}
            className="grid size-6 place-items-center rounded-md text-white/40 opacity-0 outline-none transition-opacity group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-white/8 hover:text-accent focus-visible:ring-1 focus-visible:ring-white/40"
          >
            <Play16Filled className="size-3.5" />
          </button>
        )}
        <button
          type="button"
          aria-label="Delete task"
          title="Delete"
          onClick={(e) => {
            e.stopPropagation();
            void removeTask(project, task);
          }}
          className="grid size-6 place-items-center rounded-md text-white/40 opacity-0 outline-none transition-opacity group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-white/8 hover:text-status-red focus-visible:ring-1 focus-visible:ring-white/40"
        >
          <Delete16Regular className="size-3.5" />
        </button>
      </div>
    </div>
  );
}
