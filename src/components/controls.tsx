/**
 * Themed replacements for native form controls, which render as unstyled OS
 * widgets inside the overlay and ignore the app theme.
 *
 * - {@link Select}: single choice from a popover listbox.
 * - {@link DatePicker}: `YYYY-MM-DD` day from a month calendar.
 * - {@link TimePicker}: `HH:MM` time from hour and minute columns.
 * - {@link Checkbox}: accent-filled box with an animated check.
 *
 * Popovers render into `document.body` with fixed positioning so scroll
 * areas and `overflow-hidden` parents can't clip them. They stay inside the
 * nearest `[data-popover-bounds]` ancestor of their trigger, because anything
 * outside the notch's visible shape is click-through.
 */
import {
  CalendarLtr16Regular,
  Checkmark16Regular,
  ChevronDown12Regular,
  ChevronLeft16Regular,
  ChevronRight16Regular,
  Clock16Regular,
} from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { addDays, addMonths, firstDayOfWeek, monthGrid, parseDay, sameDay, startOfDay, startOfWeek } from "../lib/calendar";
import { dueLabel, formatDate, isoDay } from "../lib/format";
import { useDismiss } from "../hooks/useDismiss";
import { focusWindow } from "../lib/native";
import { IconButton } from "./ui";

// -----------------------------------------------------------------------------
// Popover
// -----------------------------------------------------------------------------

/** Which side of the trigger a popover prefers; it flips when that side lacks room. */
export type PopoverPlacement = "up" | "down";

/** Which trigger edge a popover lines up with. */
export type PopoverAlign = "start" | "end";

const GAP = 6;
const EDGE = 8;

const TRIGGER =
  "inline-flex h-7 min-w-0 items-center gap-2 rounded-lg bg-white/7 px-2.5 text-[12px] text-white/85 outline-none transition-colors hover:bg-white/10 focus-visible:ring-1 focus-visible:ring-white/30 disabled:opacity-50 disabled:hover:bg-white/7";

// NOTE: Triggers ask for keyboard focus on pointer-down like text fields do,
// but skip useKeyboardFocus's focus hold: a focused button would otherwise pin
// the notch open after the popover has closed. The popover holds it instead.
const askForKeys = () => void focusWindow();

const keepFocus = (e: ReactMouseEvent) => e.preventDefault();

function boundsOf(el: HTMLElement) {
  const box = el.closest<HTMLElement>("[data-popover-bounds]")?.getBoundingClientRect();
  return {
    top: Math.max(0, box?.top ?? 0) + EDGE,
    bottom: Math.min(window.innerHeight, box?.bottom ?? Infinity) - EDGE,
    left: Math.max(0, box?.left ?? 0) + EDGE,
    right: Math.min(window.innerWidth, box?.right ?? Infinity) - EDGE,
  };
}

/**
 * Positions a fixed panel next to its trigger: flips to the roomier side when
 * the preferred one can't fit the panel's natural height, clamps it inside the
 * bounds horizontally, and follows the trigger when an ancestor scrolls.
 */
function useFloating(
  open: boolean,
  anchor: RefObject<HTMLElement | null>,
  panel: RefObject<HTMLElement | null>,
  placement: PopoverPlacement,
  align: PopoverAlign,
  cap: number,
) {
  const [style, setStyle] = useState<CSSProperties>({ position: "fixed", top: 0, left: 0 });
  const [up, setUp] = useState(placement === "up");

  const update = useCallback(() => {
    const a = anchor.current;
    const p = panel.current;
    if (!a || !p) return;
    const r = a.getBoundingClientRect();
    const b = boundsOf(a);
    const below = b.bottom - r.bottom - GAP;
    const above = r.top - b.top - GAP;
    // scrollHeight stays the natural height after maxHeight starts limiting it.
    const height = Math.min(p.scrollHeight, cap);
    const width = Math.max(p.offsetWidth, r.width);
    const left = Math.min(Math.max(align === "end" ? r.right - width : r.left, b.left), Math.max(b.left, b.right - width));
    if (height > Math.max(above, below)) {
      // Neither side fits, e.g. a calendar in the short notch: slide it up
      // over the trigger rather than squeezing it into a scroll box.
      const top = Math.max(b.top, Math.min(r.bottom + GAP, b.bottom - height));
      setUp(false);
      setStyle({ position: "fixed", left, minWidth: r.width, top, maxHeight: b.bottom - top });
      return;
    }
    const preferred = placement === "up" ? above : below;
    const other = placement === "up" ? below : above;
    const flip = preferred < height && other > preferred;
    const goUp = placement === "up" ? !flip : flip;
    setUp(goUp);
    setStyle({
      position: "fixed",
      left,
      minWidth: r.width,
      maxHeight: Math.max(96, Math.min(cap, goUp ? above : below)),
      ...(goUp ? { bottom: window.innerHeight - r.top + GAP } : { top: r.bottom + GAP }),
    });
  }, [anchor, panel, placement, align, cap]);

  useLayoutEffect(() => {
    if (!open) return;
    update();
    const onScroll = (e: Event) => {
      if (!panel.current?.contains(e.target as Node)) update();
    };
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", update);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", update);
    };
  }, [open, update, panel]);

  return { style, up };
}

/**
 * Floating panel shared by every control here: portal, positioning, the
 * ModelPicker-style motion, and {@link useDismiss}'s outside-click, blur and
 * Escape dismissal with a hold that keeps the notch open while it is up. It
 * takes focus on open so its `onKeyDown` receives keys.
 */
function Popover({
  open,
  onDismiss,
  anchor,
  panelRef,
  placement = "down",
  align = "start",
  cap = 320,
  className = "",
  onKeyDown,
  children,
  ...aria
}: {
  open: boolean;
  /** Outside click, window blur, or an Escape the panel's own keys didn't handle. */
  onDismiss: () => void;
  anchor: RefObject<HTMLElement | null>;
  panelRef: RefObject<HTMLDivElement | null>;
  placement?: PopoverPlacement;
  align?: PopoverAlign;
  /** Largest height the panel may grow to. */
  cap?: number;
  className?: string;
  onKeyDown?: (e: ReactKeyboardEvent<HTMLDivElement>) => void;
  children: ReactNode;
  id?: string;
  role?: string;
  "aria-label"?: string;
  "aria-activedescendant"?: string;
}) {
  const { style, up } = useFloating(open, anchor, panelRef, placement, align, cap);
  const dismissKey = useDismiss(open, onDismiss, [panelRef, anchor]);

  useEffect(() => {
    if (open) panelRef.current?.focus({ preventScroll: true });
  }, [open, panelRef]);

  return createPortal(
    <AnimatePresence>
      {open && (
        <motion.div
          ref={panelRef}
          tabIndex={-1}
          onKeyDown={(e) => {
            onKeyDown?.(e);
            dismissKey(e);
          }}
          style={style}
          initial={{ opacity: 0, y: up ? 6 : -6, scale: 0.98 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: up ? 6 : -6, scale: 0.98 }}
          transition={{ duration: 0.14, ease: [0.2, 0.8, 0.2, 1] }}
          className={`z-50 overflow-y-auto overscroll-contain rounded-xl surface-menu shadow-[0_16px_48px] shadow-shade/65 ring-1 ring-white/10 outline-none ${
            up ? "origin-bottom" : "origin-top"
          } ${className}`}
          {...aria}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}

/** Scrolls `container` just enough to show `item`, or centers it. */
function reveal(container: HTMLElement | null, item: Element | null | undefined, center = false) {
  // NOTE: scrollIntoView would also scroll <body>, which is overflow-hidden
  // but still programmatically scrollable, and shift the whole notch.
  if (!container || !(item instanceof HTMLElement)) return;
  const top = item.offsetTop;
  const bottom = top + item.offsetHeight;
  if (center) container.scrollTop = top - (container.clientHeight - item.offsetHeight) / 2;
  else if (top < container.scrollTop) container.scrollTop = top;
  else if (bottom > container.scrollTop + container.clientHeight) container.scrollTop = bottom - container.clientHeight;
}

// -----------------------------------------------------------------------------
// Select
// -----------------------------------------------------------------------------

/** One choice in a {@link Select}. */
export interface SelectOption<T> {
  value: T;
  label: string;
  /** Dimmer text after the label, e.g. a display resolution. */
  detail?: string;
  /** Leading color dot, e.g. a project color. */
  dot?: string;
}

/**
 * Finds the option to jump to for type-ahead text. Repeating one letter
 * cycles through the options starting with it, like a native select.
 */
function typeaheadMatch(labels: string[], typed: string, from: number): number {
  const q = typed.toLowerCase();
  const query = [...q].every((c) => c === q[0]) ? q[0]! : q;
  const start = query.length === 1 ? from + 1 : from;
  for (let k = 0; k < labels.length; k++) {
    const i = (start + k) % labels.length;
    if (labels[i]!.toLowerCase().startsWith(query)) return i;
  }
  return -1;
}

/**
 * Dropdown for one value out of a list: a field-style trigger showing the
 * current choice and a popover listbox with a check on the picked row.
 * Up/Down move, Enter or Space pick, Esc closes, and typing jumps to a label.
 *
 * @param props.value - Picked value; when no option matches, the placeholder shows.
 * @param props.options - Choices, in order.
 * @param props.onChange - Called with a newly picked value.
 * @param props.label - Accessible name for the trigger and list.
 * @param props.placeholder - Trigger text when nothing matches `value`.
 * @param props.disabled - Disables the trigger.
 * @param props.placement - Preferred side for the list.
 * @param props.align - Trigger edge the list lines up with.
 * @param props.className - Extra trigger classes, typically a width.
 * @example
 * <Select label="Project" value={id} options={[{ value: "", label: "Inbox" }]} onChange={setId} className="w-full" />
 */
export function Select<T extends string | number | null>({
  value,
  options,
  onChange,
  label,
  placeholder = "Choose…",
  disabled,
  placement = "down",
  align = "start",
  className = "",
}: {
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  label: string;
  placeholder?: string;
  disabled?: boolean;
  placement?: PopoverPlacement;
  align?: PopoverAlign;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const typed = useRef({ text: "", at: 0 });
  const listId = useId();
  const selected = options.findIndex((o) => o.value === value);
  const current = options[selected];

  useEffect(() => {
    if (!open) return;
    // Waits a frame so the popover's measured maxHeight is in place first.
    const frame = requestAnimationFrame(() => reveal(panel.current, panel.current?.querySelector(`[data-index="${active}"]`)));
    return () => cancelAnimationFrame(frame);
  }, [open, active]);

  const show = () => {
    setActive(Math.max(0, selected));
    setOpen(true);
  };
  const close = () => {
    setOpen(false);
    trigger.current?.focus({ preventScroll: true });
  };
  const pick = (i: number) => {
    const o = options[i];
    if (!o) return;
    close();
    if (o.value !== value) onChange(o.value);
  };

  const onListKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const last = options.length - 1;
    const step = (to: number) => {
      e.preventDefault();
      setActive(Math.min(last, Math.max(0, to)));
    };
    if (e.key === "ArrowDown") step(active + 1);
    else if (e.key === "ArrowUp") step(active - 1);
    else if (e.key === "Home" || e.key === "PageUp") step(0);
    else if (e.key === "End" || e.key === "PageDown") step(last);
    else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      pick(active);
    } else if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
      const t = typed.current;
      t.text = e.timeStamp - t.at > 700 ? e.key : t.text + e.key;
      t.at = e.timeStamp;
      const i = typeaheadMatch(
        options.map((o) => o.label),
        t.text,
        active,
      );
      if (i >= 0) setActive(i);
    }
  };

  return (
    <>
      <button
        ref={trigger}
        type="button"
        disabled={disabled}
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        onPointerDown={askForKeys}
        onClick={() => (open ? close() : show())}
        onKeyDown={(e) => {
          if (!open && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
            e.preventDefault();
            show();
          }
        }}
        className={`${TRIGGER} justify-between ${open ? "ring-1 ring-accent/60" : ""} ${className}`}
      >
        <span className="flex min-w-0 items-center gap-2">
          {current?.dot && <span className="size-2 shrink-0 rounded-full" style={{ background: current.dot }} />}
          <span className={`truncate ${current ? "" : "text-white/40"}`}>{current?.label ?? placeholder}</span>
        </span>
        <ChevronDown12Regular className={`size-3 shrink-0 text-white/40 transition-transform ${open ? "rotate-180" : ""}`} />
      </button>

      <Popover
        open={open}
        onDismiss={() => setOpen(false)}
        anchor={trigger}
        panelRef={panel}
        placement={placement}
        align={align}
        cap={280}
        id={listId}
        role="listbox"
        aria-label={label}
        aria-activedescendant={`${listId}-${active}`}
        onKeyDown={onListKey}
        className="scroll-fade flex flex-col p-1"
      >
        {options.map((o, i) => {
          const on = i === selected;
          return (
            <button
              key={String(o.value)}
              id={`${listId}-${i}`}
              data-index={i}
              type="button"
              role="option"
              tabIndex={-1}
              aria-selected={on}
              onMouseDown={keepFocus}
              onPointerMove={() => i !== active && setActive(i)}
              onClick={() => pick(i)}
              className={`flex w-full shrink-0 items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[12.5px] transition-colors ${
                i === active ? "bg-white/[0.07]" : ""
              } ${on ? "text-white" : "text-white/80"}`}
            >
              {o.dot && <span className="size-2 shrink-0 rounded-full" style={{ background: o.dot }} />}
              <span className="min-w-0 flex-1 truncate">
                {o.label}
                {o.detail && <span className="ml-1.5 text-white/40">{o.detail}</span>}
              </span>
              <Checkmark16Regular className={`size-4 shrink-0 text-white/75 ${on ? "" : "invisible"}`} />
            </button>
          );
        })}
      </Popover>
    </>
  );
}

// -----------------------------------------------------------------------------
// DatePicker
// -----------------------------------------------------------------------------

const FIRST_DAY = firstDayOfWeek();
const monthTitle = new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" });
const weekdayName = new Intl.DateTimeFormat(undefined, { weekday: "narrow" });
const fullDate = new Intl.DateTimeFormat(undefined, { weekday: "long", day: "numeric", month: "long", year: "numeric" });
// Nov 15 2026 is a Sunday, so day i of this week has weekday index i.
const WEEKDAYS = Array.from({ length: 7 }, (_, i) => weekdayName.format(new Date(2026, 10, 15 + ((FIRST_DAY + i) % 7))));

/**
 * Friendly label for a stored day: "Today", "Tomorrow", else a short date
 * such as "Fri 21 Nov" in the user's locale.
 */
function dayLabel(day: string, now: Date): { text: string; overdue: boolean } | null {
  const date = parseDay(day);
  if (!date) return null;
  const { text, tone } = dueLabel(day, now);
  return { text: text === "Today" || text === "Tomorrow" ? text : formatDate(date), overdue: tone === "overdue" };
}

/**
 * Due-date field: a trigger with a friendly label opening a month calendar
 * with quick picks. Arrows move by day and week, PageUp/PageDown by month,
 * Enter picks, Esc closes.
 *
 * @param props.value - `YYYY-MM-DD`, or `null` for no date.
 * @param props.onChange - Called with the new day, or `null` when cleared.
 * @param props.label - Accessible name.
 * @param props.placeholder - Trigger text with no date.
 * @param props.clearable - Offer a Clear chip.
 * @param props.placement - Preferred side for the calendar.
 * @param props.align - Trigger edge the calendar lines up with.
 * @param props.className - Extra trigger classes, typically a width.
 * @example
 * <DatePicker label="Due date" value={task.due} onChange={(due) => patch({ due })} className="w-full" />
 */
export function DatePicker({
  value,
  onChange,
  label,
  placeholder = "No date",
  clearable = true,
  placement = "down",
  align = "start",
  className = "",
}: {
  value: string | null;
  onChange: (value: string | null) => void;
  label: string;
  placeholder?: string;
  clearable?: boolean;
  placement?: PopoverPlacement;
  align?: PopoverAlign;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const today = startOfDay(new Date());
  const [cursor, setCursor] = useState(today);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const picked = value ? parseDay(value) : null;
  const shown = value ? dayLabel(value, today) : null;
  const days = monthGrid(cursor.getFullYear(), cursor.getMonth(), FIRST_DAY);
  const tomorrow = addDays(today, 1);
  const nextWeek = startOfWeek(addDays(today, 7), FIRST_DAY);

  const show = () => {
    setCursor(picked ?? today);
    setOpen(true);
  };
  const close = () => {
    setOpen(false);
    trigger.current?.focus({ preventScroll: true });
  };
  const pick = (d: Date | null) => {
    close();
    const next = d ? isoDay(d) : null;
    if (next !== value) onChange(next);
  };

  const onKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const move = (d: Date) => {
      e.preventDefault();
      setCursor(d);
    };
    if (e.key === "ArrowLeft") move(addDays(cursor, -1));
    else if (e.key === "ArrowRight") move(addDays(cursor, 1));
    else if (e.key === "ArrowUp") move(addDays(cursor, -7));
    else if (e.key === "ArrowDown") move(addDays(cursor, 7));
    else if (e.key === "PageUp") move(addMonths(cursor, e.shiftKey ? -12 : -1));
    else if (e.key === "PageDown") move(addMonths(cursor, e.shiftKey ? 12 : 1));
    else if (e.key === "Home") move(startOfWeek(cursor, FIRST_DAY));
    else if (e.key === "End") move(addDays(startOfWeek(cursor, FIRST_DAY), 6));
    else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      pick(cursor);
    } else if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  };

  const quick = (text: string, d: Date | null, title?: string) => {
    const on = d ? picked !== null && sameDay(d, picked) : false;
    return (
      <button
        type="button"
        tabIndex={-1}
        title={title}
        onMouseDown={keepFocus}
        onClick={() => pick(d)}
        className={`h-6 flex-1 rounded-md px-1 text-[11px] whitespace-nowrap transition-colors ${
          on ? "bg-accent/18 text-accent" : "bg-white/[0.07] text-white/70 hover:bg-white/12 hover:text-white"
        }`}
      >
        {text}
      </button>
    );
  };

  return (
    <>
      <button
        ref={trigger}
        type="button"
        aria-label={shown ? `${label}: ${shown.text}` : label}
        aria-haspopup="dialog"
        aria-expanded={open}
        onPointerDown={askForKeys}
        onClick={() => (open ? close() : show())}
        onKeyDown={(e) => {
          if (!open && e.key === "ArrowDown") {
            e.preventDefault();
            show();
          }
        }}
        className={`${TRIGGER} ${open ? "ring-1 ring-accent/60" : ""} ${className}`}
      >
        <CalendarLtr16Regular className={`size-3.5 shrink-0 ${shown?.overdue ? "text-status-red/80" : "text-white/40"}`} />
        <span className={`truncate ${shown ? (shown.overdue ? "text-status-red" : "") : "text-white/40"}`}>
          {shown?.text ?? placeholder}
        </span>
      </button>

      <Popover
        open={open}
        onDismiss={() => setOpen(false)}
        anchor={trigger}
        panelRef={panel}
        placement={placement}
        align={align}
        cap={400}
        role="dialog"
        aria-label={label}
        onKeyDown={onKey}
        className="w-[232px] p-2"
      >
        <div className="flex items-center justify-between pb-1.5 pl-1.5">
          <span className="text-[12.5px] font-semibold text-white/90" aria-live="polite">
            {monthTitle.format(cursor)}
          </span>
          <span className="flex">
            <IconButton
              icon={ChevronLeft16Regular}
              label="Previous month"
              tabIndex={-1}
              onMouseDown={keepFocus}
              onClick={() => setCursor(addMonths(cursor, -1))}
            />
            <IconButton
              icon={ChevronRight16Regular}
              label="Next month"
              tabIndex={-1}
              onMouseDown={keepFocus}
              onClick={() => setCursor(addMonths(cursor, 1))}
            />
          </span>
        </div>

        <div className="grid grid-cols-7 gap-0.5">
          {WEEKDAYS.map((w, i) => (
            <span key={i} aria-hidden className="grid h-6 place-items-center text-[10.5px] font-medium text-white/35">
              {w}
            </span>
          ))}
          {days.map((d) => {
            const isPicked = picked !== null && sameDay(d, picked);
            const isToday = sameDay(d, today);
            const inMonth = d.getMonth() === cursor.getMonth();
            const isCursor = sameDay(d, cursor);
            return (
              <button
                key={d.getTime()}
                type="button"
                tabIndex={-1}
                aria-label={fullDate.format(d)}
                aria-pressed={isPicked}
                aria-current={isToday ? "date" : undefined}
                onMouseDown={keepFocus}
                onClick={() => pick(d)}
                className={`relative grid h-7 place-items-center rounded-lg text-[12px] tabular-nums transition-colors ${
                  isPicked
                    ? "bg-accent font-semibold text-on-accent"
                    : `${isCursor ? "bg-white/10 ring-1 ring-white/20 ring-inset" : "hover:bg-white/[0.07]"} ${
                        isToday ? "font-semibold text-accent" : inMonth ? "text-white/85" : "text-white/25"
                      }`
                }`}
              >
                {d.getDate()}
                {isToday && <span className={`absolute bottom-[3px] size-[3px] rounded-full ${isPicked ? "bg-on-accent" : "bg-accent"}`} />}
              </button>
            );
          })}
        </div>

        <div className="mt-2 flex gap-1 border-t border-white/[0.07] pt-2">
          {quick("Today", today)}
          {quick("Tomorrow", tomorrow)}
          {quick("Next week", nextWeek, formatDate(nextWeek))}
          {clearable && value && quick("Clear", null)}
        </div>
      </Popover>
    </>
  );
}

// -----------------------------------------------------------------------------
// TimePicker
// -----------------------------------------------------------------------------

interface Clock {
  h: number;
  m: number;
}

const pad = (n: number) => String(n).padStart(2, "0");
const HOURS = Array.from({ length: 24 }, (_, h) => h);

function parseClock(value: string | null): Clock | null {
  const m = value ? /^(\d{1,2}):(\d{2})/.exec(value) : null;
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  return h < 24 && min < 60 ? { h, m: min } : null;
}

/**
 * Time-of-day field: a trigger opening hour and minute columns. Clicking an
 * hour then a minute commits; Up/Down step the focused column, Left/Right
 * switch columns, Enter or Tab commits, Esc discards. Dismissing by clicking
 * elsewhere keeps what was picked so far.
 *
 * @param props.value - `HH:MM` (24-hour), or `null` for no time.
 * @param props.onChange - Called with the new `HH:MM`, or `null` when cleared.
 * @param props.label - Accessible name.
 * @param props.placeholder - Trigger text with no time.
 * @param props.minuteStep - Spacing of the minute column. A stored minute off
 *   the step is still listed so it stays visible.
 * @param props.clearable - Offer a Clear button.
 * @param props.placement - Preferred side for the popover.
 * @param props.align - Trigger edge the popover lines up with.
 * @example
 * <TimePicker label="Window start" value="22:00" onChange={setStart} />
 */
export function TimePicker({
  value,
  onChange,
  label,
  placeholder = "--:--",
  minuteStep = 5,
  clearable = true,
  placement = "down",
  align = "start",
}: {
  value: string | null;
  onChange: (value: string | null) => void;
  label: string;
  placeholder?: string;
  minuteStep?: number;
  clearable?: boolean;
  placement?: PopoverPlacement;
  align?: PopoverAlign;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<Clock | null>(null);
  const [column, setColumn] = useState<0 | 1>(0);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const current = parseClock(value);
  const minutes = Array.from({ length: Math.ceil(60 / minuteStep) }, (_, i) => i * minuteStep);
  const shownMinute = (draft ?? current)?.m;
  if (shownMinute !== undefined && !minutes.includes(shownMinute)) {
    minutes.push(shownMinute);
    minutes.sort((a, b) => a - b);
  }

  const show = () => {
    setDraft(current);
    setColumn(0);
    setOpen(true);
  };
  const finish = (clock: Clock | null, refocus: boolean) => {
    setOpen(false);
    if (refocus) trigger.current?.focus({ preventScroll: true });
    const next = clock ? `${pad(clock.h)}:${pad(clock.m)}` : null;
    if (clock && next !== value) onChange(next);
  };

  const onKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const base = draft ?? { h: new Date().getHours(), m: 0 };
    const step = (dir: 1 | -1) => {
      e.preventDefault();
      if (column === 0) setDraft({ ...base, h: (base.h + dir + 24) % 24 });
      else {
        const i = minutes.indexOf(base.m);
        setDraft({ ...base, m: minutes[(i + dir + minutes.length) % minutes.length]! });
      }
    };
    if (e.key === "ArrowDown") step(1);
    else if (e.key === "ArrowUp") step(-1);
    else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      setColumn(e.key === "ArrowLeft" ? 0 : 1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      finish(draft, true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
      trigger.current?.focus({ preventScroll: true });
    } else if (e.key === "Tab") {
      e.preventDefault();
      finish(draft, true);
    }
  };

  return (
    <>
      <button
        ref={trigger}
        type="button"
        aria-label={value ? `${label}: ${value}` : label}
        aria-haspopup="dialog"
        aria-expanded={open}
        onPointerDown={askForKeys}
        onClick={() => (open ? finish(draft, false) : show())}
        onKeyDown={(e) => {
          if (!open && e.key === "ArrowDown") {
            e.preventDefault();
            show();
          }
        }}
        className={`${TRIGGER} gap-1.5 px-2 tabular-nums ${open ? "ring-1 ring-accent/60" : ""}`}
      >
        <Clock16Regular className="size-3.5 shrink-0 text-white/40" />
        <span className={value ? "" : "text-white/40"}>{value ?? placeholder}</span>
      </button>

      <Popover
        open={open}
        onDismiss={() => finish(draft, false)}
        anchor={trigger}
        panelRef={panel}
        placement={placement}
        align={align}
        role="dialog"
        aria-label={label}
        onKeyDown={onKey}
        className="flex flex-col p-1.5"
      >
        <div className="flex gap-1">
          <TimeColumn
            title="Hour"
            values={HOURS}
            picked={draft?.h ?? null}
            focused={column === 0}
            onPick={(h) => {
              setDraft({ h, m: draft?.m ?? 0 });
              setColumn(1);
            }}
          />
          <TimeColumn
            title="Min"
            values={minutes}
            picked={draft?.m ?? null}
            focused={column === 1}
            onPick={(m) => finish({ h: draft?.h ?? current?.h ?? 0, m }, true)}
          />
        </div>
        {clearable && value && (
          <button
            type="button"
            tabIndex={-1}
            onMouseDown={keepFocus}
            onClick={() => {
              setOpen(false);
              trigger.current?.focus({ preventScroll: true });
              onChange(null);
            }}
            className="mt-1.5 h-6 rounded-md text-[11.5px] text-white/55 hover:bg-white/8 hover:text-white/90"
          >
            Clear
          </button>
        )}
      </Popover>
    </>
  );
}

function TimeColumn({
  title,
  values,
  picked,
  focused,
  onPick,
}: {
  title: string;
  values: number[];
  picked: number | null;
  focused: boolean;
  onPick: (v: number) => void;
}) {
  const list = useRef<HTMLDivElement>(null);
  const opened = useRef(false);

  useLayoutEffect(() => {
    const item = list.current?.querySelector('[aria-selected="true"]');
    reveal(list.current, item, !opened.current);
    if (item) opened.current = true;
  }, [picked]);

  return (
    <div className="flex w-14 flex-col">
      <span
        className={`pb-1 text-center text-[10.5px] font-medium tracking-wide uppercase transition-colors ${focused ? "text-white/60" : "text-white/30"}`}
      >
        {title}
      </span>
      <div
        ref={list}
        role="listbox"
        aria-label={title}
        className="scroll-fade relative flex max-h-44 flex-col gap-px overflow-y-auto overscroll-contain"
      >
        {values.map((v) => {
          const on = v === picked;
          return (
            <button
              key={v}
              type="button"
              role="option"
              tabIndex={-1}
              aria-selected={on}
              onMouseDown={keepFocus}
              onClick={() => onPick(v)}
              className={`h-7 shrink-0 rounded-md text-[12.5px] tabular-nums transition-colors ${
                on ? "bg-accent font-semibold text-on-accent" : "text-white/75 hover:bg-white/[0.07] hover:text-white"
              }`}
            >
              {pad(v)}
            </button>
          );
        })}
      </div>
    </div>
  );
}

// -----------------------------------------------------------------------------
// Checkbox
// -----------------------------------------------------------------------------

/**
 * Square checkbox that fills with the accent and draws its check when on.
 * Space toggles. Inside a `<label>`, clicking the label text toggles it too.
 *
 * @param props.checked - Current state.
 * @param props.onChange - Called with the new state.
 * @param props.disabled - Disables toggling.
 * @param props.label - Accessible name when no wrapping `<label>` provides one.
 * @param props.className - Extra classes, e.g. a top margin to align with text.
 * @example
 * <label className="flex items-center gap-2"><Checkbox checked={on} onChange={setOn} /> Include</label>
 */
export function Checkbox({
  checked,
  onChange,
  disabled,
  label,
  className = "",
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: string;
  className?: string;
}) {
  return (
    <motion.button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      whileTap={disabled ? undefined : { scale: 0.88 }}
      onClick={() => onChange(!checked)}
      onKeyDown={(e) => {
        // Checkboxes toggle on Space only; Enter would also click a button.
        if (e.key === "Enter") e.preventDefault();
      }}
      className={`grid size-4 shrink-0 place-items-center rounded-[5px] text-on-accent outline-none transition-colors focus-visible:ring-2 focus-visible:ring-white/40 disabled:opacity-40 ${
        checked ? "bg-accent" : "bg-white/6 ring-1 ring-white/25 ring-inset hover:ring-white/45"
      } ${className}`}
    >
      <svg viewBox="0 0 16 16" fill="none" aria-hidden className="size-3">
        <motion.path
          d="M3.5 8.5l3 3 6-7"
          stroke="currentColor"
          strokeWidth={2}
          strokeLinecap="round"
          strokeLinejoin="round"
          initial={false}
          animate={{ pathLength: checked ? 1 : 0, opacity: checked ? 1 : 0 }}
          transition={{ duration: 0.18, ease: [0.2, 0.8, 0.2, 1] }}
        />
      </svg>
    </motion.button>
  );
}

/**
 * Single-choice button group with a thumb that slides to the picked option.
 * Arrow keys move the choice; each option can carry a tooltip.
 *
 * @param props.options - Choices, in order. `title` becomes the tooltip.
 * @param props.value - Picked value.
 * @param props.onChange - Called with a newly picked value.
 * @param props.fill - Stretch to the parent's width with equal-width options.
 * @param props.label - Accessible name for the group.
 */
export function Segmented<T extends string | number>({
  options,
  value,
  onChange,
  fill = false,
  label,
}: {
  options: { value: T; label: string; title?: string }[];
  value: T;
  onChange: (v: T) => void;
  fill?: boolean;
  label?: string;
}) {
  const id = useId();
  const index = options.findIndex((o) => o.value === value);
  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    e.preventDefault();
    const next = options[Math.min(options.length - 1, Math.max(0, index + (e.key === "ArrowRight" ? 1 : -1)))];
    if (next && next.value !== value) onChange(next.value);
  };
  return (
    <div
      role="radiogroup"
      aria-label={label}
      tabIndex={0}
      onKeyDown={onKeyDown}
      className={`relative gap-px rounded-[9px] bg-white/[0.055] p-[3px] ring-1 ring-white/[0.05] outline-none focus-visible:ring-white/25 ${
        fill ? "flex w-full" : "inline-flex"
      }`}
    >
      {options.map((o) => {
        const on = o.value === value;
        return (
          <button
            key={String(o.value)}
            type="button"
            role="radio"
            aria-checked={on}
            tabIndex={-1}
            title={o.title}
            onClick={() => !on && onChange(o.value)}
            className={`relative h-[22px] min-w-8 rounded-md text-[11.5px] whitespace-nowrap transition-colors ${
              fill ? "flex-1 px-1" : "px-2.5"
            } ${on ? "text-white" : "text-white/50 hover:text-white/85"}`}
          >
            {on && (
              <motion.span
                layoutId={`seg-${id}`}
                transition={{ type: "spring", stiffness: 500, damping: 38 }}
                className="absolute inset-0 rounded-md bg-thumb shadow-(--shadow-thumb)"
              />
            )}
            <span className="relative">{o.label}</span>
          </button>
        );
      })}
    </div>
  );
}

/**
 * On/off switch.
 *
 * @param props.checked - Current state.
 * @param props.onChange - Called with the new state.
 */
export function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={(e) => {
        e.preventDefault();
        onChange(!checked);
      }}
      className={`relative h-5 w-8.5 shrink-0 rounded-full transition-colors ${checked ? "bg-accent" : "bg-white/15"}`}
    >
      <motion.span
        className="absolute top-0.5 left-0.5 size-4 rounded-full bg-knob shadow"
        animate={{ x: checked ? 14 : 0 }}
        transition={{ type: "spring", duration: 0.25, bounce: 0.2 }}
      />
    </button>
  );
}
