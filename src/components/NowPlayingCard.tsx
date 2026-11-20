import {
  Apps16Regular,
  ArrowRepeat116Regular,
  ArrowRepeatAll16Regular,
  ArrowShuffle16Regular,
  Checkmark12Regular,
  ChevronDown12Regular,
  MusicNote216Filled,
  Next16Filled,
  Pause16Filled,
  Play16Filled,
  Previous16Filled,
  Speaker016Regular,
  Speaker116Regular,
  Speaker216Regular,
  SpeakerMute16Regular,
  type FluentIcon,
} from "@fluentui/react-icons";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { useNow } from "../hooks/useNow";
import { useNowPlaying } from "../hooks/useNowPlaying";
import { attempt } from "../lib/actions";
import { artworkWidth, formatClock, groupSessions, keySeek, positionAt, repeatLabel, sameTimeline, trackFraction } from "../lib/media";
import {
  mediaApi,
  pluginApi,
  type AppVolume,
  type MediaSession,
  type MediaTimeline,
  type MediaWidget,
  type NowPlaying,
} from "../lib/native";
import { usePillStore } from "../store";

/** Artwork box height in CSS px; the core sends artwork at twice this. */
const ART_HEIGHT = 56;
/** How long a seek's position is trusted before the player has to confirm it. */
const SEEK_GRACE_MS = 2_000;
/** Keyboard seeks wait this long for more key presses before going out. */
const KEY_SEEK_DELAY_MS = 200;
/** While dragging the volume, at most one change goes out per this many ms. */
const VOLUME_SEND_MS = 60;
/** How long a volume change is trusted before the mixer has to confirm it. */
const VOLUME_GRACE_MS = 1_500;

function send(pluginId: string, action: string) {
  void attempt(() => pluginApi.action(pluginId, action));
}

/**
 * The Now playing card on the Today tab. The player (Chrome, Spotify...) is
 * the card's header and doubles as the switcher between players; below it the
 * artwork, a two-line title and the artist, then a seek bar when the player
 * supports seeking, then one row of controls: transport on the left, shuffle
 * and repeat when the player offers them, and the player's volume on the right.
 *
 * @param props.pluginId - Plugin whose actions the transport buttons send.
 * @param props.media - The plugin's widget: track, artwork and transport state.
 */
export function NowPlayingCard({ pluginId, media }: { pluginId: string; media: MediaWidget }) {
  const detail = useNowPlaying();
  const timeline = detail?.timeline ?? null;
  return (
    <section
      className="flex flex-col gap-2.5 rounded-xl bg-white/[0.045] px-3 pt-2 pb-2.5 ring-1 ring-white/[0.06]"
      aria-label="Now playing"
    >
      <SessionSwitch app={media.app} detail={detail} playing={media.playing} />
      <div className="flex items-center gap-3">
        <Artwork src={media.artwork} aspect={detail?.artworkAspect ?? 1} />
        <div className="min-w-0 flex-1">
          <p
            className="line-clamp-2 text-[13px] leading-snug font-semibold break-words text-white/92"
            title={media.album ? `${media.title} · ${media.album}` : media.title}
          >
            {media.title}
          </p>
          {media.artist && <p className="mt-0.5 truncate text-[11.5px] text-white/50">{media.artist}</p>}
        </div>
      </div>
      {timeline && <SeekBar key={detail?.session} timeline={timeline} playing={media.playing} />}
      <div className="flex h-8 items-center gap-1">
        <Control icon={Previous16Filled} label="Previous" disabled={!media.canPrevious} onClick={() => send(pluginId, "previous")} />
        <Control
          icon={media.playing ? Pause16Filled : Play16Filled}
          label={media.playing ? "Pause" : "Play"}
          disabled={!media.canPlayPause}
          onClick={() => send(pluginId, "play-pause")}
          primary
        />
        <Control icon={Next16Filled} label="Next" disabled={!media.canNext} onClick={() => send(pluginId, "next")} />
        {(detail?.shuffle != null || detail?.repeat != null) && <span aria-hidden className="w-1.5" />}
        {detail?.shuffle != null && (
          <Toggle
            icon={ArrowShuffle16Regular}
            label={detail.shuffle ? "Shuffle on" : "Shuffle off"}
            on={detail.shuffle}
            onClick={() => void attempt(mediaApi.toggleShuffle)}
          />
        )}
        {detail?.repeat != null && (
          <Toggle
            icon={detail.repeat === "track" ? ArrowRepeat116Regular : ArrowRepeatAll16Regular}
            label={repeatLabel(detail.repeat)}
            on={detail.repeat !== "none"}
            onClick={() => void attempt(mediaApi.cycleRepeat)}
          />
        )}
        <span className="flex-1" />
        {detail?.volume && <Volume key={detail.session} volume={detail.volume} app={media.app} />}
      </div>
    </section>
  );
}

/**
 * Track artwork in a fixed-height box whose width follows the image's shape.
 * A new image fades in over the old one; the same image never reloads.
 */
function Artwork({ src, aspect }: { src: string | null; aspect: number }) {
  const still = useReducedMotion() ?? false;
  const width = artworkWidth(src ? aspect : 1, ART_HEIGHT);
  return (
    <motion.div
      className="relative h-14 shrink-0 overflow-hidden rounded-lg bg-white/8"
      initial={false}
      animate={{ width }}
      style={{ width }}
      transition={{ duration: still ? 0 : 0.2, ease: [0.2, 0.8, 0.2, 1] }}
    >
      <AnimatePresence initial={false}>
        <motion.div
          key={src ?? "none"}
          className="absolute inset-0 grid place-items-center"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: still ? 0 : 0.25 }}
        >
          {src ? (
            <img src={src} alt="" className="size-full object-cover" draggable={false} />
          ) : (
            <MusicNote216Filled className="size-5 text-white/40" />
          )}
        </motion.div>
      </AnimatePresence>
    </motion.div>
  );
}

function Control({
  icon: Icon,
  label,
  disabled,
  onClick,
  primary = false,
}: {
  icon: FluentIcon;
  label: string;
  disabled: boolean;
  onClick: () => void;
  primary?: boolean;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className={`grid place-items-center rounded-full transition-colors outline-none focus-visible:ring-1 focus-visible:ring-white/40 disabled:opacity-30 ${
        primary ? "size-8 bg-white/90 text-black/85 hover:bg-white" : "size-7 text-white/70 hover:bg-white/8 hover:text-white"
      }`}
    >
      <Icon className="size-4" />
    </button>
  );
}

function Toggle({ icon: Icon, label, on, onClick }: { icon: FluentIcon; label: string; on: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={on}
      onClick={onClick}
      className={`grid size-7 shrink-0 place-items-center rounded-full transition-colors outline-none hover:bg-white/8 focus-visible:ring-1 focus-visible:ring-white/40 ${
        on ? "text-accent" : "text-white/40 hover:text-white/75"
      }`}
    >
      <Icon className="size-3.5" />
    </button>
  );
}

/** A seek the user made, shown until the player reports a new position. */
interface PendingSeek {
  ms: number;
  at: number;
  /** Timeline on screen when the seek went out. */
  from: MediaTimeline;
}

/**
 * Seek bar with the position and length. Drag or click to seek; arrows,
 * Page Up/Down, Home and End work from the keyboard. Between the player's
 * reports the position advances locally.
 */
function SeekBar({ timeline, playing }: { timeline: MediaTimeline; playing: boolean }) {
  const now = useNow(playing ? 250 : 60_000);
  const hold = usePillStore((s) => s.hold);
  const holdId = useId();
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const sendTimer = useRef<number | undefined>(undefined);
  const [drag, setDrag] = useState<number | null>(null);
  const [pending, setPending] = useState<PendingSeek | null>(null);
  const duration = timeline.durationMs;

  useEffect(() => {
    if (pending && !sameTimeline(pending.from, timeline)) setPending(null);
  }, [pending, timeline]);

  useEffect(() => {
    if (!pending) return;
    const id = window.setTimeout(() => setPending(null), SEEK_GRACE_MS);
    return () => window.clearTimeout(id);
  }, [pending]);

  useEffect(
    () => () => {
      window.clearTimeout(sendTimer.current);
      hold(`seek${holdId}`, false);
    },
    [hold, holdId],
  );

  const settled = pending
    ? Math.min(duration, pending.ms + (playing ? Math.max(0, now - pending.at) * timeline.rate : 0))
    : positionAt(timeline, playing, now);
  const position = drag ?? settled;
  const percent = duration > 0 ? (position / duration) * 100 : 0;

  const commit = (ms: number, delay: number) => {
    setPending({ ms, at: Date.now(), from: timeline });
    window.clearTimeout(sendTimer.current);
    sendTimer.current = window.setTimeout(() => void attempt(() => mediaApi.seek(ms)), delay);
  };

  const msAt = (clientX: number) => {
    const r = trackRef.current?.getBoundingClientRect();
    return r ? trackFraction(clientX, r.left, r.width) * duration : position;
  };

  const endDrag = (commitAt: number | null) => {
    if (!dragging.current) return;
    dragging.current = false;
    hold(`seek${holdId}`, false);
    setDrag(null);
    if (commitAt !== null) commit(commitAt, 0);
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.focus({ preventScroll: true });
    e.currentTarget.setPointerCapture(e.pointerId);
    dragging.current = true;
    // NOTE: the pointer can leave the notch mid-drag; holding keeps it open.
    hold(`seek${holdId}`, true);
    setDrag(msAt(e.clientX));
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const target = keySeek(e.key, position, duration);
    if (target === null) return;
    e.preventDefault();
    e.stopPropagation();
    commit(target, KEY_SEEK_DELAY_MS);
  };

  return (
    <div className="flex flex-col gap-0.5">
      <div
        role="slider"
        tabIndex={0}
        aria-label="Seek"
        aria-valuemin={0}
        aria-valuemax={Math.round(duration / 1000)}
        aria-valuenow={Math.round(position / 1000)}
        aria-valuetext={`${formatClock(position)} of ${formatClock(duration)}`}
        onPointerDown={onPointerDown}
        onPointerMove={(e) => dragging.current && setDrag(msAt(e.clientX))}
        onPointerUp={(e) => endDrag(msAt(e.clientX))}
        onPointerCancel={() => endDrag(null)}
        onLostPointerCapture={() => endDrag(null)}
        onKeyDown={onKeyDown}
        className="group relative flex h-3 w-full cursor-pointer touch-none items-center rounded outline-none focus-visible:ring-1 focus-visible:ring-white/40"
      >
        <div ref={trackRef} className="relative h-1 w-full rounded-full bg-white/12">
          <div className="absolute inset-y-0 left-0 rounded-full bg-white/70" style={{ width: `${percent}%` }} />
          <div
            aria-hidden
            className={`absolute top-1/2 size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white shadow-sm transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100 ${
              drag !== null ? "opacity-100" : "opacity-0"
            }`}
            style={{ left: `${percent}%` }}
          />
        </div>
      </div>
      <div className="flex justify-between text-[10.5px] text-white/40 tabular-nums">
        <span>{formatClock(position)}</span>
        <span>{formatClock(duration)}</span>
      </div>
    </div>
  );
}

/**
 * The player's own volume in the Windows mixer: a mute toggle and a short
 * slider. Changes go out while dragging, throttled, and the slider keeps the
 * user's value until the mixer reports it back.
 */
function Volume({ volume, app }: { volume: AppVolume; app: string }) {
  const hold = usePillStore((s) => s.hold);
  const holdId = useId();
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const lastSent = useRef(0);
  const [local, setLocal] = useState<{ level: number; at: number } | null>(null);

  useEffect(() => {
    if (!local) return;
    const id = window.setTimeout(() => setLocal(null), VOLUME_GRACE_MS);
    return () => window.clearTimeout(id);
  }, [local]);

  useEffect(() => () => hold(`volume${holdId}`, false), [hold, holdId]);

  const level = local?.level ?? (volume.muted ? 0 : volume.level);
  const percent = Math.round(level * 100);
  const Icon =
    volume.muted || level === 0
      ? SpeakerMute16Regular
      : level < 0.34
        ? Speaker016Regular
        : level < 0.67
          ? Speaker116Regular
          : Speaker216Regular;

  const set = (next: number, force: boolean) => {
    const clamped = Math.min(1, Math.max(0, next));
    setLocal({ level: clamped, at: Date.now() });
    const now = Date.now();
    if (force || now - lastSent.current >= VOLUME_SEND_MS) {
      lastSent.current = now;
      void attempt(() => mediaApi.setVolume(clamped));
    }
  };

  const levelAt = (clientX: number) => {
    const r = trackRef.current?.getBoundingClientRect();
    return r ? trackFraction(clientX, r.left, r.width) : level;
  };

  const endDrag = (x: number | null) => {
    if (!dragging.current) return;
    dragging.current = false;
    hold(`volume${holdId}`, false);
    if (x !== null) set(levelAt(x), true);
  };

  return (
    <div className="flex shrink-0 items-center gap-1">
      <button
        type="button"
        title={volume.muted ? `Unmute ${app}` : `Mute ${app}`}
        aria-label={volume.muted ? `Unmute ${app}` : `Mute ${app}`}
        aria-pressed={volume.muted}
        onClick={() => {
          setLocal(null);
          void attempt(mediaApi.toggleMute);
        }}
        className="grid size-6 shrink-0 place-items-center rounded-md text-white/55 transition-colors outline-none hover:bg-white/8 hover:text-white/85 focus-visible:ring-1 focus-visible:ring-white/40"
      >
        <Icon className="size-3.5" />
      </button>
      <div
        role="slider"
        tabIndex={0}
        aria-label={`${app} volume`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-valuetext={volume.muted ? "Muted" : `${percent}%`}
        title={volume.muted ? "Muted" : `${percent}%`}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          e.preventDefault();
          e.currentTarget.focus({ preventScroll: true });
          e.currentTarget.setPointerCapture(e.pointerId);
          dragging.current = true;
          // NOTE: the pointer can leave the notch mid-drag; holding keeps it open.
          hold(`volume${holdId}`, true);
          set(levelAt(e.clientX), true);
        }}
        onPointerMove={(e) => dragging.current && set(levelAt(e.clientX), false)}
        onPointerUp={(e) => endDrag(e.clientX)}
        onPointerCancel={() => endDrag(null)}
        onLostPointerCapture={() => endDrag(null)}
        onKeyDown={(e) => {
          const step = e.key === "ArrowRight" || e.key === "ArrowUp" ? 0.05 : e.key === "ArrowLeft" || e.key === "ArrowDown" ? -0.05 : null;
          if (step === null) return;
          e.preventDefault();
          e.stopPropagation();
          set(level + step, true);
        }}
        className="group relative flex h-4 w-20 cursor-pointer touch-none items-center rounded outline-none focus-visible:ring-1 focus-visible:ring-white/40"
      >
        <div ref={trackRef} className="relative h-1 w-full rounded-full bg-white/12">
          <div className="absolute inset-y-0 left-0 rounded-full bg-white/60" style={{ width: `${percent}%` }} />
          <div
            aria-hidden
            className="absolute top-1/2 size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white opacity-0 shadow-sm transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100"
            style={{ left: `${percent}%` }}
          />
        </div>
      </div>
    </div>
  );
}

/** App icon from the core, or a generic one. */
function AppIcon({ session, className }: { session: MediaSession | undefined; className: string }) {
  return session?.icon ? (
    <img src={session.icon} alt="" className={`${className} shrink-0 object-contain`} draggable={false} />
  ) : (
    <Apps16Regular className={`${className} shrink-0 text-white/45`} />
  );
}

/**
 * The player's name in the byline. With more than one player it becomes a
 * menu for picking which one the card shows and controls.
 */
function SessionSwitch({ app, detail, playing }: { app: string; detail: NowPlaying | null; playing: boolean }) {
  const [open, setOpen] = useState(false);
  const hold = usePillStore((s) => s.hold);
  const holdId = useId();
  const menuId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const sessions = detail?.sessions ?? [];
  const current = sessions.find((s) => s.id === detail?.session);
  const switchable = sessions.length > 1;

  useEffect(() => {
    if (!switchable) setOpen(false);
  }, [switchable]);

  useEffect(() => {
    if (!open) return;
    hold(`media-menu${holdId}`, true);
    menuRef.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus({ preventScroll: true });
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onBlur = () => setOpen(false);
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("blur", onBlur);
    return () => {
      hold(`media-menu${holdId}`, false);
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [open, hold, holdId]);

  const state = <span className="shrink-0 text-white/35">{playing ? "Playing" : "Paused"}</span>;
  if (!switchable) {
    return (
      <div className="flex h-6 min-w-0 items-center gap-1.5 text-[11.5px] text-white/60">
        <AppIcon session={current} className="size-3.5" />
        <span className="truncate font-medium">{app}</span>
        {state}
      </div>
    );
  }

  const pick = (id: number) => {
    setOpen(false);
    triggerRef.current?.focus({ preventScroll: true });
    if (id !== detail?.session) void attempt(() => mediaApi.pickSession(id));
  };

  const onMenuKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitemradio"]') ?? []);
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      items[(at + step + items.length) % items.length]?.focus();
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      items[e.key === "Home" ? 0 : items.length - 1]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
      triggerRef.current?.focus({ preventScroll: true });
    } else if (e.key === "Tab") {
      setOpen(false);
    }
  };

  return (
    <div ref={rootRef} className="relative flex h-6 min-w-0 items-center gap-1.5 text-[11.5px] text-white/60">
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        title="Switch player"
        onClick={() => setOpen(!open)}
        className={`-mx-1.5 inline-flex h-6 min-w-0 items-center gap-1.5 rounded-md px-1.5 outline-none transition-colors hover:bg-white/8 hover:text-white/85 focus-visible:ring-1 focus-visible:ring-white/40 ${
          open ? "bg-white/8 text-white/85" : ""
        }`}
      >
        <AppIcon session={current} className="size-3.5" />
        <span className="truncate font-medium">{app}</span>
        <ChevronDown12Regular className="size-3 shrink-0 text-white/40" />
      </button>
      {state}
      <AnimatePresence>
        {open && (
          <motion.div
            ref={menuRef}
            id={menuId}
            role="menu"
            aria-label="Players"
            onKeyDown={onMenuKey}
            initial={{ opacity: 0, y: -4, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -4, scale: 0.98 }}
            transition={{ duration: 0.12 }}
            className="surface-menu absolute top-full left-0 z-40 mt-1.5 max-h-80 w-72 origin-top-left overflow-y-auto rounded-xl p-1 shadow-[0_16px_48px] shadow-shade/55 ring-1 ring-white/10"
          >
            {groupSessions(sessions).map((group) => (
              <div key={group.app} role="group" aria-label={group.app} className="flex flex-col py-0.5">
                <div className="flex h-6 items-center gap-1.5 px-2 text-[11px] font-medium text-white/45">
                  {group.icon ? (
                    <img src={group.icon} alt="" className="size-3.5 shrink-0 object-contain" draggable={false} />
                  ) : (
                    <Apps16Regular className="size-3.5 shrink-0" />
                  )}
                  <span className="truncate">{group.app}</span>
                </div>
                {group.entries.map(({ session: s, repeat }) => {
                  const selected = s.id === detail?.session;
                  return (
                    <button
                      key={s.id}
                      type="button"
                      role="menuitemradio"
                      aria-checked={selected}
                      onClick={() => pick(s.id)}
                      className={`flex w-full items-center gap-2 rounded-lg py-1.5 pr-2 pl-[26px] text-left outline-none hover:bg-white/8 focus-visible:bg-white/8 ${
                        selected ? "bg-white/[0.06]" : ""
                      }`}
                    >
                      <span className="min-w-0 flex-1">
                        <span className={`block truncate text-[12.5px] ${s.playing ? "text-white/90" : "text-white/60"}`} title={s.title}>
                          {s.title || "Untitled"}
                          {repeat !== null && <span className="text-white/35"> ({repeat})</span>}
                        </span>
                        {(s.artist || !s.playing) && (
                          <span className="block truncate text-[11px] text-white/40">
                            {[s.artist, s.playing ? "" : "Paused"].filter(Boolean).join(" · ")}
                          </span>
                        )}
                      </span>
                      <Checkmark12Regular className={`size-3 shrink-0 text-white/70 ${selected ? "" : "invisible"}`} />
                    </button>
                  );
                })}
              </div>
            ))}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
