import { FolderAdd16Regular } from "@fluentui/react-icons";
import { Reorder } from "framer-motion";
import { useEffect, useId, useMemo, useState } from "react";
import { useClock } from "../../hooks/useClock";
import { attempt } from "../../lib/actions";
import { api, type Task } from "../../lib/native";
import { refOf, reorderSlots, todayGroups, type TodayGroup } from "../../lib/tasks";
import { usePillStore } from "../../store";
import { PluginWidgets } from "../PluginWidgets";
import { TaskRow } from "../TaskRow";
import { Kbd } from "../ui";

/**
 * Plugin cards on top (Pill's own, then third-party ones with their
 * alerts), then everything due today (or overdue) plus everything in Doing,
 * across all projects, grouped by project. Rows can be dragged to reorder.
 */
export function TodayTab() {
  // NOTE: the panel scrolls this whole column, so it grows with the cards;
  // a fixed height let the empty state centre itself over them.
  return (
    <div className="flex min-h-full flex-col">
      <PluginWidgets />
      <div className="flex flex-1 flex-col">
        <TodayTasks />
      </div>
    </div>
  );
}

function TodayTasks() {
  const snapshot = usePillStore((s) => s.snapshot);
  const selected = usePillStore((s) => s.selectedProjectId);
  const config = usePillStore((s) => s.config);
  const setTab = usePillStore((s) => s.setActiveTab);
  const now = useClock();

  const groups = useMemo(() => {
    const all = todayGroups(snapshot, now);
    return selected ? all.filter((g) => g.project?.id === selected) : all;
  }, [snapshot, now, selected]);

  if (config.roots.length === 0 && config.projects.length === 0) {
    return (
      <Empty title="Add your projects">
        <p>Pick a folder (or a whole drive) and every git repo inside becomes a project.</p>
        <button
          type="button"
          onClick={() => setTab("settings")}
          className="mt-3 inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-[12px] font-semibold text-on-accent"
        >
          <FolderAdd16Regular className="size-4" /> Choose folders
        </button>
      </Empty>
    );
  }

  if (groups.length === 0) {
    return (
      <Empty title="Nothing due today">
        <p>
          Tasks due today and everything in Doing show up here. Press <Kbd>{config.hotkey}</Kbd> anywhere to add one.
        </p>
      </Empty>
    );
  }

  return (
    <div className="flex flex-col gap-3 py-1">
      {groups.map((g) => (
        <Group key={g.project?.id ?? "inbox"} group={g} />
      ))}
    </div>
  );
}

function Group({ group }: { group: TodayGroup }) {
  const [items, setItems] = useState<Task[]>(group.tasks);
  const hold = usePillStore((s) => s.hold);
  const [dragging, setDragging] = useState<string | null>(null);
  const key = group.tasks.map((t) => `${t.id}:${t.order}:${t.updatedAt}`).join("|");

  // Re-sync with the snapshot whenever the underlying tasks change.
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` captures the relevant changes
  useEffect(() => setItems(group.tasks), [key]);

  const dragHold = `drag${useId()}`;
  useEffect(() => hold(dragHold, dragging !== null), [hold, dragHold, dragging]);
  useEffect(() => () => hold(dragHold, false), [hold, dragHold]);

  // NOTE: A row removed mid-drag (completed or deleted elsewhere) never gets
  // its drag end, so the drag ends here instead of holding the notch open.
  const draggedGone = dragging !== null && !group.tasks.some((t) => t.id === dragging);
  useEffect(() => {
    if (draggedGone) setDragging(null);
  }, [draggedGone]);

  const persist = async () => {
    setDragging(null);
    const updates = reorderSlots(group.tasks, items);
    for (const u of updates) {
      await attempt(() => api.updateTask(refOf(group.project), u.id, { order: u.order }));
    }
  };

  return (
    <section>
      <header className="flex items-center gap-2 px-2 pb-1 text-[12px] text-white/60">
        <span
          className="size-2 rounded-full"
          style={{ background: group.project?.color ?? "color-mix(in srgb, var(--color-white) 40%, transparent)" }}
        />
        <span className="font-semibold text-white/80">{group.project?.name ?? "Inbox"}</span>
        <span className="tabular-nums">{group.tasks.length}</span>
      </header>
      <Reorder.Group axis="y" values={items} onReorder={setItems} className="flex flex-col">
        {items.map((task) => (
          <Reorder.Item
            key={task.id}
            value={task}
            onDragStart={() => setDragging(task.id)}
            onDragEnd={() => void persist()}
            className="cursor-grab active:cursor-grabbing"
          >
            <TaskRow task={task} project={group.project} />
          </Reorder.Item>
        ))}
      </Reorder.Group>
    </section>
  );
}

/**
 * Centered empty state for a tab.
 *
 * @param props.title - Headline.
 */
export function Empty({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center px-10 text-center">
      <p className="font-display text-[15px] font-semibold text-white/85">{title}</p>
      <div className="mt-1.5 flex max-w-sm flex-col items-center text-[12px] leading-relaxed text-white/50">{children}</div>
    </div>
  );
}
