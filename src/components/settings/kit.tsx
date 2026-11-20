/**
 * Building blocks shared by every Settings section: section headers,
 * label/control rows, path rows and committed number/time inputs. Keeping
 * them in one place keeps spacing and type sizes identical across sections.
 */
import { Dismiss12Regular, type FluentIcon } from "@fluentui/react-icons";
import { createContext, useEffect, useRef, useState, type ReactNode } from "react";
import { useKeyboardFocus } from "../../hooks/useKeyboardFocus";
import { TimePicker } from "../controls";

/**
 * Titled block of rows inside a settings page.
 *
 * @param props.title - Heading.
 * @param props.hint - One-line explanation under the heading.
 * @param props.action - Optional control aligned to the right of the heading.
 */
export function Group({ title, hint, action, children }: { title: string; hint?: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-1.5">
      <header className="flex min-h-7 items-end justify-between gap-3 px-1">
        <div className="min-w-0">
          <h3 className="text-[11px] font-semibold tracking-wider text-white/45 uppercase">{title}</h3>
          {hint && <p className="text-[11.5px] leading-snug text-white/35">{hint}</p>}
        </div>
        {action}
      </header>
      <div className="flex flex-col divide-y divide-white/[0.05] rounded-xl bg-white/[0.035] [&>*:first-child]:rounded-t-xl [&>*:last-child]:rounded-b-xl">
        {children}
      </div>
    </section>
  );
}

/**
 * Label on the left, control on the right. Hints wrap under the label.
 *
 * @param props.label - Setting name.
 * @param props.hint - Optional explanation.
 */
export function Row({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex min-h-10 items-center justify-between gap-4 px-3 py-1.5">
      <span className="min-w-0">
        <span className="block text-[12.5px] text-white/85">{label}</span>
        {hint && <span className="block text-[11px] leading-snug text-white/40">{hint}</span>}
      </span>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

/**
 * A folder or drive with optional metadata and a remove button on hover.
 *
 * @param props.icon - Leading icon.
 * @param props.path - Absolute path, shown truncated with the full value as a tooltip.
 * @param props.meta - Right-aligned detail such as a repo count.
 * @param props.onRemove - Removes the path.
 */
export function PathRow({ icon: Icon, path, meta, onRemove }: { icon: FluentIcon; path: string; meta?: string; onRemove: () => void }) {
  return (
    <div className="group flex h-9 items-center gap-2.5 px-3">
      <Icon className="size-4 shrink-0 text-white/45" />
      <span className="min-w-0 flex-1 truncate text-[12.5px] text-white/85" title={path}>
        {path}
      </span>
      {meta && <span className="text-[11.5px] text-white/40 tabular-nums">{meta}</span>}
      <button
        type="button"
        aria-label={`Remove ${path}`}
        title="Remove"
        onClick={onRemove}
        className="grid size-6 place-items-center rounded-md text-white/40 opacity-0 group-hover:opacity-100 hover:bg-white/8 hover:text-white/90 focus-visible:opacity-100"
      >
        <Dismiss12Regular className="size-3" />
      </button>
    </div>
  );
}

/**
 * Small secondary button with a leading icon.
 *
 * @param props.icon - Leading icon element.
 * @param props.onClick - Click handler.
 * @param props.disabled - Disables the button.
 */
export function SoftButton({
  icon,
  children,
  onClick,
  disabled,
}: {
  icon?: ReactNode;
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="inline-flex h-7 items-center gap-1.5 rounded-lg bg-white/7 px-2.5 text-[12px] text-white/80 hover:bg-white/12 hover:text-white disabled:opacity-40"
    >
      {icon}
      {children}
    </button>
  );
}

/**
 * Integer input that commits on blur or Enter and snaps back when the value
 * is out of range, so a half-typed number never reaches the config.
 *
 * @param props.value - Current value.
 * @param props.min - Inclusive minimum.
 * @param props.max - Inclusive maximum.
 * @param props.suffix - Unit shown after the field.
 * @param props.onCommit - Called with a valid, changed value.
 */
export function NumberField({
  value,
  min,
  max,
  suffix,
  onCommit,
}: {
  value: number;
  min: number;
  max: number;
  suffix?: string;
  onCommit: (v: number) => void;
}) {
  const [text, setText] = useState(String(value));
  const focus = useKeyboardFocus();
  // Escape blurs the field, and the blur would otherwise commit the text
  // the reset hasn't replaced yet.
  const cancelled = useRef(false);
  useEffect(() => setText(String(value)), [value]);
  const commit = () => {
    const n = Math.round(Number(text));
    if (text !== "" && Number.isFinite(n) && n >= min && n <= max && n !== value) onCommit(n);
    else setText(String(value));
  };
  return (
    <span className="inline-flex items-center gap-1.5">
      <input
        inputMode="numeric"
        value={text}
        aria-label={suffix}
        onChange={(e) => setText(e.target.value.replace(/\D/g, ""))}
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
          if (e.key === "Enter") focus.release(e.currentTarget);
          if (e.key === "Escape") {
            e.preventDefault();
            cancelled.current = true;
            setText(String(value));
            focus.release(e.currentTarget);
          }
        }}
        className="h-7 w-16 rounded-lg bg-white/7 px-2 text-right text-[12px] text-white/85 tabular-nums outline-none focus:bg-white/10 focus:ring-1 focus:ring-accent/60"
      />
      {suffix && <span className="w-10 text-[11.5px] text-white/40">{suffix}</span>}
    </span>
  );
}

/**
 * Time-of-day setting in 15-minute steps, which is as fine as a schedule
 * window needs. Clearing commits `null`.
 *
 * @param props.value - `HH:MM` or `null`.
 * @param props.onCommit - Called with each picked time.
 * @param props.label - Accessible name.
 */
export function TimeField({ value, onCommit, label }: { value: string | null; onCommit: (v: string | null) => void; label: string }) {
  return <TimePicker label={label} value={value} onChange={onCommit} minuteStep={15} align="end" />;
}

/**
 * Page heading for a settings section.
 *
 * @param props.title - Section name.
 * @param props.subtitle - One sentence on what lives here.
 * @param props.action - Optional control on the right.
 */
export function PageHeader({ title, subtitle, action }: { title: string; subtitle: string; action?: ReactNode }) {
  return (
    <header className="flex items-start justify-between gap-3 px-1 pb-1">
      <div className="min-w-0">
        <h2 className="text-[15px] font-semibold text-white/95">{title}</h2>
        <p className="text-[12px] text-white/45">{subtitle}</p>
      </div>
      {action}
    </header>
  );
}

/**
 * Where a settings page renders a floating bar (e.g. unsaved changes). It
 * sits over the bottom of the scroll area rather than inside it, so the
 * area's edge fade doesn't dim the bar.
 */
export const SettingsFooterContext = createContext<HTMLElement | null>(null);
