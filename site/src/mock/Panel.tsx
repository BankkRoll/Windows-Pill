/**
 * The notch's two faces, drawn with the markup and classes of the app's
 * CollapsedContent.tsx and ExpandedPanel.tsx: the collapsed clock and status
 * cluster, and the expanded panel's header, content area and command bar.
 */

import {
  Add16Regular,
  Board16Regular,
  Bot16Filled,
  Bot16Regular,
  CalendarToday16Regular,
  Chat16Regular,
  ChevronDown12Regular,
  Clock16Filled,
  Mic16Regular,
  Settings16Regular,
  Shield16Regular,
  ShieldError16Filled,
  WindowNew16Regular,
} from "@fluentui/react-icons";
import type { ReactNode } from "react";
import { SHAPES } from "../lib/shapes";
import { Artwork } from "./Today";
import { IconButton } from "./ui";

/** A header tab of the panel. */
export type PanelTab = "today" | "board" | "chat" | "agents" | "security";

const TABS = [
  { id: "today", label: "Today", icon: CalendarToday16Regular },
  { id: "board", label: "Board", icon: Board16Regular },
  { id: "chat", label: "Chat", icon: Chat16Regular },
  { id: "agents", label: "Agents", icon: Bot16Regular },
] as const;

/**
 * The collapsed notch: the time on the left; the live item (Now playing
 * artwork) and the status cluster on the right.
 */
export function CollapsedContent() {
  const { width, height } = SHAPES.peek;
  return (
    <div className="flex items-center justify-between gap-2 px-3" style={{ width, height }}>
      <div className="flex items-baseline gap-2 whitespace-nowrap">
        <span className="text-[13px] font-semibold tabular-nums">9:41</span>
      </div>
      <div className="flex min-w-0 items-center gap-2">
        <span className="flex shrink-0 items-center gap-1.5">
          <Artwork className="size-4 rounded-[4px]" />
          <Bars />
        </span>
        <div className="flex items-center gap-1.5">
          <span className="flex items-center gap-0.5 rounded-full text-status-red" title="Critical or high alerts">
            <ShieldError16Filled className="size-3.5" />
            <span className="text-[10px] leading-none font-semibold tabular-nums">2</span>
          </span>
          <span className="flex items-center gap-0.5 rounded-full text-accent" title="Agent running">
            <Bot16Filled className="size-3.5 animate-breathe motion-reduce:animate-none" />
          </span>
          <span className="flex items-center gap-0.5 rounded-full text-status-amber" title="Due today">
            <Clock16Filled className="size-3.5" />
            <span className="text-[10px] leading-none font-semibold tabular-nums">3</span>
          </span>
        </div>
      </div>
    </div>
  );
}

/** Three small equalizer bars: the "playing" hint next to the artwork. */
function Bars() {
  const heights = [3, 7, 4];
  return (
    <span className="flex h-2.5 items-end gap-[2px]" aria-hidden>
      {heights.map((h, i) => (
        <span
          key={i}
          className="eq-bar w-[2px] rounded-full bg-white/65"
          style={{ height: 8, transform: `scaleY(${h / 8})`, animationDuration: `${0.9 + i * 0.15}s`, animationDelay: `${-i * 0.3}s` }}
        />
      ))}
    </span>
  );
}

/** Props for {@link ExpandedPanel}. */
export interface ExpandedPanelProps {
  active: PanelTab;
  /** The header's project picker; "All projects" when absent. */
  project?: { name: string; color: string };
  /** Whether the command bar shows; the app hides it on Chat, Settings and plugin tabs. */
  commandBar?: boolean;
  children: ReactNode;
}

/**
 * The expanded panel at its real size (680 × 440): tab bar, project picker
 * and settings on top, the active tab in the middle, the command bar below.
 */
export function ExpandedPanel({ active, project, commandBar = true, children }: ExpandedPanelProps) {
  const { width, height } = SHAPES.expanded;
  return (
    <div className="relative flex flex-col text-[13px] text-white/92" style={{ width, height }}>
      <header className="grid h-11 shrink-0 grid-cols-[1fr_auto_1fr] items-center px-3 pt-1">
        <nav className="flex items-center gap-0.5 self-stretch" aria-label="Tabs">
          {TABS.map((t) => (
            <IconButton key={t.id} icon={t.icon} label={t.label} active={active === t.id} tabIndex={-1} />
          ))}
          <span aria-hidden className="mx-1 h-4 w-px bg-white/10" />
          <IconButton icon={Shield16Regular} label="Security" active={active === "security"} tabIndex={-1} />
        </nav>
        <span className="inline-flex h-7 max-w-56 items-center gap-1.5 rounded-lg px-2.5 text-[12px] text-white/80">
          <span
            className="size-2 shrink-0 rounded-full"
            style={{ background: project?.color ?? "conic-gradient(#2F7BFF, #30D158, #FFB340, #FF453A, #2F7BFF)" }}
          />
          <span className="truncate font-medium">{project?.name ?? "All projects"}</span>
          <ChevronDown12Regular className="size-3 shrink-0 text-white/45" />
        </span>
        <div className="flex items-center justify-end gap-0.5 self-stretch">
          <IconButton icon={WindowNew16Regular} label="Pop out into a window" tabIndex={-1} />
          <IconButton icon={Settings16Regular} label="Settings" tabIndex={-1} />
        </div>
      </header>

      <main className="fade-bottom relative min-h-0 flex-1 overflow-hidden px-2">{children}</main>

      {commandBar ? (
        <footer className="shrink-0 px-3 pt-2 pb-3">
          <div className="flex h-9 shrink-0 items-center gap-2 rounded-xl bg-white/6 px-3">
            <Add16Regular className="size-4 shrink-0 text-white/40" />
            <span className="min-w-0 flex-1 truncate text-[13px] text-white/30">Add a task, type / for commands, or ? to ask AI</span>
            <span className="grid size-6 shrink-0 place-items-center rounded-md text-white/40">
              <Mic16Regular className="size-3.5" />
            </span>
          </div>
        </footer>
      ) : (
        <div className="h-2 shrink-0" />
      )}
    </div>
  );
}
