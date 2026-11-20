import { ArrowClockwise16Regular, FolderOpen16Regular, ReOrderDotsVertical16Regular, Warning16Regular } from "@fluentui/react-icons";
import { Reorder, useDragControls } from "framer-motion";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { attempt, openPluginsFolder, openSystemSettings } from "../../lib/actions";
import { pluginApi, type PluginState, type PluginStatusState } from "../../lib/native";
import { usePillStore } from "../../store";
import { PluginIcon } from "../PluginIcon";
import { Checkbox, Toggle } from "../controls";
import { Chip, type ChipTone } from "../ui";
import { Group, PageHeader, SoftButton } from "./kit";

const STATUS: Record<PluginStatusState, { label: string; tone: ChipTone } | null> = {
  off: null,
  starting: { label: "Starting", tone: "neutral" },
  running: { label: "Running", tone: "green" },
  needsPermission: { label: "Needs permission", tone: "amber" },
  unsupported: { label: "Not available", tone: "neutral" },
  error: { label: "Error", tone: "red" },
  changed: { label: "Command changed", tone: "amber" },
  invalid: { label: "Invalid", tone: "red" },
};

/**
 * Settings → Plugins: built-in plugins and the external plugins found in the
 * plugins folder, with toggles, live status and fixes. External plugins show
 * their exact command and need a confirmation before they first run, and can
 * be dragged (or moved with Alt+Up/Down) into the order every plugin list
 * follows: header tabs, Chat's project sidebar, Today cards and signals.
 */
export function PluginsSettings() {
  const plugins = usePillStore((s) => s.plugins);
  const builtin = plugins.filter((p) => p.source === "builtin");
  const external = plugins.filter((p) => p.source === "external");

  // The folder may have changed while Settings was closed.
  useEffect(() => {
    void pluginApi.rescan().catch(() => {});
  }, []);

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Plugins"
        subtitle="Extra signals, notch items and Today cards."
        action={
          <SoftButton icon={<FolderOpen16Regular className="size-3.5" />} onClick={openPluginsFolder}>
            Open plugins folder
          </SoftButton>
        }
      />

      <Group title="Built-in">
        {builtin.map((p) => (
          <PluginRow key={p.id} plugin={p} />
        ))}
      </Group>

      <Group
        title="External"
        hint={`Programs in your plugins folder. They only run after you turn them on here.${
          external.length > 1 ? " Drag to set the order Pill lists them in." : ""
        }`}
        action={
          <SoftButton icon={<ArrowClockwise16Regular className="size-3.5" />} onClick={() => void attempt(() => pluginApi.rescan())}>
            Reload
          </SoftButton>
        }
      >
        {external.length === 0 ? (
          <p className="px-3 py-3 text-[12px] leading-relaxed text-white/45">
            No plugins yet. Put a folder with a plugin.json in the plugins folder, then reload.
          </p>
        ) : (
          <ExternalList plugins={external} />
        )}
      </Group>
    </div>
  );
}

/**
 * External plugins in the user's order. A row moves by dragging its handle,
 * with Alt+Up/Down from anywhere in it, or with the arrows on its handle;
 * every move is saved.
 */
function ExternalList({ plugins }: { plugins: readonly PluginState[] }) {
  const hold = usePillStore((s) => s.hold);
  const [ids, setIds] = useState(() => plugins.map((p) => p.id));
  const latest = useRef(ids);
  latest.current = ids;
  const [announcement, setAnnouncement] = useState("");
  const key = plugins.map((p) => p.id).join("|");

  // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` captures order and membership
  useEffect(() => setIds(plugins.map((p) => p.id)), [key]);
  useEffect(() => () => hold("drag", false), [hold]);

  const byId = new Map(plugins.map((p) => [p.id, p]));
  const save = (next: readonly string[]) => void attempt(() => pluginApi.setOrder(next));

  const move = (id: string, by: -1 | 1) => {
    const from = ids.indexOf(id);
    const to = from + by;
    if (from < 0 || to < 0 || to >= ids.length) return;
    const next = [...ids];
    next.splice(to, 0, ...next.splice(from, 1));
    // NOTE: React reorders by moving DOM nodes, which can drop focus from the
    // moved row; restore it so repeated Alt+Arrow presses keep moving it.
    const focused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setIds(next);
    save(next);
    setAnnouncement(`${byId.get(id)?.name ?? id}, position ${to + 1} of ${next.length}`);
    requestAnimationFrame(() => {
      if (focused && document.activeElement !== focused) focused.focus();
    });
  };

  return (
    <div className="relative">
      <Reorder.Group as="div" axis="y" values={ids} onReorder={setIds} className="flex flex-col divide-y divide-white/[0.05]">
        {ids.map((id) => {
          const plugin = byId.get(id);
          return (
            plugin && (
              <ExternalItem
                key={id}
                plugin={plugin}
                onMove={(by) => move(id, by)}
                onDragStart={() => hold("drag", true)}
                onDragEnd={() => {
                  hold("drag", false);
                  save(latest.current);
                }}
              />
            )
          );
        })}
      </Reorder.Group>
      <p aria-live="polite" className="sr-only">
        {announcement}
      </p>
    </div>
  );
}

function ExternalItem({
  plugin,
  onMove,
  onDragStart,
  onDragEnd,
}: {
  plugin: PluginState;
  onMove: (by: -1 | 1) => void;
  onDragStart: () => void;
  onDragEnd: () => void;
}) {
  const controls = useDragControls();
  const [dragging, setDragging] = useState(false);
  const arrows = (e: KeyboardEvent, needAlt: boolean) => {
    if ((e.key !== "ArrowUp" && e.key !== "ArrowDown") || (needAlt && !e.altKey)) return;
    e.preventDefault();
    e.stopPropagation();
    onMove(e.key === "ArrowUp" ? -1 : 1);
  };
  const handle = (
    <button
      type="button"
      aria-label={`Move ${plugin.name}`}
      title="Drag to reorder, or use Alt+Up and Alt+Down"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        controls.start(e);
      }}
      onKeyDown={(e) => arrows(e, false)}
      className="grid h-7 w-5 shrink-0 cursor-grab touch-none place-items-center rounded-md text-white/30 outline-none transition-colors hover:bg-white/8 hover:text-white/70 focus-visible:text-white/80 focus-visible:ring-1 focus-visible:ring-white/40 active:cursor-grabbing"
    >
      <ReOrderDotsVertical16Regular className="size-4" />
    </button>
  );
  return (
    <Reorder.Item
      as="div"
      value={plugin.id}
      dragListener={false}
      dragControls={controls}
      onDragStart={() => {
        setDragging(true);
        onDragStart();
      }}
      onDragEnd={() => {
        setDragging(false);
        onDragEnd();
      }}
      onKeyDown={(e) => arrows(e, true)}
      style={{ position: "relative" }}
      whileDrag={{ zIndex: 1, scale: 1.01 }}
      className={dragging ? "rounded-xl surface-menu shadow-xl ring-1 ring-white/10" : undefined}
    >
      <div className="absolute top-2 left-1.5">{handle}</div>
      <PluginRow plugin={plugin} indent />
    </Reorder.Item>
  );
}

/**
 * One plugin's toggle, status, command and fixes.
 *
 * @param props.indent - Leaves room at the start for a drag handle.
 */
function PluginRow({ plugin, indent = false }: { plugin: PluginState; indent?: boolean }) {
  const [confirming, setConfirming] = useState(false);
  const status = STATUS[plugin.status.state];
  const external = plugin.source === "external";
  const invalid = plugin.status.state === "invalid";
  const { fix, message } = plugin.status;

  // A command or access that changed under an open confirmation must be shown again.
  useEffect(() => setConfirming(false), [plugin.approval]);
  const readsProjects = plugin.capabilities.includes("projects");
  const suggestsTasks = plugin.capabilities.includes("feed") || plugin.capabilities.includes("tab");

  const toggle = (on: boolean) => {
    if (on && external) {
      setConfirming(true);
      return;
    }
    setConfirming(false);
    void attempt(() => pluginApi.setEnabled(plugin.id, on));
  };

  const approve = () => {
    setConfirming(false);
    void attempt(() => pluginApi.setEnabled(plugin.id, true, plugin.approval));
  };

  return (
    <div className={`flex flex-col gap-1.5 py-2.5 pr-3 ${indent ? "pl-8" : "pl-3"}`}>
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="truncate text-[12.5px] text-white/85">{plugin.name}</span>
            {status && <Chip tone={status.tone}>{status.label}</Chip>}
          </div>
          {plugin.description && <p className="text-[11px] leading-snug text-white/40">{plugin.description}</p>}
          {external && readsProjects && <p className="text-[11px] leading-snug text-white/40">Can read your project folders.</p>}
          {plugin.tab && <p className="text-[11px] leading-snug text-white/40">Adds the “{plugin.tab.title}” tab.</p>}
        </div>
        {!invalid && <Toggle checked={plugin.enabled || confirming} onChange={toggle} />}
      </div>

      {plugin.tab && plugin.enabled && !confirming && (
        <label className="flex w-fit cursor-pointer items-center gap-2 text-[11.5px] text-white/60 hover:text-white/80">
          <Checkbox checked={plugin.pinned} onChange={(on) => void attempt(() => pluginApi.setPinned(plugin.id, on))} />
          Show in the top bar
          {!plugin.pinned && <span className="text-white/35">Opens from the header’s “+” menu instead</span>}
        </label>
      )}

      {external && plugin.command && !confirming && (
        <code className="block rounded-md bg-white/[0.05] px-2 py-1 font-mono text-[11px] break-all text-white/60 select-text" title={plugin.folder ?? undefined}>
          {plugin.command}
        </code>
      )}

      {confirming && plugin.command && (
        <div className="flex flex-col gap-2 rounded-lg bg-status-amber/8 p-2.5 ring-1 ring-status-amber/20">
          <p className="flex items-start gap-1.5 text-[11.5px] leading-snug text-white/75">
            <Warning16Regular className="mt-px size-3.5 shrink-0 text-status-amber" />
            Pill will run this command as you, from {plugin.folder ?? "the plugin folder"}. Only turn it on if you trust it.
          </p>
          <code className="block rounded-md bg-white/[0.06] px-2 py-1 font-mono text-[11px] break-all text-white/85 select-text">{plugin.command}</code>
          {readsProjects && (
            <p className="flex items-start gap-1.5 text-[11.5px] leading-snug text-white/75">
              <FolderOpen16Regular className="mt-px size-3.5 shrink-0 text-status-amber" />
              It also gets the name and folder of every project, so it can read the files in them.
            </p>
          )}
          {plugin.tab && (
            <p className="flex items-start gap-1.5 text-[11.5px] leading-snug text-white/75">
              <PluginIcon icon={plugin.tab.icon} className="mt-px size-3.5 shrink-0 text-white/55" />
              It adds a “{plugin.tab.title}” tab to the panel, showing pages it sends.
            </p>
          )}
          {suggestsTasks && (
            <p className="flex items-start gap-1.5 text-[11.5px] leading-snug text-white/75">
              <PluginIcon icon="bot" className="mt-px size-3.5 shrink-0 text-white/55" />
              It can suggest tasks for your agents, which you start yourself.
            </p>
          )}
          <div className="flex justify-end gap-1.5">
            <button type="button" onClick={() => setConfirming(false)} className="h-7 rounded-lg px-2.5 text-[12px] text-white/65 hover:bg-white/8">
              Cancel
            </button>
            <button type="button" onClick={approve} className="h-7 rounded-lg bg-accent px-3 text-[12px] font-semibold text-on-accent">
              Run it
            </button>
          </div>
        </div>
      )}

      {message && (
        <p
          className={`text-[11px] leading-snug select-text ${
            plugin.status.state === "error" || invalid ? "text-status-red/90" : plugin.status.state === "running" ? "text-white/45" : "text-status-amber/90"
          }`}
        >
          {message}
        </p>
      )}

      {fix && plugin.enabled && (
        <div>
          {fix.kind === "settings" && (
            <SoftButton onClick={() => openSystemSettings(fix.page)}>{fix.label}</SoftButton>
          )}
          {fix.kind === "action" && (
            <SoftButton onClick={() => void attempt(() => pluginApi.action(plugin.id, fix.id))}>{fix.label}</SoftButton>
          )}
          {fix.kind === "restart" && (
            <SoftButton icon={<ArrowClockwise16Regular className="size-3.5" />} onClick={() => void attempt(() => pluginApi.restart(plugin.id))}>
              Restart
            </SoftButton>
          )}
        </div>
      )}
    </div>
  );
}
