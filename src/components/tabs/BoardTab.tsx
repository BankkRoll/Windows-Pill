import { useEffect, useId, useMemo, useRef, useState, type DragEvent } from "react";
import { attempt, trackIssue } from "../../lib/actions";
import { boardIssues, type IssueItem } from "../../lib/github";
import { api, STATUSES, type Project, type Task, type TaskStatus } from "../../lib/native";
import { dropOrder, refOf } from "../../lib/tasks";
import { usePillStore } from "../../store";
import { IssueCard } from "../GithubBits";
import { TaskCard } from "../TaskCard";

const LABELS: Record<TaskStatus, string> = {
  inbox: "Inbox",
  todo: "To do",
  doing: "Doing",
  review: "Review",
  done: "Done",
};

const EMPTY_HINTS: Record<TaskStatus, string> = {
  inbox: "New tasks land here",
  todo: "Drag tasks here to plan them",
  doing: "Work in progress",
  review: "Agent PRs wait here",
  done: "Finished tasks",
};

/** A card on the board with the project it belongs to (`null` = global Inbox). */
interface Item {
  task: Task;
  project: Project | null;
}

interface DropTarget {
  status: TaskStatus;
  index: number;
}

/** Drag id of an issue card, distinct from any task id. */
const issueKey = (it: IssueItem) => `gh:${it.project.id}#${it.issue.number}`;

/**
 * Kanban board for the selected project, or every project at once. Cards
 * drag between and within columns; dropping a global Inbox card into a
 * project's column moves it into that project.
 *
 * GitHub issues assigned to the user sit at the bottom of the Inbox column
 * under their own heading: they are incoming work that hasn't been planned,
 * which is what the Inbox is for, and a sixth column wouldn't fit the
 * notch. Dragging one into another column adds it as a task there.
 */
export function BoardTab() {
  const snapshot = usePillStore((s) => s.snapshot);
  const selectedId = usePillStore((s) => s.selectedProjectId);
  const hold = usePillStore((s) => s.hold);
  const openDetail = usePillStore((s) => s.openDetail);
  const showToast = usePillStore((s) => s.showToast);
  const [dragId, setDragId] = useState<string | null>(null);
  const [dragIssue, setDragIssue] = useState<IssueItem | null>(null);
  const github = usePillStore((s) => s.github);
  const [target, setTarget] = useState<DropTarget | null>(null);
  const columnRefs = useRef<Partial<Record<TaskStatus, HTMLDivElement | null>>>({});

  const selected = snapshot.projects.find((p) => p.id === selectedId) ?? null;
  const issues = useMemo(() => boardIssues(snapshot.projects, github, selected?.id ?? null), [snapshot.projects, github, selected]);

  const columns = useMemo(() => {
    const items: Item[] = [];
    const projects = selected ? [selected] : snapshot.projects;
    for (const p of projects) for (const t of p.tasks) items.push({ task: t, project: p });
    for (const t of snapshot.inbox) items.push({ task: t, project: null });
    const byStatus = Object.fromEntries(STATUSES.map((s) => [s, [] as Item[]])) as Record<TaskStatus, Item[]>;
    for (const it of items) byStatus[it.task.status].push(it);
    for (const s of STATUSES) byStatus[s].sort((a, b) => a.task.order - b.task.order);
    return byStatus;
  }, [snapshot, selected]);

  const dragged = useMemo(() => {
    if (!dragId) return null;
    for (const s of STATUSES) {
      const hit = columns[s].find((i) => i.task.id === dragId);
      if (hit) return hit;
    }
    return null;
  }, [dragId, columns]);

  const indexAt = (status: TaskStatus, clientY: number) => {
    const el = columnRefs.current[status];
    if (!el) return 0;
    const cards = [...el.querySelectorAll<HTMLElement>("[data-task-id]")].filter((c) => c.dataset.taskId !== dragId);
    const i = cards.findIndex((c) => {
      const r = c.getBoundingClientRect();
      return clientY < r.top + r.height / 2;
    });
    return i === -1 ? cards.length : i;
  };

  const onDragOver = (status: TaskStatus) => (e: DragEvent) => {
    if (!dragId) return;
    // Issues are already in the Inbox; any other column adds them as tasks.
    if (dragIssue && status === "inbox") return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    const index = indexAt(status, e.clientY);
    if (target?.status !== status || target.index !== index) setTarget({ status, index });
  };

  const finish = () => {
    setDragId(null);
    setDragIssue(null);
    setTarget(null);
  };

  const dragHold = `drag${useId()}`;
  useEffect(() => hold(dragHold, dragId !== null), [hold, dragHold, dragId]);
  useEffect(() => () => hold(dragHold, false), [hold, dragHold]);

  // NOTE: A card removed mid-drag (deleted or moved elsewhere, an issue
  // tracked from another window) never gets its dragend, so the drag ends
  // here instead of holding the notch open forever.
  const dragAlive = dragIssue ? issues.some((it) => issueKey(it) === dragId) : dragged !== null;
  useEffect(() => {
    if (dragId !== null && !dragAlive) finish();
  }, [dragId, dragAlive]);

  const onDrop = (status: TaskStatus) => async (e: DragEvent) => {
    e.preventDefault();
    const item = dragged;
    const issue = dragIssue;
    const index = target?.status === status ? target.index : columns[status].length;
    finish();
    if (issue) {
      if (status === "inbox") return;
      const order = dropOrder(
        columns[status].map((i) => i.task),
        index,
      );
      const task = await trackIssue(issue.project, issue.issue, status);
      if (task) await attempt(() => api.updateTask(issue.project.id, task.id, { order }));
      return;
    }
    if (!item) return;
    const rest = columns[status].filter((i) => i.task.id !== item.task.id).map((i) => i.task);
    const order = dropOrder(rest, index);

    if (!item.project && status !== "inbox") {
      // A global Inbox task needs a project before it can leave the Inbox.
      if (!selected) {
        showToast({ message: "Pick a project for this Inbox task first", tone: "info" });
        openDetail(item.task.id);
        return;
      }
      const moved = await attempt(() => api.moveTask(null, item.task.id, selected.id));
      if (moved) await attempt(() => api.updateTask(selected.id, moved.id, { status, order }));
      return;
    }
    if (item.task.status === status && item.task.order === order) return;
    await attempt(() => api.updateTask(refOf(item.project), item.task.id, { status, order }));
  };

  return (
    <div className="flex h-full gap-2 pb-1">
      {STATUSES.map((status) => {
        const items = columns[status];
        // Drop indices count cards other than the one being dragged.
        const rest = dragId ? items.filter((i) => i.task.id !== dragId) : items;
        const slots = new Map(rest.map((it, i) => [it.task.id, i]));
        return (
          <div key={status} className="flex min-w-0 flex-1 basis-0 flex-col">
            <header className="flex items-center justify-between px-1.5 pb-1.5">
              <span className="text-[12px] font-semibold text-white/75">{LABELS[status]}</span>
              <span className="text-[11px] text-white/35 tabular-nums">{items.length}</span>
            </header>
            <div
              ref={(el) => {
                columnRefs.current[status] = el;
              }}
              onDragOver={onDragOver(status)}
              onDragLeave={(e) => {
                if (!e.currentTarget.contains(e.relatedTarget as Node)) setTarget(null);
              }}
              onDrop={(e) => void onDrop(status)(e)}
              className={`scroll-fade flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto rounded-xl p-1 transition-colors ${
                target?.status === status ? "bg-white/4" : ""
              }`}
            >
              {items.map((item) => {
                const slot = slots.get(item.task.id) ?? -1;
                return (
                  <div key={item.task.id} data-task-id={item.task.id} className="relative">
                    {target?.status === status && target.index === slot && <DropLine />}
                    <TaskCard
                      task={item.task}
                      project={item.project}
                      showProject={!selected || !item.project}
                      dragging={dragId === item.task.id}
                      onDragStart={(e) => {
                        e.dataTransfer.effectAllowed = "move";
                        e.dataTransfer.setData("text/plain", item.task.title);
                        setDragId(item.task.id);
                      }}
                      onDragEnd={finish}
                    />
                  </div>
                );
              })}
              {items.length === 0 && target?.status !== status && !(status === "inbox" && issues.length > 0) && (
                <p className="grid h-16 place-items-center rounded-lg border border-dashed border-white/[0.07] px-2 text-center text-[11px] text-white/25">
                  {EMPTY_HINTS[status]}
                </p>
              )}
              {target?.status === status && target.index >= rest.length && (
                <div className="relative h-1">
                  <DropLine />
                </div>
              )}
              {status === "inbox" && issues.length > 0 && (
                <>
                  <p
                    className="flex items-center justify-between px-1.5 pt-1 text-[11px] font-medium text-white/40"
                    title="Open issues assigned to you on GitHub. Drag one to a column to add it, or start an agent on it."
                  >
                    <span>GitHub</span>
                    <span className="tabular-nums text-white/30">{issues.length}</span>
                  </p>
                  {issues.map((it) => (
                    <IssueCard
                      key={issueKey(it)}
                      item={it}
                      showProject={!selected}
                      dragging={dragId === issueKey(it)}
                      onDragStart={(e) => {
                        e.dataTransfer.effectAllowed = "move";
                        e.dataTransfer.setData("text/plain", it.issue.url);
                        setDragId(issueKey(it));
                        setDragIssue(it);
                      }}
                      onDragEnd={finish}
                    />
                  ))}
                </>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function DropLine() {
  return <div className="pointer-events-none absolute -top-1.25 right-1 left-1 h-0.5 rounded-full bg-accent" />;
}
