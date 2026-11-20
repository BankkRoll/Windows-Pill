/**
 * A GitHub issue assigned to the user, as its own page: title, labels and
 * the body rendered as sanitized markdown, with ways to act on it (add to
 * the board, start an agent, open on GitHub).
 */
import { Add12Regular, ArrowLeft16Regular, Open16Regular, Play16Filled } from "@fluentui/react-icons";
import { motion } from "framer-motion";
import { useEffect, useMemo, useState } from "react";
import { openLink, startIssueAgent, trackIssue } from "../lib/actions";
import { ago, labelColor } from "../lib/github";
import { handleMarkdownClick, renderIssueMarkdown } from "../lib/markdown";
import { INHERIT_RUN, inheritedChoice } from "../lib/models";
import { isDetached, type GhIssue, type Project, type RunChoice } from "../lib/native";
import { usePillStore } from "../store";
import { Toggle } from "./controls";
import { ModelPicker } from "./ModelPicker";
import { Chip, IconButton } from "./ui";

/**
 * The open issue's page. When the issue becomes a task (here or anywhere
 * else) the page switches to that task, so an issue never shows twice.
 */
export function IssueDetail() {
  const ref = usePillStore((s) => s.detailIssue);
  const project = usePillStore((s) => s.snapshot.projects.find((p) => p.id === ref?.projectId) ?? null);
  const issue = usePillStore((s) => (ref ? (s.github[ref.projectId]?.issues.find((i) => i.number === ref.number) ?? null) : null));
  const openIssue = usePillStore((s) => s.openIssue);
  const openDetail = usePillStore((s) => s.openDetail);
  const linked = project?.tasks.find((t) => t.issue?.number === ref?.number) ?? null;

  useEffect(() => {
    if (!ref) return;
    if (linked) openDetail(linked.id);
    else if (!project || !issue) openIssue(null);
  }, [ref, linked, project, issue, openDetail, openIssue]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (e.key === "Escape" && !typing && !e.defaultPrevented) openIssue(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openIssue]);

  if (!project || !issue || linked) return null;
  return <IssuePage key={`${project.id}#${issue.number}`} project={project} issue={issue} onClose={() => openIssue(null)} />;
}

function IssuePage({ project, issue, onClose }: { project: Project; issue: GhIssue; onClose: () => void }) {
  const html = useMemo(() => renderIssueMarkdown(issue.body), [issue.body]);
  const repo = usePillStore((s) => s.github[project.id]?.repo);

  return (
    <motion.div
      initial={{ opacity: 0, x: 10 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.16, ease: [0.2, 0.8, 0.2, 1] }}
      className="flex h-full flex-col"
    >
      <header className="flex h-9 shrink-0 items-center gap-1.5 px-1">
        <IconButton icon={ArrowLeft16Regular} label="Back (Esc)" onClick={onClose} />
        <span className="flex min-w-0 items-center gap-1.5 text-[12px] text-white/45">
          <span className="size-1.5 shrink-0 rounded-full" style={{ background: project.color }} />
          <span className="truncate">{project.name}</span>
          <span className="text-white/25">/</span>
          <span className="shrink-0">GitHub issue</span>
        </span>
        <span className="flex-1" />
        <IconButton icon={Open16Regular} label="Open on GitHub" onClick={() => openLink(issue.url)} />
      </header>

      <div className="scroll-fade min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        <div className={`mx-auto flex w-full flex-col gap-3 pt-1 ${isDetached ? "max-w-[680px]" : ""}`}>
          <h1 className="px-1 font-display text-[19px] leading-snug font-semibold text-white/95 select-text">
            <span className="mr-1.5 text-white/40 tabular-nums">#{issue.number}</span>
            {issue.title}
          </h1>

          <div className="flex flex-wrap items-center gap-1.5 px-1">
            {issue.labels.map((l) => (
              <Chip key={l.name} dot={labelColor(l.color)}>
                {l.name}
              </Chip>
            ))}
            <span className="text-[11.5px] text-white/40">
              {repo ? `${repo} · ` : ""}updated {ago(issue.updatedAt)}
            </span>
          </div>

          {issue.body.trim() ? (
            <article
              className="md chat px-1 text-[13px] select-text"
              onClick={handleMarkdownClick}
              dangerouslySetInnerHTML={{ __html: html }}
            />
          ) : (
            <p className="px-1 text-[12.5px] text-white/40">No description.</p>
          )}

          <IssueActions project={project} issue={issue} />
        </div>
      </div>
    </motion.div>
  );
}

function IssueActions({ project, issue }: { project: Project; issue: GhIssue }) {
  const config = usePillStore((s) => s.config);
  const [plan, setPlan] = useState(project.settings.planMode);
  const [choice, setChoice] = useState<RunChoice>(INHERIT_RUN);
  const provider = choice.provider ?? project.settings.provider;

  return (
    <section className="flex flex-col gap-2 rounded-xl bg-white/[0.04] p-3 ring-1 ring-white/[0.06]">
      <div className="flex items-center gap-2">
        <span className="text-[12.5px] font-medium text-white/85">Work on it</span>
        <span className="flex-1" />
        <button
          type="button"
          onClick={() => openLink(issue.url)}
          className="inline-flex shrink-0 items-center gap-1 text-[11.5px] text-accent hover:underline"
        >
          <Open16Regular className="size-3.5" /> Open on GitHub
        </button>
      </div>
      <p className="text-[11.5px] leading-snug text-white/45">
        Starting an agent adds this issue to the board as a task. The agent reads the issue, and its pull request closes it when merged.
      </p>
      <div className="flex flex-wrap items-center gap-1.5">
        <ModelPicker
          variant="chip"
          placement="up"
          provider={provider}
          value={choice}
          inherited={inheritedChoice(config, project, provider)}
          onChange={(c) => setChoice({ ...choice, ...c })}
          onProvider={(p) => setChoice({ provider: p, model: "", effort: "" })}
        />
        <label
          className="inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-lg px-2 text-[12px] text-white/60 hover:bg-white/5"
          title="The agent proposes a plan and waits for your approval before editing"
        >
          <Toggle checked={plan} onChange={setPlan} />
          Plan first
        </label>
        <span className="flex-1" />
        <button
          type="button"
          onClick={() => void trackIssue(project, issue)}
          title="Add it to To do without starting an agent"
          className="inline-flex h-7 items-center gap-1 rounded-lg px-2.5 text-[12px] text-white/70 hover:bg-white/8 hover:text-white"
        >
          <Add12Regular className="size-3" />
          Add to board
        </button>
        <button
          type="button"
          onClick={() => void startIssueAgent(project, issue, plan, choice)}
          className="inline-flex h-7 items-center gap-1.5 rounded-lg bg-accent px-3 text-[12px] font-semibold text-on-accent hover:brightness-110"
        >
          <Play16Filled className="size-3" />
          Start agent
        </button>
      </div>
    </section>
  );
}
