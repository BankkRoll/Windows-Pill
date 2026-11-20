import { Checkmark12Regular, ChevronDown12Regular, Search12Regular } from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useMemo, useRef, useState } from "react";
import { useDismiss } from "../hooks/useDismiss";
import { useKeyboardFocus } from "../hooks/useKeyboardFocus";
import { focusWindow } from "../lib/native";
import { usePillStore } from "../store";

/** Above this many projects the menu gets a search field. */
const SEARCH_FROM = 8;

/**
 * Dropdown choosing which project the panel focuses on, or all projects.
 * Projects are listed most recently active first.
 */
export function ProjectPicker() {
  const projects = usePillStore((s) => s.snapshot.projects);
  const selectedId = usePillStore((s) => s.selectedProjectId);
  const select = usePillStore((s) => s.selectProject);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const selected = projects.find((p) => p.id === selectedId) ?? null;
  const [query, setQuery] = useState("");
  const focus = useKeyboardFocus();
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? projects.filter((p) => p.name.toLowerCase().includes(q)) : projects;
  }, [projects, query]);
  const onKeyDown = useDismiss(open, () => setOpen(false), [ref]);

  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  const choose = (id: string | null) => {
    select(id);
    setOpen(false);
    if (query) focus.release();
  };

  return (
    <div ref={ref} className="relative" onKeyDown={onKeyDown}>
      <button
        type="button"
        onPointerDown={() => void focusWindow()}
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="listbox"
        aria-expanded={open}
        className="inline-flex h-7 max-w-56 items-center gap-1.5 rounded-lg px-2.5 text-[12px] text-white/80 hover:bg-white/8"
      >
        <span
          className="size-2 shrink-0 rounded-full"
          style={{
            background: selected?.color ?? "conic-gradient(#2F7BFF, #30D158, #FFB340, #FF453A, #2F7BFF)",
          }}
        />
        <span className="truncate font-medium">{selected?.name ?? "All projects"}</span>
        <ChevronDown12Regular className="size-3 shrink-0 text-white/45" />
      </button>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: -4, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -4, scale: 0.98 }}
            transition={{ duration: 0.12 }}
            className="absolute top-8 left-1/2 z-20 flex max-h-[300px] w-60 -translate-x-1/2 flex-col rounded-xl surface-menu p-1 shadow-xl ring-1 ring-white/10"
          >
            {projects.length > SEARCH_FROM && (
              <label className="mb-1 flex h-7 shrink-0 items-center gap-1.5 rounded-lg bg-white/[0.06] px-2">
                <Search12Regular className="size-3 shrink-0 text-white/40" />
                <input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  {...focus.props}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && shown[0]) choose(shown[0].id);
                    if (e.key === "Escape") {
                      e.preventDefault();
                      focus.release(e.currentTarget);
                      setOpen(false);
                    }
                  }}
                  placeholder={`Search ${projects.length} projects`}
                  spellCheck={false}
                  className="min-w-0 flex-1 bg-transparent text-[12px] text-white/90 outline-none placeholder:text-white/35"
                />
              </label>
            )}
            <ul role="listbox" className="scroll-fade min-h-0 flex-1 overflow-y-auto">
              {!query && <Option label="All projects" active={!selected} onClick={() => choose(null)} />}
              {!query && projects.length > 0 && <li className="my-1 h-px bg-white/8" />}
              {query && shown.length === 0 && <li className="px-2 py-1.5 text-[12px] text-white/40">No project matches</li>}
              {shown.map((p) => (
                <Option
                  key={p.id}
                  label={p.name}
                  color={p.color}
                  meta={p.tasks.filter((t) => t.status !== "done").length || undefined}
                  active={p.id === selectedId}
                  onClick={() => choose(p.id)}
                />
              ))}
            </ul>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function Option({
  label,
  color,
  meta,
  active,
  onClick,
}: {
  label: string;
  color?: string;
  meta?: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <li>
      <button
        type="button"
        role="option"
        aria-selected={active}
        onClick={onClick}
        className="flex h-7 w-full items-center gap-2 rounded-lg px-2 text-left text-[12px] text-white/85 hover:bg-white/8"
      >
        <span className="size-2 shrink-0 rounded-full" style={{ background: color ?? "transparent" }} />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {meta !== undefined && <span className="text-white/35 tabular-nums">{meta}</span>}
        {active && <Checkmark12Regular className="size-3 text-accent" />}
      </button>
    </li>
  );
}
