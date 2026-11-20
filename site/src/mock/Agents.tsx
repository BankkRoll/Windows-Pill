/**
 * The Agents tab with the markup and classes of the app's AgentsTab.tsx and
 * RunTranscript.tsx: the run list on the left, the selected run's
 * transcript, tests, pull request and changed files on the right.
 */

import {
  ArrowSync16Regular,
  Bot16Regular,
  BranchFork16Regular,
  Checkmark12Filled,
  CheckmarkCircle16Filled,
  ChevronRight12Regular,
  Code16Regular,
  DocumentText16Regular,
  ErrorCircle16Filled,
  HandRight16Filled,
  Open16Regular,
  Wrench16Regular,
  type FluentIcon,
} from "@fluentui/react-icons";
import type { ReactNode } from "react";
import { RUN_DETAIL, RUNS, type DemoRun, type DemoTranscript } from "./demo";
import { Chip } from "./ui";

function StateIcon({ state }: { state: DemoRun["state"] }) {
  if (state === "running") {
    return (
      <svg
        className="size-3.5 shrink-0 animate-spin text-accent [animation-duration:1.6s] motion-reduce:animate-none"
        viewBox="0 0 12 12"
        aria-hidden
      >
        <circle cx="6" cy="6" r="4.5" fill="none" stroke="currentColor" strokeOpacity="0.25" strokeWidth="1.5" />
        <path d="M6 1.5a4.5 4.5 0 0 1 4.5 4.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    );
  }
  if (state === "pr_open" || state === "committed") return <CheckmarkCircle16Filled className="size-3.5 shrink-0 text-status-green" />;
  if (state === "awaiting_approval") return <HandRight16Filled className="size-3.5 shrink-0 text-status-purple" />;
  return <ErrorCircle16Filled className="size-3.5 shrink-0 text-status-red" />;
}

/** The Agents tab, with the first run selected. */
export function AgentsTab() {
  return (
    <div className="flex h-full gap-2 pb-1">
      <div className="flex w-52 shrink-0 flex-col gap-1">
        <ul className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-hidden pr-1">
          {RUNS.map((r, i) => (
            <li key={r.id}>
              <div className={`flex w-full flex-col gap-0.5 rounded-lg px-2 py-1.5 text-left ${i === 0 ? "bg-white/10" : ""}`}>
                <span className="flex items-center gap-1.5">
                  <StateIcon state={r.state} />
                  <span className="truncate text-[12px] text-white/88">{r.title}</span>
                </span>
                <span className="flex items-center gap-1.5 pl-5 text-[11px] text-white/40">
                  <span className="size-1.5 shrink-0 rounded-full" style={{ background: r.project.color }} />
                  <span className="truncate">{r.project.name}</span>
                  <span className="ml-auto shrink-0">{r.label}</span>
                </span>
              </div>
            </li>
          ))}
        </ul>
      </div>
      <RunDetail />
    </div>
  );
}

function RunDetail() {
  const run = RUN_DETAIL;
  return (
    <section className="flex min-w-0 flex-1 flex-col gap-2 overflow-hidden">
      <header className="flex flex-col gap-1">
        <div className="flex items-center gap-2">
          <h2 className="min-w-0 flex-1 truncate font-display text-[14px] font-semibold text-white/92">{run.title}</h2>
          <Chip tone="green">PR open</Chip>
        </div>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-white/45">
          <span className="inline-flex min-w-0 items-center gap-1">
            <BranchFork16Regular className="size-3 shrink-0" />
            <code className="truncate">{run.branch}</code>
          </span>
          <span>{run.setup}</span>
          <span className="tabular-nums">{run.duration}</span>
          <span className="tabular-nums">{run.turns} turns</span>
          <span className="tabular-nums">
            <span className="text-status-green/90">+{run.diff.insertions}</span>{" "}
            <span className="text-status-red/90">−{run.diff.deletions}</span> <span>· {run.diff.files} files</span>
          </span>
          <span className="tabular-nums">{run.cost}</span>
        </div>
      </header>

      <div className="flex min-h-0 flex-1 gap-2">
        {/* NOTE: the app keeps a live transcript scrolled to the newest activity, so the mockup shows its end. */}
        <div className="fade-top flex min-w-0 flex-1 flex-col justify-end gap-2.5 overflow-hidden rounded-xl bg-white/[0.035] px-3 py-2.5">
          {run.transcript.map((block, i) => (
            <TranscriptBlock key={i} block={block} />
          ))}
        </div>
        <aside className="flex w-40 shrink-0 flex-col gap-2 overflow-hidden">
          <Section title="Tests">
            <p className="text-[12px] font-medium text-status-green">Passed</p>
          </Section>
          <Section title="Pull request">
            <div className="flex flex-col gap-0.5 text-[11.5px] text-white/65">
              <span className="inline-flex items-center gap-1.5">
                <Checkmark12Filled className="size-3 shrink-0 text-status-green/80" />
                All 4 checks passed
              </span>
              <span className="text-white/45">Draft</span>
            </div>
          </Section>
          <Section title={`Files · ${run.files.length}`}>
            <ul className="flex flex-col gap-0.5">
              {run.files.map((f) => (
                <li key={f} className="truncate font-mono text-[10.5px] text-white/65">
                  {f}
                </li>
              ))}
            </ul>
          </Section>
        </aside>
      </div>

      <footer className="flex flex-wrap items-center gap-1">
        <Action icon={ArrowSync16Regular} label="Retry with feedback" />
        <Action icon={Open16Regular} label="Open PR" />
        <Action icon={Code16Regular} label="Open worktree" />
        <Action icon={Bot16Regular} label="Task" />
      </footer>
    </section>
  );
}

function TranscriptBlock({ block }: { block: DemoTranscript }) {
  switch (block.kind) {
    case "brief":
      return <Disclosure icon={<DocumentText16Regular className="size-3 shrink-0" />} label="Instructions sent to the agent" />;
    case "tools":
      return <Disclosure icon={<Wrench16Regular className="size-3 shrink-0" />} label={block.summary} />;
    case "step":
      return (
        <p className="flex items-start gap-2 text-[11px] leading-snug text-white/40">
          <span className="mt-[0.6em] h-px w-3 shrink-0 bg-white/15" />
          <span className="min-w-0">{block.text}</span>
        </p>
      );
    case "message":
      return <article className="md chat text-[12.5px]" dangerouslySetInnerHTML={{ __html: block.html }} />;
    case "result":
      return (
        <p className={`flex items-center gap-1.5 text-[12px] font-medium ${block.ok ? "text-status-green" : "text-status-red"}`}>
          {block.ok ? <CheckmarkCircle16Filled className="size-3.5 shrink-0" /> : <ErrorCircle16Filled className="size-3.5 shrink-0" />}
          {block.text}
        </p>
      );
  }
}

function Disclosure({ icon, label }: { icon: ReactNode; label: string }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="-ml-1.5 inline-flex w-fit max-w-full items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[11.5px] text-white/40">
        <ChevronRight12Regular className="size-3 shrink-0" />
        {icon}
        <span className="truncate">{label}</span>
      </span>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[10.5px] font-medium tracking-wide text-white/40 uppercase">{title}</span>
      {children}
    </div>
  );
}

function Action({ icon: Icon, label }: { icon: FluentIcon; label: string }) {
  return (
    <span className="inline-flex h-7 items-center gap-1.5 rounded-lg px-2 text-[12px] text-white/70">
      <Icon className="size-3.5" />
      {label}
    </span>
  );
}
