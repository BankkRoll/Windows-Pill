/**
 * CVE watch's Security tab with the markup and classes of the app's
 * PluginTab.tsx: the project sidebar with the plugin's badges, and the
 * selected project's page drawn by {@link PluginBlocks}.
 */

import { PROJECTS, SECURITY_NAV, SECURITY_PAGE } from "./demo";
import { PageChipView, PluginBlocks, type PageChip } from "./PluginBlocks";

/** The Security tab with storefront selected. */
export function SecurityTab() {
  const selected = PROJECTS.storefront.id;
  return (
    <div className="flex h-full min-h-0 gap-2 pb-1">
      <nav aria-label="Security projects" className="flex w-44 shrink-0 flex-col gap-1 pt-1 pb-2">
        <ul className="flex min-h-0 flex-1 flex-col gap-px overflow-hidden">
          {SECURITY_NAV.map(({ project, badge }, i) => (
            <SidebarEntry
              key={project?.id ?? "all"}
              label={project?.name ?? "All projects"}
              color={project?.color}
              badge={badge}
              active={project?.id === selected}
              divider={i === 0}
            />
          ))}
        </ul>
      </nav>
      <div className="min-h-0 min-w-0 flex-1 overflow-hidden px-1 pt-1 pb-3">
        <PluginBlocks blocks={SECURITY_PAGE} />
      </div>
    </div>
  );
}

function SidebarEntry({
  label,
  color,
  badge,
  active,
  divider,
}: {
  label: string;
  color?: string;
  badge: PageChip | null;
  active: boolean;
  divider: boolean;
}) {
  return (
    <>
      <li className="shrink-0">
        <span
          className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[12px] ${
            active ? "bg-white/10 text-white/92" : "text-white/70"
          }`}
        >
          <span
            aria-hidden
            className="size-2 shrink-0 rounded-full"
            style={{ background: color ?? "conic-gradient(#2F7BFF, #30D158, #FFB340, #FF453A, #2F7BFF)" }}
          />
          <span className={`min-w-0 flex-1 truncate ${active ? "font-medium" : ""}`}>{label}</span>
          {badge && <PageChipView chip={badge} />}
        </span>
      </li>
      {divider && <li aria-hidden className="mx-2 my-1 h-px shrink-0 bg-white/8" />}
    </>
  );
}
