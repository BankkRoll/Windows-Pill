/**
 * The Today tab's pieces with the markup and classes of the app's
 * NowPlayingCard.tsx, NotificationsCard.tsx, PluginWidgets.tsx,
 * PluginFeed.tsx and TaskRow.tsx, filled with demo data.
 */

import {
  ArrowRepeatAll16Regular,
  ArrowShuffle16Regular,
  Apps16Regular,
  ChevronDown12Regular,
  ChevronRight12Regular,
  Next16Filled,
  Pause16Filled,
  Previous16Filled,
  Speaker116Regular,
  type FluentIcon,
} from "@fluentui/react-icons";
import { ALERTS, NOTIFICATIONS, TODAY, TRACK, type DemoAlert, type DemoNotification, type DemoTask } from "./demo";
import { PluginIcon } from "./PluginIcon";
import { Chip, priorityTone, type ChipTone } from "./ui";

/**
 * Cover art for the demo track: drawn in CSS so no image is needed.
 *
 * @param props.className - Size and corner classes.
 */
export function Artwork({ className }: { className: string }) {
  return (
    <span
      aria-hidden
      className={`block shrink-0 ${className}`}
      style={{
        background:
          "radial-gradient(circle at 50% 46%, #f4c27a 0 17%, transparent 18%), linear-gradient(to bottom, #3b5a6b 0 60%, #22394a 60%)",
      }}
    />
  );
}

function formatClock(ms: number): string {
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function Control({ icon: Icon, label, primary = false }: { icon: FluentIcon; label: string; primary?: boolean }) {
  return (
    <span
      title={label}
      className={`grid place-items-center rounded-full ${primary ? "size-8 bg-white/90 text-black/85" : "size-7 text-white/70"}`}
    >
      <Icon className="size-4" />
    </span>
  );
}

/** The Now playing card: player, artwork, title, seek bar, transport, shuffle, repeat and per-app volume. */
export function NowPlayingCard() {
  const percent = (TRACK.positionMs / TRACK.durationMs) * 100;
  return (
    <section
      className="flex flex-col gap-2.5 rounded-xl bg-white/[0.045] px-3 pt-2 pb-2.5 ring-1 ring-white/[0.06]"
      aria-label="Now playing"
    >
      <div className="flex h-6 min-w-0 items-center gap-1.5 text-[11.5px] text-white/60">
        <span className="-mx-1.5 inline-flex h-6 min-w-0 items-center gap-1.5 rounded-md px-1.5">
          <Apps16Regular className="size-3.5 shrink-0 text-white/45" />
          <span className="truncate font-medium">{TRACK.app}</span>
          <ChevronDown12Regular className="size-3 shrink-0 text-white/40" />
        </span>
        <span className="shrink-0 text-white/35">Playing</span>
      </div>
      <div className="flex items-center gap-3">
        <Artwork className="h-14 w-14 rounded-lg" />
        <div className="min-w-0 flex-1">
          <p className="line-clamp-2 text-[13px] leading-snug font-semibold break-words text-white/92">{TRACK.title}</p>
          <p className="mt-0.5 truncate text-[11.5px] text-white/50">{TRACK.artist}</p>
        </div>
      </div>
      <div className="flex flex-col gap-0.5">
        <div className="relative flex h-3 w-full items-center">
          <div className="relative h-1 w-full rounded-full bg-white/12">
            <div className="absolute inset-y-0 left-0 rounded-full bg-white/70" style={{ width: `${percent}%` }} />
          </div>
        </div>
        <div className="flex justify-between text-[10.5px] text-white/40 tabular-nums">
          <span>{formatClock(TRACK.positionMs)}</span>
          <span>{formatClock(TRACK.durationMs)}</span>
        </div>
      </div>
      <div className="flex h-8 items-center gap-1">
        <Control icon={Previous16Filled} label="Previous" />
        <Control icon={Pause16Filled} label="Pause" primary />
        <Control icon={Next16Filled} label="Next" />
        <span aria-hidden className="w-1.5" />
        <span className="grid size-7 shrink-0 place-items-center rounded-full text-white/40">
          <ArrowShuffle16Regular className="size-3.5" />
        </span>
        <span className="grid size-7 shrink-0 place-items-center rounded-full text-accent">
          <ArrowRepeatAll16Regular className="size-3.5" />
        </span>
        <span className="flex-1" />
        <div className="flex shrink-0 items-center gap-1">
          <span className="grid size-6 shrink-0 place-items-center rounded-md text-white/55">
            <Speaker116Regular className="size-3.5" />
          </span>
          <div className="relative flex h-4 w-20 items-center">
            <div className="relative h-1 w-full rounded-full bg-white/12">
              <div className="absolute inset-y-0 left-0 rounded-full bg-white/60" style={{ width: `${TRACK.volume * 100}%` }} />
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

/** The Windows notifications card: Action Center, newest first. */
export function NotificationsCard() {
  return (
    <section
      className="flex min-w-0 flex-col rounded-xl bg-white/[0.045] px-1.5 pt-1.5 pb-1 ring-1 ring-white/[0.06]"
      aria-label="Notifications"
    >
      <header className="flex h-6 items-center justify-between gap-2 pr-0.5 pl-1.5">
        <h3 className="flex items-baseline gap-1.5 text-[12px] font-semibold text-white/85">
          Notifications
          <span className="text-[11px] font-normal text-white/40 tabular-nums">{NOTIFICATIONS.length}</span>
        </h3>
        <span className="inline-flex h-6 items-center gap-1 rounded-md px-2 text-[11.5px] text-white/60">Clear all</span>
      </header>
      <ul className="flex flex-col">
        {NOTIFICATIONS.map((note) => (
          <NotificationRow key={note.id} note={note} />
        ))}
      </ul>
    </section>
  );
}

function NotificationRow({ note }: { note: DemoNotification }) {
  return (
    <li className="shrink-0 overflow-hidden">
      <div className="flex items-start gap-2.5 rounded-lg px-1.5 py-1.5">
        <span
          className="mt-px grid size-6 shrink-0 place-items-center rounded-md bg-white/8 text-[11px] font-semibold text-white/60"
          aria-hidden
        >
          {note.app.charAt(0)}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex h-4 items-center gap-2">
            <span className="min-w-0 flex-1 truncate text-[12px] font-medium text-white/90">{note.headline}</span>
            <span className="max-w-[40%] shrink-0 truncate text-[11px] text-white/40">{note.app}</span>
            <span className="grid h-4 min-w-6 shrink-0 place-items-center text-[10.5px] text-white/35 tabular-nums">{note.age}</span>
          </div>
          <p className="mt-0.5 line-clamp-2 text-[11.5px] leading-snug text-white/55">{note.body}</p>
        </div>
      </div>
    </li>
  );
}

const SEVERITY = {
  critical: { label: "Critical", text: "text-status-red", dot: "bg-status-red" },
  high: { label: "High", text: "text-status-amber", dot: "bg-status-amber" },
  medium: { label: "Medium", text: "text-white/55", dot: "bg-white/45" },
} as const;

/** CVE watch's Today card: its summary row and button, then its alerts. */
export function CveWatchCard() {
  return (
    <section className="flex flex-col gap-2 rounded-xl bg-white/[0.045] px-3 py-2.5 ring-1 ring-white/[0.06]" aria-label="CVE watch">
      <div className="flex flex-col gap-1.5">
        <header className="flex items-center justify-between gap-2">
          <h3 className="truncate text-[12px] font-semibold text-white/85">CVE watch</h3>
          <div className="flex shrink-0 items-center gap-1.5">
            <span className="inline-flex h-6 items-center gap-1 rounded-md px-2 text-[11.5px] text-white/70">
              Open Security
              <ChevronRight12Regular className="size-3" />
            </span>
            <span className="inline-flex h-6 items-center gap-1 rounded-md bg-white/7 px-2 text-[11.5px] text-white/80">
              <PluginIcon icon="shield" className="size-3" />
              Check now
            </span>
          </div>
        </header>
        <ul className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-x-4 gap-y-1">
          <li className="flex min-w-0 items-start gap-2">
            <PluginIcon icon="shield" className="mt-px size-3.5 shrink-0 text-status-red" />
            <span className="line-clamp-2 block text-[12px] leading-snug text-white/85">2 critical · 4 high · 2 more in 4 projects</span>
          </li>
          <li className="flex min-w-0 items-start gap-2">
            <PluginIcon icon="clock" className="mt-px size-3.5 shrink-0 text-white/70" />
            <span className="line-clamp-2 block text-[12px] leading-snug text-white/85">Checked 09:12</span>
          </li>
        </ul>
      </div>
      <ul className="flex flex-col gap-1">
        {ALERTS.map((alert) => (
          <AlertRow key={alert.title} alert={alert} />
        ))}
      </ul>
      <footer className="flex items-center gap-2">
        <span className="h-6 rounded-md px-1.5 text-[11.5px] leading-6 text-white/45">Show all 6</span>
        <span className="ml-auto h-6 rounded-md px-2 text-[11.5px] leading-6 text-white/45">Dismiss all</span>
      </footer>
    </section>
  );
}

function AlertRow({ alert }: { alert: DemoAlert }) {
  const severity = SEVERITY[alert.severity];
  return (
    <li className="relative shrink-0 overflow-hidden rounded-[10px] ring-1 ring-white/[0.06] ring-inset">
      <div className="flex items-start gap-2.5 px-2.5 py-2">
        <span className="grid h-[18px] w-3 shrink-0 place-items-center" aria-hidden>
          <span className={`size-2 rounded-full ${severity.dot}`} />
        </span>
        <div className="min-w-0 flex-1">
          <span className="block truncate text-[12.5px] leading-snug text-white/90">{alert.title}</span>
          <span className="block truncate text-[11px] text-white/45">{alert.detail}</span>
          <div className="mt-1 flex min-w-0 items-center gap-1.5">
            <span className={`text-[11px] font-medium ${severity.text}`}>{severity.label}</span>
            <Chip dot={alert.project.color}>{alert.project.name}</Chip>
          </div>
        </div>
      </div>
    </li>
  );
}

const DUE_TONE: Record<string, ChipTone> = { overdue: "red", today: "amber", soon: "accent", later: "neutral" };

const RING: Record<string, string> = {
  1: "border-status-red/80",
  2: "border-status-amber/80",
  3: "border-white/35",
  none: "border-white/30",
};

function TaskRow({ task }: { task: DemoTask }) {
  return (
    <div className="relative flex h-8 items-center gap-2.5 rounded-lg pr-1.5 pl-2">
      <span className={`grid size-4 shrink-0 place-items-center rounded-full border-[1.5px] ${RING[task.priority ?? "none"]}`} />
      <span className="min-w-0 flex-1 truncate text-[13px] text-white/92">{task.title}</span>
      <div className="flex shrink-0 items-center gap-1">
        {task.tags.slice(0, 2).map((t) => (
          <Chip key={t}>{t}</Chip>
        ))}
        {task.priority === 1 && <Chip tone={priorityTone(1)}>P1</Chip>}
        {task.due && <Chip tone={DUE_TONE[task.due.tone]}>{task.due.text}</Chip>}
        {task.agent ? <Chip tone="accent">Agent working</Chip> : task.status === "doing" && <Chip tone="accent">Doing</Chip>}
      </div>
    </div>
  );
}

/** Due today and in Doing across projects, grouped by project. */
export function TodayTasks() {
  return (
    <div className="flex flex-col gap-3 py-1">
      {TODAY.map((group) => (
        <section key={group.project.id}>
          <header className="flex items-center gap-2 px-2 pb-1 text-[12px] text-white/60">
            <span className="size-2 rounded-full" style={{ background: group.project.color }} />
            <span className="font-semibold text-white/80">{group.project.name}</span>
            <span className="tabular-nums">{group.tasks.length}</span>
          </header>
          <div className="flex flex-col">
            {group.tasks.map((task) => (
              <TaskRow key={task.title} task={task} />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

/** The Today tab as the panel shows it: Pill's own cards side by side, then the tasks. */
export function TodayTab() {
  return (
    <div className="flex min-h-full flex-col">
      <div className="flex shrink-0 flex-col gap-2 pt-1 pb-3">
        <div className="grid grid-cols-2 items-start gap-2">
          <NowPlayingCard />
          <NotificationsCard />
        </div>
      </div>
      <TodayTasks />
    </div>
  );
}
