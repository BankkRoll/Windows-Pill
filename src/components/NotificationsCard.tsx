import { ArrowClockwise16Regular, ChevronDown12Regular, ChevronUp12Regular, Dismiss12Regular } from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useState } from "react";
import { useClock } from "../hooks/useClock";
import { attempt, openOutside, openSystemSettings, reportError } from "../lib/actions";
import { notificationsApi, pluginApi, type NotificationItem, type PluginState } from "../lib/native";
import {
  cardMode,
  COLLAPSED_ROWS,
  listedNotes,
  noteLines,
  NOTIFICATIONS_PLUGIN,
  pruneHidden,
  relativeTime,
  remainingTotal,
} from "../lib/notifications";
import { usePillStore } from "../store";

const SMALL_BUTTON =
  "inline-flex h-6 items-center gap-1 rounded-md px-2 text-[11.5px] text-white/60 hover:bg-white/8 hover:text-white disabled:opacity-40";

/**
 * Whether the Today tab has a notifications card to show.
 *
 * @returns `false` while the Windows notifications plugin is off or starting.
 */
export function useHasNotificationsCard(): boolean {
  const plugin = usePillStore((s) => s.plugins.find((p) => p.id === NOTIFICATIONS_PLUGIN));
  const feed = usePillStore((s) => s.notifications);
  return cardMode(plugin, feed) !== "hidden";
}

/**
 * The Windows notifications card on the Today tab: Action Center's
 * notifications newest first, each dismissable, with Clear all. Dismissing
 * hides the row at once and brings it back with an error toast if Windows
 * refuses. Explains itself when Windows blocks access.
 */
export function NotificationsCard() {
  const plugin = usePillStore((s) => s.plugins.find((p) => p.id === NOTIFICATIONS_PLUGIN));
  const feed = usePillStore((s) => s.notifications);
  const [hidden, setHidden] = useState<ReadonlySet<number>>(() => new Set());
  const [expanded, setExpanded] = useState(false);
  const [clearing, setClearing] = useState(false);
  const now = useClock().getTime();

  useEffect(() => setHidden((h) => pruneHidden(h, feed)), [feed]);

  const mode = cardMode(plugin, feed);
  if (mode === "hidden" || !plugin) return null;
  if (mode === "blocked") return <Blocked plugin={plugin} />;

  const notes = listedNotes(feed, hidden);
  const total = remainingTotal(feed, hidden);
  const shown = expanded ? notes : notes.slice(0, COLLAPSED_ROWS);

  const hide = (ids: readonly number[], on: boolean) =>
    setHidden((h) => {
      const next = new Set(h);
      for (const id of ids) {
        if (on) next.add(id);
        else next.delete(id);
      }
      return next;
    });

  const dismiss = async (note: NotificationItem) => {
    hide([note.id], true);
    try {
      await notificationsApi.dismiss(note.id);
    } catch (e) {
      hide([note.id], false);
      reportError(e);
    }
  };

  const clearAll = async () => {
    const ids = notes.map((n) => n.id);
    hide(ids, true);
    setClearing(true);
    try {
      await notificationsApi.clear();
      setExpanded(false);
    } catch (e) {
      hide(ids, false);
      reportError(e);
    } finally {
      setClearing(false);
    }
  };

  return (
    <section
      className="flex min-w-0 flex-col rounded-xl bg-white/[0.045] px-1.5 pt-1.5 pb-1 ring-1 ring-white/[0.06]"
      aria-label="Notifications"
    >
      <header className="flex h-6 items-center justify-between gap-2 pr-0.5 pl-1.5">
        <h3 className="flex items-baseline gap-1.5 text-[12px] font-semibold text-white/85">
          Notifications
          {total > 0 && <span className="text-[11px] font-normal text-white/40 tabular-nums">{total}</span>}
        </h3>
        {notes.length > 0 && (
          <button type="button" disabled={clearing} onClick={() => void clearAll()} className={SMALL_BUTTON}>
            Clear all
          </button>
        )}
      </header>

      {notes.length === 0 ? (
        <p className="px-1.5 pt-0.5 pb-1.5 text-[11.5px] text-white/40">Action Center is empty.</p>
      ) : (
        <ul
          className={`flex flex-col ${expanded ? "scroll-fade max-h-[244px] overflow-y-auto overscroll-contain" : ""}`}
          aria-label={`${total} notifications`}
        >
          <AnimatePresence initial={false}>
            {shown.map((note) => (
              <Row key={note.id} note={note} now={now} onDismiss={() => void dismiss(note)} />
            ))}
          </AnimatePresence>
        </ul>
      )}

      {notes.length > COLLAPSED_ROWS && (
        <div className="flex items-center justify-between gap-2 pl-0.5">
          <button type="button" onClick={() => setExpanded((x) => !x)} className={SMALL_BUTTON} aria-expanded={expanded}>
            {expanded ? <ChevronUp12Regular className="size-3" /> : <ChevronDown12Regular className="size-3" />}
            {expanded ? "Show less" : `Show all ${notes.length}`}
          </button>
          {expanded && total > notes.length && (
            <span className="pr-1.5 text-[10.5px] text-white/35">
              Latest {notes.length} of {total}
            </span>
          )}
        </div>
      )}
    </section>
  );
}

function Row({ note, now, onDismiss }: { note: NotificationItem; now: number; onDismiss: () => void }) {
  const { headline, body } = noteLines(note);
  const age = relativeTime(note.createdMs, now);
  return (
    <motion.li
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: "auto" }}
      exit={{ opacity: 0, height: 0 }}
      transition={{ duration: 0.18, ease: "easeOut" }}
      className="shrink-0 overflow-hidden"
    >
      <div className="group flex items-start gap-2.5 rounded-lg px-1.5 py-1.5 hover:bg-white/[0.04]">
        <AppIcon note={note} />
        <div className="min-w-0 flex-1">
          <div className="flex h-4 items-center gap-2">
            <span className="min-w-0 flex-1 truncate text-[12px] font-medium text-white/90" title={headline}>
              {headline}
            </span>
            <span className="max-w-[40%] shrink-0 truncate text-[11px] text-white/40">{note.app}</span>
            <span className="relative grid h-4 min-w-6 shrink-0 place-items-center">
              <time
                dateTime={new Date(note.createdMs).toISOString()}
                className="text-[10.5px] text-white/35 tabular-nums transition-opacity group-focus-within:opacity-0 group-hover:opacity-0"
              >
                {age}
              </time>
              <button
                type="button"
                onClick={onDismiss}
                title="Dismiss"
                aria-label={`Dismiss ${note.app} notification`}
                className="absolute inset-y-[-4px] right-[-4px] grid w-6 place-items-center rounded-md text-white/55 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-white/10 hover:text-white focus-visible:opacity-100"
              >
                <Dismiss12Regular className="size-3" />
              </button>
            </span>
          </div>
          {body && <p className="mt-0.5 line-clamp-2 text-[11.5px] leading-snug text-white/55">{body}</p>}
          {note.screenshot && (
            <button
              type="button"
              onClick={() => void openOutside(() => notificationsApi.openScreenshot(note.id))}
              title="Open screenshot"
              aria-label="Open screenshot"
              className="mt-1.5 block overflow-hidden rounded-md ring-1 ring-white/[0.08] transition-opacity hover:opacity-85"
            >
              <img src={note.screenshot} alt="" className="h-[60px] w-24 object-cover" draggable={false} />
            </button>
          )}
        </div>
      </div>
    </motion.li>
  );
}

function AppIcon({ note }: { note: NotificationItem }) {
  if (note.icon) return <img src={note.icon} alt="" className="mt-px size-6 shrink-0 rounded-md object-contain" draggable={false} />;
  return (
    <span
      className="mt-px grid size-6 shrink-0 place-items-center rounded-md bg-white/8 text-[11px] font-semibold text-white/60"
      aria-hidden
    >
      {note.app.trim().charAt(0).toUpperCase() || "?"}
    </span>
  );
}

function Blocked({ plugin }: { plugin: PluginState }) {
  const fix = plugin.status.fix;
  return (
    <section
      className="col-span-full flex items-center gap-3 rounded-xl bg-white/[0.045] px-3 py-2.5 ring-1 ring-white/[0.06]"
      aria-label="Notifications"
    >
      <div className="min-w-0 flex-1">
        <h3 className="text-[12px] font-semibold text-white/85">Notifications</h3>
        <p className="text-[11.5px] leading-snug text-white/50">
          {plugin.status.message ?? "Windows isn't letting Pill read notifications."}
        </p>
      </div>
      {fix && (
        <button
          type="button"
          onClick={() => {
            if (fix.kind === "settings") openSystemSettings(fix.page);
            else if (fix.kind === "action") void attempt(() => pluginApi.action(plugin.id, fix.id));
            else void attempt(() => pluginApi.restart(plugin.id));
          }}
          className="inline-flex h-6 shrink-0 items-center gap-1 rounded-md bg-white/7 px-2 text-[11.5px] text-white/80 hover:bg-white/12 hover:text-white"
        >
          {fix.kind === "restart" && <ArrowClockwise16Regular className="size-3" />}
          {fix.kind === "restart" ? "Restart" : fix.label}
        </button>
      )}
    </section>
  );
}
