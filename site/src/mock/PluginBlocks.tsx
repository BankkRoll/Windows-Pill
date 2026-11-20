/**
 * A static copy of the app's plugin page renderer (src/components/PluginBlocks.tsx):
 * the same markup and classes for headers, stat tiles, sections, lists and
 * notices, without the plugin round trips behind the buttons.
 */

import { ChevronRight12Regular } from "@fluentui/react-icons";
import type { ReactNode } from "react";
import { PluginIcon, type PluginIconName } from "./PluginIcon";

/** A tone from the plugin protocol's fixed list. */
export type PluginTone = "neutral" | "accent" | "green" | "amber" | "red" | "purple";

/** Severity of an advisory row. */
export type Severity = "critical" | "high" | "medium" | "low" | "info";

/** A small coloured label. */
export interface PageChip {
  text: string;
  tone: PluginTone;
}

/** A button on a page; drawn only. */
export interface PageButton {
  label: string;
  icon?: PluginIconName | null;
  primary?: boolean;
}

/** One row of a list block. */
export interface PageRow {
  title: string;
  detail?: string;
  severity?: Severity;
  tone?: PluginTone;
  icon?: PluginIconName;
  chips?: PageChip[];
  actions?: PageButton[];
}

/** One block of a plugin page, as far as the mockups use them. */
export type PageBlock =
  | { type: "header"; title: string; subtitle: string; tone: PluginTone; actions: PageButton[] }
  | { type: "stats"; items: { label: string; value: string; detail?: string; tone: PluginTone }[] }
  | { type: "section"; title: string; count: number; blocks: PageBlock[] }
  | { type: "list"; rows: PageRow[] }
  | { type: "notice"; tone: PluginTone; icon: PluginIconName; title: string; text: string; actions: PageButton[] };

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

const SEVERITY: Record<Severity, { label: string; tone: PluginTone }> = {
  critical: { label: "Critical", tone: "red" },
  high: { label: "High", tone: "amber" },
  medium: { label: "Medium", tone: "neutral" },
  low: { label: "Low", tone: "neutral" },
  info: { label: "Info", tone: "neutral" },
};

/**
 * Draws a plugin page from its blocks.
 *
 * @param props.blocks - The page, in the shape a plugin sends it.
 */
export function PluginBlocks({ blocks }: { blocks: readonly PageBlock[] }) {
  return (
    <div className="flex flex-col gap-4">
      {blocks.map((block, i) => (
        <BlockView key={i} block={block} />
      ))}
    </div>
  );
}

function BlockView({ block }: { block: PageBlock }) {
  switch (block.type) {
    case "header":
      return (
        <header className="flex items-start gap-3 px-1 pt-0.5">
          <div className="min-w-0 flex-1">
            <h2 className="truncate font-display text-[16px] leading-tight font-semibold text-white/92">{block.title}</h2>
            <p className="mt-1 flex min-w-0 items-center gap-1.5 text-[11.5px] text-white/50">
              {block.tone !== "neutral" && <span aria-hidden className={`size-1.5 shrink-0 rounded-full ${TONES[block.tone].dot}`} />}
              <span className="truncate">{block.subtitle}</span>
            </p>
          </div>
          <Buttons buttons={block.actions} row={false} />
        </header>
      );
    case "stats":
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
    case "section":
      return (
        <section aria-label={block.title} className="flex flex-col gap-1.5">
          <div className="group flex h-6 items-center gap-1.5 rounded-md px-1 text-left text-[10.5px] font-medium tracking-wide uppercase">
            <span className="truncate text-white/45">{block.title}</span>
            <span className="text-white/30 tabular-nums">{block.count}</span>
            <ChevronRight12Regular className="size-3 shrink-0 rotate-90 text-white/30" />
          </div>
          <div className="flex flex-col gap-2">
            {block.blocks.map((child, i) => (
              <BlockView key={i} block={child} />
            ))}
          </div>
        </section>
      );
    case "list":
      return (
        <ul className="flex flex-col divide-y divide-white/[0.06] overflow-hidden rounded-xl bg-white/[0.025] ring-1 ring-white/[0.06] ring-inset">
          {block.rows.map((row, i) => (
            <RowView key={i} row={row} />
          ))}
        </ul>
      );
    case "notice":
      return (
        <div className="flex items-start gap-2.5 rounded-xl bg-white/[0.035] px-3 py-2.5 ring-1 ring-white/[0.06] ring-inset" role="note">
          <PluginIcon icon={block.icon} className={`mt-px size-4 shrink-0 ${TONES[block.tone].icon}`} />
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <p className="text-[12.5px] font-medium text-white/85">{block.title}</p>
            <p className="text-[11.5px] leading-relaxed text-white/65">{block.text}</p>
            <Buttons buttons={block.actions} row={false} className="mt-1" />
          </div>
        </div>
      );
  }
}

function RowView({ row }: { row: PageRow }) {
  const severity = row.severity ? SEVERITY[row.severity] : null;
  const tone = severity?.tone ?? row.tone ?? "neutral";
  const buttons = row.actions ?? [];
  // NOTE: as in the app, a lone button sits beside the title instead of costing the row another line.
  const aside = buttons.length === 1;
  return (
    <li className="hover:bg-white/[0.03]">
      <div className="flex items-start gap-2.5 px-3 py-2.5">
        <span className="grid h-[18px] w-3.5 shrink-0 place-items-center" aria-hidden>
          <span className={`size-2 rounded-full ${TONES[tone].dot}`} />
        </span>
        <div className="min-w-0 flex-1">
          <span className="block truncate text-[12.5px] leading-snug text-white/90">{row.title}</span>
          {row.detail && <span className="block truncate text-[11px] text-white/45">{row.detail}</span>}
          {row.chips && row.chips.length > 0 && (
            <span className="mt-1 flex min-w-0 flex-wrap items-center gap-1">
              {row.chips.map((chip, i) => (
                <ChipView key={i} chip={chip} />
              ))}
            </span>
          )}
        </div>
        {(severity || aside) && (
          <span className="flex shrink-0 flex-col items-end gap-1.5">
            {severity && <span className={`mt-px text-[11px] font-medium ${TONES[severity.tone].text}`}>{severity.label}</span>}
            {aside && <Buttons buttons={buttons} row className="justify-end" />}
          </span>
        )}
      </div>
      {!aside && buttons.length > 0 && <Buttons buttons={buttons} row className="px-3 pb-2.5 pl-[36px]" />}
    </li>
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

function Buttons({ buttons, row, className = "" }: { buttons: readonly PageButton[]; row: boolean; className?: string }) {
  if (buttons.length === 0) return null;
  return (
    <div className={`flex flex-wrap items-center gap-1.5 ${className}`}>
      {buttons.map((b) => (
        // NOTE: a list repeats its row buttons down the page; only page-level buttons get the accent.
        <ActionButton key={b.label} primary={!!b.primary && !row}>
          {b.icon && <PluginIcon icon={b.icon} className="size-3" />}
          {b.label}
        </ActionButton>
      ))}
    </div>
  );
}

function ActionButton({ primary, children }: { primary: boolean; children: ReactNode }) {
  return (
    <span
      className={`inline-flex h-6 items-center gap-1 rounded-md px-2 text-[11.5px] whitespace-nowrap ${
        primary ? "bg-accent/18 font-medium text-accent" : "bg-white/7 text-white/80"
      }`}
    >
      {children}
    </span>
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
