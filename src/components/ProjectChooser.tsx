import { Search12Regular } from "@fluentui/react-icons";
import { useMemo, useState } from "react";
import { useKeyboardFocus } from "../hooks/useKeyboardFocus";
import { usePillStore } from "../store";
import { Empty } from "./tabs/TodayTab";

/** Above this many projects the grid gets a search field. */
const SEARCH_FROM = 9;

/**
 * Grid of projects for tabs that need one project (Notes, Chat). Picking a
 * project selects it globally, like the picker in the header.
 *
 * @param props.purpose - What the project is needed for, e.g. "Notes belong to a project".
 */
export function ProjectChooser({ purpose }: { purpose: string }) {
  const projects = usePillStore((s) => s.snapshot.projects);
  const select = usePillStore((s) => s.selectProject);
  const [query, setQuery] = useState("");
  const focus = useKeyboardFocus();
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? projects.filter((p) => p.name.toLowerCase().includes(q)) : projects;
  }, [projects, query]);

  if (projects.length === 0) {
    return (
      <Empty title="No projects yet">
        <p>Add a project root in Settings first.</p>
      </Empty>
    );
  }

  const choose = (id: string) => {
    if (query) focus.release();
    select(id);
  };

  return (
    <div className="flex h-full flex-col px-2 py-1">
      <div className="flex items-center gap-3 pb-2">
        <p className="min-w-0 flex-1 text-[12px] text-white/50">{purpose}. Pick one:</p>
        {projects.length > SEARCH_FROM && (
          <label className="flex h-7 w-52 shrink-0 items-center gap-1.5 rounded-lg bg-white/[0.06] px-2">
            <Search12Regular className="size-3 shrink-0 text-white/40" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              {...focus.props}
              onKeyDown={(e) => {
                if (e.key === "Enter" && shown[0]) choose(shown[0].id);
                if (e.key === "Escape") {
                  setQuery("");
                  focus.release(e.currentTarget);
                }
              }}
              placeholder={`Search ${projects.length} projects`}
              spellCheck={false}
              className="min-w-0 flex-1 bg-transparent text-[12px] text-white/90 outline-none placeholder:text-white/35"
            />
          </label>
        )}
      </div>
      {shown.length === 0 ? (
        <p className="px-1 text-[12px] text-white/40">No project matches “{query.trim()}”.</p>
      ) : (
        <div className="scroll-fade grid grid-cols-3 gap-1.5 overflow-y-auto">
          {shown.map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => choose(p.id)}
              className="flex h-9 items-center gap-2 rounded-lg bg-white/5 px-2.5 text-left text-[12px] text-white/80 hover:bg-white/9"
            >
              <span className="size-2 shrink-0 rounded-full" style={{ background: p.color }} />
              <span className="truncate">{p.name}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
