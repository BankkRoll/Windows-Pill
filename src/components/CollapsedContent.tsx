import {
  Alert16Filled,
  Bot16Filled,
  BranchFork16Filled,
  HandRight16Filled,
  Clock16Filled,
  ErrorCircle16Filled,
  MusicNote216Filled,
  ShieldError16Filled,
  type FluentIcon,
} from "@fluentui/react-icons";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useMemo } from "react";
import { useClock } from "../hooks/useClock";
import { formatDate, formatTime } from "../lib/format";
import type { LiveItem } from "../lib/native";
import { activeLive, collapsedLayout, toneText } from "../lib/plugins";
import { usePillStore, type Signal, type SignalKind, type TabId } from "../store";
import { PluginIcon } from "./PluginIcon";

interface SignalStyle {
  icon: FluentIcon;
  /** Tailwind text color for the icon and badge. */
  color: string;
  label: string;
  /** Tab the panel opens on when this signal is clicked. */
  tab: TabId;
}

const SIGNAL_STYLES: Record<Exclude<SignalKind, "plugin">, SignalStyle> = {
  failed: { icon: ErrorCircle16Filled, color: "text-status-red", label: "Agent failed", tab: "agents" },
  approval: { icon: HandRight16Filled, color: "text-status-purple", label: "Plan waiting for approval", tab: "agents" },
  security: { icon: ShieldError16Filled, color: "text-status-red", label: "Critical or high alerts", tab: "today" },
  pr: { icon: BranchFork16Filled, color: "text-status-green", label: "Ready for review", tab: "agents" },
  agent: { icon: Bot16Filled, color: "text-accent", label: "Agent running", tab: "agents" },
  due: { icon: Clock16Filled, color: "text-status-amber", label: "Due today", tab: "today" },
  notification: { icon: Alert16Filled, color: "text-white/70", label: "Notifications", tab: "today" },
};

/**
 * Time, date, a plugin's live item and the status cluster, laid out for the
 * collapsed notch. {@link collapsedLayout} decides what fits.
 *
 * @param props.width - Width of the collapsed body this content fills.
 * @param props.height - Height of the collapsed body.
 */
export function CollapsedContent({ width, height }: { width: number; height: number }) {
  const now = useClock();
  const signals = usePillStore((s) => s.signals);
  const plugins = usePillStore((s) => s.plugins);
  const live = useMemo(() => activeLive(plugins), [plugins]);
  const layout = collapsedLayout(signals, live?.live ?? null);
  const right = live !== null || layout.signals.length > 0;

  return (
    <div className={`flex items-center gap-2 px-3 ${right ? "justify-between" : "justify-center"}`} style={{ width, height }}>
      <div className="flex items-baseline gap-2 whitespace-nowrap">
        <span className="text-[13px] font-semibold tabular-nums">{formatTime(now)}</span>
        {layout.showDate && <span className="text-[12px] text-white/55">{formatDate(now)}</span>}
      </div>
      {right && (
        <div className="flex min-w-0 items-center gap-2">
          {live && <Live live={live.live} from={live.plugin.name} />}
          {(layout.signals.length > 0 || layout.overflow > 0) && <SignalCluster signals={layout.signals} overflow={layout.overflow} />}
        </div>
      )}
    </div>
  );
}

/**
 * A plugin's live item: tiny artwork with a playing hint, or an icon and a
 * few words. Kept to the notch's height and a fixed maximum width.
 */
function Live({ live, from }: { live: LiveItem; from: string }) {
  const setActiveTab = usePillStore((s) => s.setActiveTab);
  const reduceMotion = useReducedMotion() ?? false;
  if (live.kind === "media") {
    return (
      <button
        type="button"
        className="flex shrink-0 items-center gap-1.5 outline-none"
        title={`${live.playing ? "Playing" : "Paused"}: ${live.title}`}
        aria-label={`${live.playing ? "Playing" : "Paused"}: ${live.title}`}
        onClick={() => setActiveTab("today")}
      >
        {live.artwork ? (
          <img src={live.artwork} alt="" className={`size-4 rounded-[4px] object-cover ${live.playing ? "" : "opacity-60"}`} draggable={false} />
        ) : (
          <span className="grid size-4 place-items-center rounded-[4px] bg-white/10">
            <MusicNote216Filled className="size-2.5 text-white/60" />
          </span>
        )}
        {live.playing && <Bars still={reduceMotion} />}
      </button>
    );
  }
  return (
    <button
      type="button"
      className={`flex max-w-[96px] min-w-0 shrink items-center gap-1 outline-none ${toneText(live.tone)}`}
      title={`${from}: ${live.text}`}
      onClick={() => setActiveTab("today")}
    >
      {live.icon && <PluginIcon icon={live.icon} className="size-3.5 shrink-0" />}
      {live.text && <span className="truncate text-[11.5px] font-medium tabular-nums">{live.text}</span>}
    </button>
  );
}

/** Three small equalizer bars: the "playing" hint next to the artwork. */
function Bars({ still }: { still: boolean }) {
  const heights = [[3, 8, 4], [7, 3, 8], [4, 7, 3]];
  return (
    <span className="flex h-2.5 items-end gap-[2px]" aria-hidden>
      {heights.map((h, i) => (
        <motion.span
          key={i}
          className="w-[2px] rounded-full bg-white/65"
          style={{ height: h[1] }}
          animate={still ? undefined : { height: h }}
          transition={{ duration: 0.9 + i * 0.15, repeat: Infinity, repeatType: "mirror", ease: "easeInOut" }}
        />
      ))}
    </span>
  );
}

function SignalCluster({ signals, overflow }: { signals: readonly Signal[]; overflow: number }) {
  const setActiveTab = usePillStore((s) => s.setActiveTab);
  return (
    <div className="flex items-center gap-1.5">
      <AnimatePresence initial={false} mode="popLayout">
        {signals.map((signal) => {
          const key = signal.plugin?.key ?? signal.kind;
          const style = signal.kind === "plugin" ? null : SIGNAL_STYLES[signal.kind];
          const label = signal.plugin?.label ?? style?.label ?? "";
          const color = signal.plugin ? toneText(signal.plugin.tone) : style!.color;
          const Icon = style?.icon;
          return (
            <motion.button
              key={key}
              type="button"
              layout
              className={`flex items-center gap-0.5 rounded-full outline-none ${color}`}
              initial={{ opacity: 0, scale: 0.6 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.6 }}
              transition={{ type: "spring", duration: 0.3, bounce: 0.25 }}
              title={signal.count > 1 ? `${label} (${signal.count})` : label}
              aria-label={signal.count > 1 ? `${label}: ${signal.count}` : label}
              onClick={() => setActiveTab(style?.tab ?? "today")}
            >
              {Icon ? (
                <Icon className={`size-3.5 ${signal.kind === "agent" ? "animate-breathe" : ""}`} />
              ) : (
                signal.plugin && <PluginIcon icon={signal.plugin.icon} className="size-3.5" />
              )}
              {signal.count > 1 && <span className="text-[10px] leading-none font-semibold tabular-nums">{signal.count}</span>}
            </motion.button>
          );
        })}
      </AnimatePresence>
      {overflow > 0 && (
        <span className="text-[10px] leading-none font-semibold text-white/50 tabular-nums" title={`${overflow} more`}>
          +{overflow}
        </span>
      )}
    </div>
  );
}
