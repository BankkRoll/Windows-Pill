import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useDismiss } from "../hooks/useDismiss";
import { focusWindow } from "../lib/native";
import { overviewBadge, pluginTabId, type TabPlugin } from "../lib/pages";
import { usePillStore } from "../store";
import { PageChipView } from "./PluginBlocks";
import { PluginIcon } from "./PluginIcon";

/**
 * The header's `+N` button for plugin tabs that didn't fit (or that the user
 * keeps out of the header): a menu listing each with its icon, title and the
 * badge of its overview. Picking one opens its tab, which then takes a slot
 * in the header while it's open.
 *
 * @param props.plugins - Overflowed tab plugins, in Settings order.
 */
export function PluginTabMenu({ plugins }: { plugins: readonly TabPlugin[] }) {
  const setTab = usePillStore((s) => s.setActiveTab);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const items = useRef<(HTMLButtonElement | null)[]>([]);
  const count = plugins.length;
  const onKeyDown = useDismiss(open, () => setOpen(false), [ref]);

  // Plugins turned off while the menu is open can empty it.
  useEffect(() => {
    if (count === 0) setOpen(false);
  }, [count]);

  const close = () => {
    setOpen(false);
    trigger.current?.focus();
  };

  const choose = (id: string) => {
    setOpen(false);
    setTab(pluginTabId(id));
  };

  const onMenuKey = (e: KeyboardEvent) => {
    const at = items.current.findIndex((el) => el === document.activeElement);
    const step = { ArrowDown: 1, ArrowUp: -1 }[e.key];
    if (step !== undefined) {
      e.preventDefault();
      items.current[(at + step + count) % count]?.focus();
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      items.current[e.key === "Home" ? 0 : count - 1]?.focus();
    } else if (e.key === "Escape" || e.key === "Tab") {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
      }
      close();
    }
  };

  const onTriggerKey = (e: KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    setOpen(true);
    const target = e.key === "ArrowDown" ? 0 : count - 1;
    requestAnimationFrame(() => items.current[target]?.focus());
  };

  if (count === 0) return null;
  const label = `${count} more plugin ${count === 1 ? "tab" : "tabs"}`;

  return (
    <div ref={ref} className="relative" onKeyDown={onKeyDown}>
      <button
        ref={trigger}
        type="button"
        title={label}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        onPointerDown={() => void focusWindow()}
        onClick={() => {
          setOpen((o) => !o);
          if (!open) requestAnimationFrame(() => items.current[0]?.focus({ preventScroll: true }));
        }}
        onKeyDown={onTriggerKey}
        className={`grid h-7 min-w-7 place-items-center rounded-lg px-1.5 text-[11.5px] font-medium tabular-nums outline-none transition-colors focus-visible:ring-1 focus-visible:ring-white/40 ${
          open ? "bg-white/12 text-white/92" : "text-white/50 hover:bg-white/8 hover:text-white/85"
        }`}
      >
        +{count}
      </button>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: -4, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -4, scale: 0.98 }}
            transition={{ duration: 0.12 }}
            className="absolute top-8 left-0 z-20 flex max-h-[300px] w-60 flex-col rounded-xl surface-menu p-1 shadow-xl ring-1 ring-white/10"
          >
            <ul role="menu" aria-label="Plugin tabs" onKeyDown={onMenuKey} className="scroll-fade min-h-0 flex-1 overflow-y-auto">
              {plugins.map((p, i) => {
                const badge = overviewBadge(p.nav);
                return (
                  <li key={p.id} role="none">
                    <button
                      ref={(el) => {
                        items.current[i] = el;
                      }}
                      type="button"
                      role="menuitem"
                      tabIndex={-1}
                      onClick={() => choose(p.id)}
                      title={p.tab.title === p.name ? p.name : `${p.tab.title}, from ${p.name}`}
                      className="flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[12px] text-white/85 outline-none hover:bg-white/8 focus-visible:bg-white/8"
                    >
                      <PluginIcon icon={p.tab.icon} className="size-4 shrink-0 text-white/60" />
                      <span className="min-w-0 flex-1 truncate">{p.tab.title}</span>
                      {badge && <PageChipView chip={badge} />}
                    </button>
                  </li>
                );
              })}
            </ul>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
