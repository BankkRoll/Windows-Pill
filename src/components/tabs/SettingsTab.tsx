import {
  Bot16Regular,
  Folder16Regular,
  Options16Regular,
  PersonCircle16Regular,
  PuzzlePiece16Regular,
  type FluentIcon,
} from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useRef, useState } from "react";
import { usePillStore, type SettingsSection } from "../../store";
import { ProjectSettingsForm } from "../ProjectSettingsForm";
import { SettingsFooterContext } from "../settings/kit";
import { AccountSettings } from "../settings/AccountSettings";
import { AgentSettings } from "../settings/AgentSettings";
import { GeneralSettings } from "../settings/GeneralSettings";
import { PluginsSettings } from "../settings/PluginsSettings";
import { ProjectsSettings } from "../settings/ProjectsSettings";

const SECTIONS: readonly {
  id: Exclude<SettingsSection, "project">;
  label: string;
  icon: FluentIcon;
}[] = [
  { id: "projects", label: "Projects", icon: Folder16Regular },
  { id: "agents", label: "Agents", icon: Bot16Regular },
  { id: "accounts", label: "Accounts", icon: PersonCircle16Regular },
  { id: "plugins", label: "Plugins", icon: PuzzlePiece16Regular },
  { id: "general", label: "General", icon: Options16Regular },
];

/**
 * Settings: a rail of pages on the left and the open page on the right.
 * Picking a project in the header jumps to that project's page.
 */
export function SettingsTab() {
  const section = usePillStore((s) => s.settingsSection);
  const setSection = usePillStore((s) => s.setSettingsSection);
  const configError = usePillStore((s) => s.configError);
  const selectedId = usePillStore((s) => s.selectedProjectId);
  const selected = usePillStore((s) =>
    s.snapshot.projects.find((p) => p.id === s.selectedProjectId),
  );
  const pane = useRef<HTMLDivElement>(null);
  const [footer, setFooter] = useState<HTMLDivElement | null>(null);
  const firstRender = useRef(true);

  useEffect(() => {
    if (firstRender.current) {
      firstRender.current = false;
      if (selectedId && section === "projects") setSection("project");
      return;
    }
    setSection(selectedId ? "project" : "projects");
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only react to the header picker
  }, [selectedId]);

  useEffect(() => {
    pane.current?.scrollTo({ top: 0 });
  }, [section, selectedId]);

  const current: SettingsSection =
    section === "project" && !selected ? "projects" : section;

  return (
    <div className="flex h-full gap-3 pb-2">
      <nav
        className="flex w-36 shrink-0 flex-col gap-0.5 pt-1"
        aria-label="Settings sections"
      >
        {selected && (
          <>
            <RailItem
              active={current === "project"}
              onClick={() => setSection("project")}
              label={selected.name}
            >
              <span className="grid size-4 place-items-center">
                <span
                  className="size-2 rounded-full"
                  style={{ background: selected.color }}
                />
              </span>
            </RailItem>
            <div className="mx-2 my-1.5 h-px bg-white/[0.06]" />
          </>
        )}
        {SECTIONS.map((s) => (
          <RailItem
            key={s.id}
            active={current === s.id}
            onClick={() => setSection(s.id)}
            label={s.label}
          >
            <s.icon className="size-4" />
          </RailItem>
        ))}
      </nav>

      <div className="relative min-w-0 flex-1">
        <SettingsFooterContext.Provider value={footer}>
          <div
            ref={pane}
            className="scroll-fade h-full scroll-pb-16 overflow-y-auto pt-1 pr-1 pb-5 [scrollbar-gutter:stable]"
          >
            {configError && (
              <p className="mb-3 rounded-xl bg-status-red/12 px-3 py-2 text-[12px] text-status-red">
                config.json couldn't be read, so defaults are in use until you
                change a setting: {configError}
              </p>
            )}
            <AnimatePresence mode="wait" initial={false}>
              <motion.div
                key={
                  current === "project" ? `project:${selected?.id}` : current
                }
                initial={{ opacity: 0, x: 6 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.12 }}
              >
                {current === "project" && selected && (
                  <ProjectSettingsForm project={selected} />
                )}
                {current === "projects" && <ProjectsSettings />}
                {current === "agents" && <AgentSettings />}
                {current === "accounts" && <AccountSettings />}
                {current === "plugins" && <PluginsSettings />}
                {current === "general" && <GeneralSettings />}
              </motion.div>
            </AnimatePresence>
          </div>
        </SettingsFooterContext.Provider>
        <div
          ref={setFooter}
          className="pointer-events-none absolute inset-x-0 bottom-1 z-10 pr-3 *:pointer-events-auto"
        />
      </div>
    </div>
  );
}

function RailItem({
  active,
  onClick,
  label,
  children,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={`flex h-8 items-center gap-2 rounded-lg px-2 text-left text-[12.5px] transition-colors ${
        active
          ? "bg-white/10 text-white"
          : "text-white/55 hover:bg-white/5 hover:text-white/85"
      }`}
    >
      <span className={active ? "text-white/90" : "text-white/45"}>
        {children}
      </span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
    </button>
  );
}
