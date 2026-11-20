import { useMemo, useState } from "react";
import { ChevronDown12Regular, ChevronRight12Regular } from "@fluentui/react-icons";
import { attempt, openPluginPage } from "../lib/actions";
import { pluginApi, type PageChip, type PluginState, type PluginWidget } from "../lib/native";
import { todayCards, toneText, type ExtensionCard as ExtensionCardData } from "../lib/plugins";
import { usePillStore } from "../store";
import { NotificationsCard, useHasNotificationsCard } from "./NotificationsCard";
import { NowPlayingCard } from "./NowPlayingCard";
import { FeedRows } from "./PluginFeed";
import { PluginIcon } from "./PluginIcon";

/**
 * Plugin cards at the top of the Today tab, in two tiers. Pill's own come
 * first: Now playing beside the Windows notifications card. Third-party
 * plugins follow in the user's order, one card each, holding the plugin's
 * summary and its alerts; past a few, the rest fold into one row that
 * expands in place. Renders nothing when there are no cards.
 */
export function PluginWidgets() {
  const plugins = usePillStore((s) => s.plugins);
  const { native, extensions, more } = useMemo(() => todayCards(plugins), [plugins]);
  const notifications = useHasNotificationsCard();
  const nativeCount = native.length + (notifications ? 1 : 0);
  if (nativeCount === 0 && extensions.length === 0) return null;
  return (
    <div className="flex shrink-0 flex-col gap-2 pt-1 pb-3">
      {nativeCount > 0 && (
        <div className={`grid items-start gap-2 ${nativeCount > 1 ? "grid-cols-2" : "grid-cols-1"}`}>
          {native.map(({ plugin, widget }) => (
            <NativeWidget key={plugin.id} plugin={plugin} widget={widget} />
          ))}
          {notifications && <NotificationsCard />}
        </div>
      )}
      {extensions.map(({ plugin, widget }) => (
        <ExtensionCard key={plugin.id} plugin={plugin} widget={widget} />
      ))}
      {more.length > 0 && <MoreCards cards={more} />}
    </div>
  );
}

/**
 * Third-party cards past the first few, folded into one row naming the
 * plugins and how many alerts they hold. Expanding it draws the cards in
 * place.
 */
function MoreCards({ cards }: { cards: readonly ExtensionCardData[] }) {
  const [open, setOpen] = useState(false);
  const alerts = cards.reduce((n, c) => n + c.plugin.feed.length, 0);
  const names = cards.map((c) => c.plugin.name).join(", ");
  return (
    <>
      {open && cards.map(({ plugin, widget }) => <ExtensionCard key={plugin.id} plugin={plugin} widget={widget} />)}
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex h-8 w-full items-center gap-2 rounded-xl bg-white/[0.03] px-3 text-left text-[12px] ring-1 ring-white/[0.05] transition-colors outline-none hover:bg-white/[0.06] focus-visible:ring-white/30"
      >
        <span className="shrink-0 font-medium text-white/70">
          {open ? "Show fewer plugins" : `+${cards.length} more ${cards.length === 1 ? "plugin" : "plugins"}`}
        </span>
        {!open && (
          <span className="min-w-0 flex-1 truncate text-white/40" title={names}>
            {names}
          </span>
        )}
        {open && <span className="flex-1" />}
        {!open && alerts > 0 && (
          <span className="shrink-0 text-[11px] text-white/50 tabular-nums">
            {alerts} {alerts === 1 ? "alert" : "alerts"}
          </span>
        )}
        <ChevronDown12Regular className={`size-3 shrink-0 text-white/45 transition-transform ${open ? "rotate-180" : ""}`} />
      </button>
    </>
  );
}

function NativeWidget({ plugin, widget }: { plugin: PluginState; widget: PluginWidget }) {
  return widget.kind === "media" ? <NowPlayingCard pluginId={plugin.id} media={widget} /> : <SummaryCard plugin={plugin} widget={widget} />;
}

/** Opens the plugin's tab at the project picked in the header, or its overview. */
function OpenTab({ plugin }: { plugin: PluginState }) {
  const selected = usePillStore((s) => s.selectedProjectId);
  return (
    <button
      type="button"
      onClick={() => openPluginPage(plugin.id, selected)}
      className="inline-flex h-6 items-center gap-1 rounded-md px-2 text-[11.5px] text-white/70 hover:bg-white/8 hover:text-white"
    >
      Open {plugin.tab?.title}
      <ChevronRight12Regular className="size-3" />
    </button>
  );
}

function send(pluginId: string, action: string) {
  void attempt(() => pluginApi.action(pluginId, action));
}

/** A third-party plugin's card: its summary, then its alerts. */
function ExtensionCard({ plugin, widget }: { plugin: PluginState; widget: Extract<PluginWidget, { kind: "card" }> | null }) {
  const project = usePillStore((s) => s.snapshot.projects.find((p) => p.id === s.selectedProjectId) ?? null);
  // NOTE: the widget is one summary for every project; with a project picked,
  // its line in the tab's sidebar is the honest figure instead.
  const scoped = project && plugin.tab ? projectStanding(plugin, project.id) : undefined;
  const card = widget ?? { kind: "card" as const, title: plugin.name, rows: [], actions: [] };
  return (
    <section className="flex flex-col gap-2 rounded-xl bg-white/[0.045] px-3 py-2.5 ring-1 ring-white/[0.06]" aria-label={card.title}>
      <Summary plugin={plugin} widget={scoped === undefined ? card : { ...card, rows: [] }} />
      {scoped !== undefined && project && (
        <p className={`text-[12px] ${scoped ? toneText(scoped.tone) : "text-white/55"}`}>
          {scoped ? `${scoped.text} in ${project.name}` : `Nothing flagged in ${project.name}`}
        </p>
      )}
      <FeedRows plugin={plugin} />
    </section>
  );
}

/**
 * The badge a plugin gives a project in its tab's sidebar, `null` when it
 * gives none (nothing to report).
 */
function projectStanding(plugin: PluginState, projectId: string): PageChip | null {
  return plugin.nav.find((n) => n.project === projectId)?.badge ?? null;
}

function SummaryCard({ plugin, widget }: { plugin: PluginState; widget: Extract<PluginWidget, { kind: "card" }> }) {
  return (
    <section className="flex flex-col gap-1 rounded-xl bg-white/[0.045] px-3 py-2.5 ring-1 ring-white/[0.06]" aria-label={widget.title}>
      <Summary plugin={plugin} widget={widget} />
    </section>
  );
}

/**
 * A card widget's title, rows and buttons. Rows sit side by side when there's
 * room, so a status summary reads as one line of facts.
 */
function Summary({ plugin, widget }: { plugin: PluginState; widget: Extract<PluginWidget, { kind: "card" }> }) {
  return (
    <div className="flex flex-col gap-1.5">
      <header className="flex items-center justify-between gap-2">
        <h3 className="truncate text-[12px] font-semibold text-white/85">{widget.title}</h3>
        <div className="flex shrink-0 items-center gap-1.5">
          {widget.title !== plugin.name && <span className="truncate text-[10.5px] text-white/35">{plugin.name}</span>}
          {plugin.tab && <OpenTab plugin={plugin} />}
          {widget.actions.map((a) => (
            <button
              key={a.id}
              type="button"
              onClick={() => send(plugin.id, a.id)}
              className="inline-flex h-6 items-center gap-1 rounded-md bg-white/7 px-2 text-[11.5px] text-white/80 hover:bg-white/12 hover:text-white"
            >
              {a.icon && <PluginIcon icon={a.icon} className="size-3" />}
              {a.label}
            </button>
          ))}
        </div>
      </header>
      {widget.rows.length > 0 && (
        <ul className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-x-4 gap-y-1">
          {widget.rows.map((row, i) => (
            <li key={i} className="flex min-w-0 items-start gap-2">
              {row.icon && <PluginIcon icon={row.icon} className={`mt-px size-3.5 shrink-0 ${toneText(row.tone)}`} />}
              <span className="min-w-0 flex-1">
                <span className="line-clamp-2 block text-[12px] leading-snug text-white/85" title={row.text}>
                  {row.text}
                </span>
                {row.detail && (
                  <span className="block truncate text-[11px] text-white/45" title={row.detail}>
                    {row.detail}
                  </span>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
