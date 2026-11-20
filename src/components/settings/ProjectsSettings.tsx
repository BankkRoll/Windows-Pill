import {
  ArrowClockwise16Regular,
  ChevronRight12Regular,
  Folder16Regular,
  FolderAdd16Regular,
  HardDrive16Regular,
  Search16Regular,
  Warning16Regular,
} from "@fluentui/react-icons";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
import { useKeyboardFocus } from "../../hooks/useKeyboardFocus";
import { attempt } from "../../lib/actions";
import { api, isTauri } from "../../lib/native";
import { usePillStore } from "../../store";
import { Group, PageHeader, PathRow, SoftButton } from "./kit";

/**
 * Where projects come from (root folders, whole drives, single repos) and a
 * searchable list of every project found, each linking to its settings.
 */
export function ProjectsSettings() {
  const config = usePillStore((s) => s.config);
  const projects = usePillStore((s) => s.snapshot.projects);
  const hold = usePillStore((s) => s.hold);
  const [drives, setDrives] = useState<string[]>([]);
  const [scanning, setScanning] = useState(false);

  useEffect(() => {
    void api.listDrives().then(setDrives);
  }, []);

  const pickFolder = async (title: string): Promise<string | null> => {
    if (!isTauri) return null;
    // The dialog takes focus; holding keeps the panel open behind it.
    hold("dialog", true);
    try {
      const picked = await open({ directory: true, multiple: false, title });
      return typeof picked === "string" ? picked : null;
    } finally {
      hold("dialog", false);
    }
  };

  const withScan = async (op: () => Promise<unknown>) => {
    setScanning(true);
    await attempt(op);
    setScanning(false);
  };

  const addRoot = async (path: string | null) => {
    if (path && !config.roots.includes(path)) await withScan(() => api.setRoots([...config.roots, path]));
  };

  const countUnder = (root: string) => {
    const prefix = root.endsWith("\\") ? root.toLowerCase() : `${root.toLowerCase()}\\`;
    return projects.filter((p) => p.path.toLowerCase().startsWith(prefix)).length;
  };

  const unusedDrives = drives.filter((d) => !config.roots.includes(d));

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Projects"
        subtitle={`${projects.length} git repo${projects.length === 1 ? "" : "s"} found. Each one keeps its tasks and notes in .pill/.`}
        action={
          <SoftButton
            disabled={scanning}
            icon={<ArrowClockwise16Regular className={`size-3.5 ${scanning ? "animate-spin" : ""}`} />}
            onClick={() => void withScan(() => api.rescan())}
          >
            {scanning ? "Scanning…" : "Rescan"}
          </SoftButton>
        }
      />

      <Group title="Roots" hint="Every git repo inside these folders, up to four levels deep, becomes a project.">
        {config.roots.length === 0 && (
          <p className="px-3 py-2.5 text-[12px] text-white/45">No roots yet. Add a folder such as C:\dev, or a whole drive.</p>
        )}
        {config.roots.map((root) => (
          <PathRow
            key={root}
            icon={root.length <= 3 ? HardDrive16Regular : Folder16Regular}
            path={root}
            meta={`${countUnder(root)} repos`}
            onRemove={() => void withScan(() => api.setRoots(config.roots.filter((r) => r !== root)))}
          />
        ))}
        <div className="flex flex-wrap items-center gap-1.5 px-3 py-2">
          <SoftButton icon={<FolderAdd16Regular className="size-4" />} onClick={async () => addRoot(await pickFolder("Add a project root"))}>
            Add folder…
          </SoftButton>
          {unusedDrives.map((d) => (
            <SoftButton key={d} icon={<HardDrive16Regular className="size-4" />} onClick={() => void addRoot(d)}>
              Scan {d.replace(/\\$/, "")}
            </SoftButton>
          ))}
        </div>
      </Group>

      <Group title="Single repos" hint="Repos you want without scanning their parent folder.">
        {config.projects.map((p) => (
          <PathRow key={p} icon={Folder16Regular} path={p} onRemove={() => void withScan(() => api.removeProject(p))} />
        ))}
        <div className="px-3 py-2">
          <SoftButton
            icon={<FolderAdd16Regular className="size-4" />}
            onClick={async () => {
              const path = await pickFolder("Add a project");
              if (path) await withScan(() => api.addProject(path));
            }}
          >
            Add repo…
          </SoftButton>
        </div>
      </Group>

      <ProjectList />
    </div>
  );
}

function ProjectList() {
  const projects = usePillStore((s) => s.snapshot.projects);
  const selectProject = usePillStore((s) => s.selectProject);
  const [query, setQuery] = useState("");
  const focus = useKeyboardFocus();

  const shown = useMemo(() => {
    const words = query.trim().toLowerCase().split(/s+/).filter(Boolean);
    if (words.length === 0) return projects;
    return projects.filter((p) => {
      const hay = `${p.name} ${p.path}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
  }, [projects, query]);

  if (projects.length === 0) return null;

  return (
    <Group
      title="All projects"
      hint="Most recently active first. Pick one to edit its settings."
      action={
        <label className="flex h-7 w-44 items-center gap-1.5 rounded-lg bg-white/7 px-2 focus-within:ring-1 focus-within:ring-accent/60">
          <Search16Regular className="size-3.5 shrink-0 text-white/40" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            {...focus.props}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                if (query) setQuery("");
                else focus.release(e.currentTarget);
              }
              if (e.key === "Enter" && shown[0]) selectProject(shown[0].id);
            }}
            placeholder="Filter"
            spellCheck={false}
            className="min-w-0 flex-1 bg-transparent text-[12px] text-white/85 outline-none placeholder:text-white/30"
          />
        </label>
      }
    >
      {shown.length === 0 && <p className="px-3 py-2.5 text-[12px] text-white/40">Nothing matches “{query}”.</p>}
      {shown.map((p) => {
        const open = p.tasks.filter((t) => t.status !== "done").length;
        return (
          <button
            key={p.id}
            type="button"
            onClick={() => selectProject(p.id)}
            className="group flex h-9 items-center gap-2.5 px-3 text-left hover:bg-white/[0.04]"
          >
            <span className="size-2 shrink-0 rounded-full" style={{ background: p.color }} />
            <span className="w-40 shrink-0 truncate text-[12.5px] text-white/85">{p.name}</span>
            <span className="min-w-0 flex-1 truncate text-[11px] text-white/30" title={p.path}>
              {p.path}
            </span>
            {p.errors.length > 0 && <Warning16Regular className="size-3.5 shrink-0 text-status-amber" aria-label="Has problems" />}
            {!p.hasRemote && <span className="shrink-0 text-[10.5px] text-white/30">local</span>}
            <span className="w-12 shrink-0 text-right text-[11px] text-white/40 tabular-nums">{open > 0 ? `${open} open` : ""}</span>
            <ChevronRight12Regular className="size-3 shrink-0 text-white/25 group-hover:text-white/60" />
          </button>
        );
      })}
    </Group>
  );
}
