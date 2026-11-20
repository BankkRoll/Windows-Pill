/**
 * Small shared building blocks for the panel: icon buttons, chips and keys.
 */

import type { FluentIcon } from "@fluentui/react-icons";
import type { ButtonHTMLAttributes, ReactNode } from "react";

/** Props for {@link IconButton}. */
export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: FluentIcon;
  /** Accessible label, also shown as a tooltip. */
  label: string;
  active?: boolean;
  /** Shows a small accent dot in the corner, for something waiting behind the button. */
  badge?: boolean;
}

/**
 * A 28 px square button holding a 16 px Fluent icon.
 *
 * @example
 * <IconButton icon={Settings16Regular} label="Settings" onClick={open} />
 */
export function IconButton({ icon: Icon, label, active, badge, className = "", ...rest }: IconButtonProps) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      className={`relative grid size-7 shrink-0 place-items-center rounded-lg outline-none transition-colors focus-visible:ring-1 focus-visible:ring-white/40 disabled:opacity-35 ${
        active ? "bg-white/12 text-white/92" : "text-white/50 hover:bg-white/8 hover:text-white/85"
      } ${className}`}
      {...rest}
    >
      <Icon className="size-4" />
      {badge && <span aria-hidden className="absolute top-1 right-1 size-1.5 rounded-full bg-accent" />}
    </button>
  );
}

const CHIP_TONES = {
  neutral: "bg-white/8 text-white/70",
  accent: "bg-accent/18 text-accent",
  red: "bg-status-red/16 text-status-red",
  amber: "bg-status-amber/16 text-status-amber",
  green: "bg-status-green/16 text-status-green",
} as const;

/** Color treatment of a {@link Chip}. */
export type ChipTone = keyof typeof CHIP_TONES;

/**
 * Compact pill-shaped label used for project, priority, due and tags.
 *
 * @param props.tone - Color treatment.
 * @param props.dot - Optional leading color dot (e.g. a project color).
 */
export function Chip({ children, tone = "neutral", dot }: { children: ReactNode; tone?: ChipTone; dot?: string }) {
  return (
    <span
      className={`inline-flex h-5 max-w-40 shrink-0 items-center gap-1 rounded-md px-1.5 text-[11px] leading-none font-medium whitespace-nowrap ${CHIP_TONES[tone]}`}
    >
      {dot && <span className="size-1.5 shrink-0 rounded-full" style={{ background: dot }} />}
      <span className="truncate">{children}</span>
    </span>
  );
}

/** A keyboard key cap, for hints like Ctrl + Shift + Space. */
export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-grid h-4.5 min-w-4.5 place-items-center rounded border border-white/12 px-1 font-sans text-[10px] text-white/55">
      {children}
    </kbd>
  );
}

/** Tone for a priority value. */
export function priorityTone(p: 1 | 2 | 3 | null): ChipTone {
  return p === 1 ? "red" : p === 2 ? "amber" : "neutral";
}
