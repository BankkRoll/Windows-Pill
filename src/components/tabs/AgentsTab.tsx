import {
  ArrowSync16Regular,
  Bot12Regular,
  Bot16Regular,
  BranchFork16Regular,
  CheckmarkCircle16Filled,
  Code16Regular,
  Delete16Regular,
  ErrorCircle16Filled,
  HandRight16Filled,
  Open16Regular,
  Stop16Regular,
} from "@fluentui/react-icons";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useKeyboardFocus } from "../../hooks/useKeyboardFocus";
import { useNow } from "../../hooks/useNow";
import { useRunLog } from "../../hooks/useRunLog";
import { attempt, openLink, openOutside } from "../../lib/actions";
import { prFor, prSignal, reviewText } from "../../lib/github";
import { modelLabel, useModelCatalog } from "../../lib/models";
import { ACTIVE_RUN_STATES, api, SHIPPED_RUN_STATES, type PrStatus, type RunRecord, type RunState } from "../../lib/native";
import { usePillStore } from "../../store";
import { ApprovalCard } from "../ApprovalCard";
import { displayedRun } from "../../lib/viewing";
import { CheckIcon } from "../GithubBits";
import { RunTranscript } from "../RunTranscript";
import { TestSetup } from "../TestSetup";
import { Chip, type ChipTone } from "../ui";
import { Empty } from "./TodayTab";

const STATE_LABEL: Record<RunState, string> = {
  queued: "Queued",
  preparing: "Preparing",
  running: "Running",
  awaiting_approval: "Plan ready",
  verifying: "Verifying",
  shipping: "Shipping",
  failed: "Failed",
  ready: "Ready to compare",
  pr_open: "PR open",
  pushed: "Pushed",
  committed: "Committed",
  stopped: "Stopped",
  interrupted: "Interrupted",
  answered: "Answered",
};

const STATE_TONE: Record<RunState, ChipTone> = {
  queued: "neutral",
  preparing: "accent",
  running: "accent",
  awaiting_approval: "amber",
  verifying: "accent",
  shipping: "accent",
  failed: "red",
  ready: "accent",
  pr_open: "green",
  pushed: "green",
  committed: "green",
  stopped: "neutral",
  interrupted: "red",
  answered: "green",
};

const isActive = (r: RunRecord) => ACTIVE_RUN_STATES.includes(r.state);

/** Runs that ended without a result, which "Clear" removes. */
const isDud = (r: RunRecord) => r.state === "failed" || r.state === "stopped" || r.state === "interrupted";

/**
 * Formats a duration in milliseconds as `42s`, `5m 03s` or `1h 12m`.
 *
 * @param ms - Duration.
 */
export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}

/**
 * Every agent run: a list on the left, and the selected run's live activity,
 * tests, files and actions on the right.
 */
export function AgentsTab() {
  const runs = usePillStore((s) => s.runs);
  const projects = usePillStore((s) => s.snapshot.projects);
  const selectedProjectId = usePillStore((s) => s.selectedProjectId);
  const selectedRunId = usePillStore((s) => s.selectedRunId);
  const selectRun = usePillStore((s) => s.selectRun);

  const visible = useMemo(
    () => (selectedProjectId ? runs.filter((r) => r.projectId === selectedProjectId) : runs),
    [runs, selectedProjectId],
  );
  const selected = displayedRun(runs, selectedProjectId, selectedRunId);
  const approvals = usePillStore((s) => s.approvals);
  const needsYou = useMemo(() => new Set(approvals.map((a) => a.runId).filter(Boolean)), [approvals]);

  if (visible.length === 0) {
    return (
      <Empty title="No agent runs yet">
        <p>
          Hover a task and press ▶ to start one. It works on its own branch, runs your checks, then opens a draft PR, pushes or commits
          locally, as the project's settings say.
        </p>
      </Empty>
    );
  }

  const duds = visible.filter(isDud).length;

  return (
    <div className="flex h-full gap-2 pb-1">
      <div className="flex w-52 shrink-0 flex-col gap-1">
        {duds > 0 && <ClearDuds count={duds} projectId={selectedProjectId} />}
        <ul className="scroll-fade flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto pr-1">
          {visible.map((r) => {
            const project = projects.find((p) => p.id === r.projectId);
            return (
              <li key={r.id}>
                <button
                  type="button"
                  onClick={() => selectRun(r.id)}
                  className={`flex w-full flex-col gap-0.5 rounded-lg px-2 py-1.5 text-left ${
                    selected?.id === r.id ? "bg-white/10" : "hover:bg-white/6"
                  }`}
                >
                  <span className="flex items-center gap-1.5">
                    <StateIcon state={r.state} />
                    <span className="truncate text-[12px] text-white/88">{r.taskTitle}</span>
                    {needsYou.has(r.id) && (
                      <span
                        className="ml-auto size-1.5 shrink-0 animate-breathe rounded-full bg-status-amber"
                        title="Needs your approval"
                      />
                    )}
                    {r.compareGroup && (
                      <span className="ml-auto shrink-0 rounded bg-white/8 px-1 text-[9.5px] text-white/50 uppercase">vs</span>
                    )}
                  </span>
                  <span className="flex items-center gap-1.5 pl-5 text-[11px] text-white/40">
                    <span className="size-1.5 shrink-0 rounded-full" style={{ background: project?.color }} />
                    <span className="truncate">{project?.name ?? "Unknown project"}</span>
                    {r.handsOff && (
                      <span title="Hands-off: the agent owns this task" className="inline-flex shrink-0 text-white/40">
                        <Bot12Regular className="size-3" aria-label="Hands-off" />
                      </span>
                    )}
                    <span className={`ml-auto shrink-0 ${needsYou.has(r.id) ? "text-status-amber" : ""}`}>
                      {needsYou.has(r.id) ? "Needs you" : STATE_LABEL[r.state]}
                    </span>
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </div>
      {selected && <RunDetail key={selected.id} run={selected} />}
    </div>
  );
}

function StateIcon({ state }: { state: RunState }) {
  if (ACTIVE_RUN_STATES.includes(state)) {
    return (
      <svg className="size-3.5 shrink-0 animate-spin text-accent [animation-duration:1.6s]" viewBox="0 0 12 12" aria-hidden>
        <circle cx="6" cy="6" r="4.5" fill="none" stroke="currentColor" strokeOpacity="0.25" strokeWidth="1.5" />
        <path d="M6 1.5a4.5 4.5 0 0 1 4.5 4.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    );
  }
  if (SHIPPED_RUN_STATES.includes(state)) return <CheckmarkCircle16Filled className="size-3.5 shrink-0 text-status-green" />;
  if (state === "answered") return <CheckmarkCircle16Filled className="size-3.5 shrink-0 text-status-green" />;
  if (state === "ready") return <CheckmarkCircle16Filled className="size-3.5 shrink-0 text-accent" />;
  if (state === "awaiting_approval") return <HandRight16Filled className="size-3.5 shrink-0 text-status-purple" />;
  if (state === "failed" || state === "interrupted") return <ErrorCircle16Filled className="size-3.5 shrink-0 text-status-red" />;
  return <Bot16Regular className="size-3.5 shrink-0 text-white/40" />;
}

function RunDetail({ run }: { run: RunRecord }) {
  const project = usePillStore((s) => s.snapshot.projects.find((p) => p.id === run.projectId));
  const lines = useRunLog(run.id);
  const markSeen = usePillStore((s) => s.markSeen);
  const openDetail = usePillStore((s) => s.openDetail);
  const approvals = usePillStore((s) => s.approvals);
  const waiting = useMemo(() => approvals.filter((a) => a.runId === run.id), [approvals, run.id]);
  const now = useNow(isActive(run) ? 1000 : 60_000);
  const [retrying, setRetrying] = useState(false);
  const pr = usePillStore((s) => prFor(s.github, run.projectId, run.prUrl));

  useEffect(() => {
    if (!isActive(run) && run.state !== "awaiting_approval") markSeen(run.id);
  }, [run, markSeen]);

  const end = run.endedAt ? Date.parse(run.endedAt) : now;
  const duration = formatDuration(end - Date.parse(run.resumedAt ?? run.startedAt));
  const canRetry = !isActive(run) && run.state !== "awaiting_approval" && run.state !== "ready" && !!run.claudeSessionId;
  const { catalog } = useModelCatalog();
  const catalogLabel = (id: string) => modelLabel(id, catalog[run.provider]);

  return (
    <section className="flex min-w-0 flex-1 flex-col gap-2 overflow-hidden">
      <header className="flex flex-col gap-1">
        <div className="flex items-center gap-2">
          <h2 className="min-w-0 flex-1 truncate font-display text-[14px] font-semibold text-white/92">{run.taskTitle}</h2>
          <Chip tone={STATE_TONE[run.state]}>{STATE_LABEL[run.state]}</Chip>
        </div>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-white/45">
          <span className="inline-flex items-center gap-1">
            <BranchFork16Regular className="size-3" />
            <code>{run.branch}</code>
          </span>
          <span>{setupLabel(run, catalogLabel)}</span>
          <span className="tabular-nums">{duration}</span>
          {run.turns !== null && <span className="tabular-nums">{run.turns} turns</span>}
          {run.diff && <DiffLabel diff={run.diff} />}
          {run.costUsd !== null && <span className="tabular-nums">${run.costUsd.toFixed(2)}</span>}
          {run.planMode && <span>plan mode</span>}
          {run.handsOff && (
            <span title="The agent owns this task: it moves the card and notifies you only when it needs you">hands-off</span>
          )}
          {run.fixAttempts > 0 && <span>{run.fixAttempts === 1 ? "1 fix attempt" : `${run.fixAttempts} fix attempts`}</span>}
        </div>
      </header>

      {run.error && run.state !== "stopped" && (
        <p className="rounded-lg bg-status-red/12 px-2.5 py-1.5 text-[12px] whitespace-pre-wrap text-status-red">{run.error}</p>
      )}

      {run.compareGroup && <CompareStrip run={run} />}

      {run.state === "awaiting_approval" && <PlanApproval run={run} />}

      {project && (SHIPPED_RUN_STATES.includes(run.state) || run.state === "answered") && (
        <TestSetup project={project} ran={run.testsDetected} />
      )}

      {waiting.map((a) => (
        <ApprovalCard key={a.id} approval={a} scope="run" />
      ))}

      <div className="flex min-h-0 flex-1 gap-2">
        <RunTranscript runId={run.id} lines={lines} active={isActive(run)} />
        <aside className="scroll-fade flex w-40 shrink-0 flex-col gap-2 overflow-y-auto">
          {run.tests.passed !== null && (
            <Section title="Tests">
              <p className={`text-[12px] font-medium ${run.tests.passed ? "text-status-green" : "text-status-red"}`}>
                {run.tests.passed ? "Passed" : "Failed"}
              </p>
              {!run.tests.passed && run.tests.summary && (
                <pre className="max-h-32 overflow-auto rounded-md bg-white/5 p-1.5 text-[10px] leading-snug whitespace-pre-wrap text-white/60">
                  {run.tests.summary}
                </pre>
              )}
            </Section>
          )}
          {pr && (
            <Section title="Pull request">
              <PrSummary pr={pr} />
            </Section>
          )}
          <Section title={`Files · ${run.filesChanged.length}`}>
            {run.filesChanged.length === 0 ? (
              <p className="text-[11px] text-white/35">None yet</p>
            ) : (
              <ul className="flex flex-col gap-0.5">
                {run.filesChanged.map((f) => (
                  <li key={f} className="truncate font-mono text-[10.5px] text-white/65" title={f}>
                    {f}
                  </li>
                ))}
              </ul>
            )}
          </Section>
        </aside>
      </div>

      {retrying ? (
        <RetryBox run={run} onDone={() => setRetrying(false)} />
      ) : (
        <footer className="flex flex-wrap items-center gap-1">
          {(isActive(run) || run.state === "awaiting_approval") && (
            <Action icon={Stop16Regular} label="Stop" danger onClick={() => void attempt(() => api.stopAgent(run.id))} />
          )}
          {run.state === "ready" && <ShipPick run={run} />}
          {canRetry && <Action icon={ArrowSync16Regular} label="Retry with feedback" onClick={() => setRetrying(true)} />}
          {run.prUrl && <Action icon={Open16Regular} label="Open PR" onClick={() => openLink(run.prUrl!)} />}
          {run.worktreePresent && (
            <Action icon={Code16Regular} label="Open worktree" onClick={() => void openOutside(() => api.openWorktree(run.id))} />
          )}
          {project?.tasks.some((t) => t.id === run.taskId) && (
            <Action icon={Bot16Regular} label="Task" onClick={() => openDetail(run.taskId)} />
          )}
          {!isActive(run) && run.state !== "awaiting_approval" && <DeleteRun runId={run.id} />}
        </footer>
      )}
    </section>
  );
}

/** The run's PR on GitHub: state, CI and review, synced in the background. */
function PrSummary({ pr }: { pr: PrStatus }) {
  const review = reviewText(pr);
  if (pr.state !== "open") {
    return <p className="text-[12px] text-white/70">{pr.state === "merged" ? "Merged" : "Closed without merging"}</p>;
  }
  const { mark, checks } = prSignal(pr);
  return (
    <div className="flex flex-col gap-0.5 text-[11.5px] text-white/65">
      <span className="inline-flex items-center gap-1.5">
        <CheckIcon mark={mark} />
        {checks}
      </span>
      {review && <span>{review}</span>}
      {pr.draft && <span className="text-white/45">Draft</span>}
    </div>
  );
}

/**
 * "Claude Code · Opus 5.5" for a run: provider, then the model it reported
 * (or was given), else the one picked at start, else nothing.
 */
function setupLabel(run: RunRecord, label: (id: string) => string = (id) => modelLabel(id)): string {
  const provider = run.provider === "claude" ? "Claude Code" : "Codex";
  const model = run.model ?? (run.choice.model || null);
  return model ? `${provider} · ${label(model)}` : provider;
}

function DiffLabel({ diff }: { diff: NonNullable<RunRecord["diff"]> }) {
  return (
    <span className="tabular-nums">
      <span className="text-status-green/90">+{diff.insertions}</span> <span className="text-status-red/90">−{diff.deletions}</span>{" "}
      <span>
        · {diff.files} file{diff.files === 1 ? "" : "s"}
      </span>
    </span>
  );
}

/** Ships a compared run that passed its tests; the others are discarded. */
function ShipPick({ run }: { run: RunRecord }) {
  const [busy, setBusy] = useState(false);
  return (
    <button
      type="button"
      disabled={busy}
      onClick={async () => {
        setBusy(true);
        await attempt(() => api.pickRun(run.id));
        setBusy(false);
      }}
      className="inline-flex h-7 items-center gap-1.5 rounded-lg bg-accent px-2.5 text-[12px] font-semibold text-on-accent disabled:opacity-40"
    >
      <CheckmarkCircle16Filled className="size-3.5" />
      {busy ? "Shipping…" : "Ship this one"}
    </button>
  );
}

/**
 * Every run of a comparison side by side, so the setups can be judged on
 * tests, change size, turns, cost and time before one is shipped.
 */
function CompareStrip({ run }: { run: RunRecord }) {
  const all = usePillStore((s) => s.runs);
  const selectRun = usePillStore((s) => s.selectRun);
  const { catalog } = useModelCatalog();
  const now = useNow(1000);
  const group = all.filter((r) => r.compareGroup === run.compareGroup);
  const pending = group.some((r) => isActive(r));
  const winner = group.find((r) => SHIPPED_RUN_STATES.includes(r.state));

  return (
    <div className="flex flex-col gap-1.5">
      <p className="text-[11px] text-white/45">
        {winner
          ? `Shipped the ${setupLabel(winner, (id) => modelLabel(id, catalog[winner.provider]))} version.`
          : pending
            ? "Comparing setups. Each works in its own worktree; nothing is pushed until you pick one."
            : "Pick the version to ship. The others are discarded."}
      </p>
      <div
        className="grid gap-1.5"
        style={{
          gridTemplateColumns: `repeat(${Math.min(group.length, 3)}, minmax(0, 1fr))`,
        }}
      >
        {group.map((r) => {
          const end = r.endedAt ? Date.parse(r.endedAt) : now;
          return (
            <button
              key={r.id}
              type="button"
              onClick={() => selectRun(r.id)}
              className={`flex flex-col gap-1 rounded-xl p-2 text-left ring-1 transition-colors ${
                r.id === run.id ? "bg-white/8 ring-white/20" : "bg-white/[0.03] ring-white/[0.06] hover:bg-white/[0.06]"
              }`}
            >
              <span className="flex items-center gap-1.5">
                <StateIcon state={r.state} />
                <span className="truncate text-[12px] text-white/90">{setupLabel(r, (id) => modelLabel(id, catalog[r.provider]))}</span>
              </span>
              <span className="flex flex-wrap gap-x-2 text-[11px] text-white/45">
                <span className={r.tests.passed === true ? "text-status-green" : r.tests.passed === false ? "text-status-red" : ""}>
                  {r.tests.passed === true ? "Tests pass" : r.tests.passed === false ? "Tests fail" : STATE_LABEL[r.state]}
                </span>
                {r.diff && <DiffLabel diff={r.diff} />}
                {r.turns !== null && <span className="tabular-nums">{r.turns} turns</span>}
                {r.costUsd !== null && <span className="tabular-nums">${r.costUsd.toFixed(2)}</span>}
                <span className="tabular-nums">{formatDuration(end - Date.parse(r.resumedAt ?? r.startedAt))}</span>
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function PlanApproval({ run }: { run: RunRecord }) {
  const [plan, setPlan] = useState(run.plan ?? "");
  const [busy, setBusy] = useState(false);
  const focus = useKeyboardFocus();
  return (
    <div className="flex flex-col gap-1.5 rounded-xl bg-status-purple/8 p-2 ring-1 ring-status-purple/25">
      <p className="text-[12px] font-medium text-status-purple">Review the plan. Edit it if you like, then approve to build.</p>
      <textarea
        value={plan}
        onChange={(e) => setPlan(e.target.value)}
        {...focus.props}
        rows={6}
        spellCheck={false}
        className="resize-none rounded-lg bg-well/40 p-2 font-mono text-[11px] leading-relaxed text-white/85 outline-none"
      />
      <div className="flex justify-end gap-1.5">
        <button
          type="button"
          disabled={busy || !plan.trim()}
          onClick={async () => {
            setBusy(true);
            focus.release();
            await attempt(() => api.approvePlan(run.id, plan));
            setBusy(false);
          }}
          className="h-7 rounded-lg bg-status-purple px-3 text-[12px] font-semibold text-on-accent disabled:opacity-40"
        >
          Approve and build
        </button>
      </div>
    </div>
  );
}

function RetryBox({ run, onDone }: { run: RunRecord; onDone: () => void }) {
  const [feedback, setFeedback] = useState("");
  const focus = useKeyboardFocus();
  const send = async () => {
    focus.release();
    await attempt(() => api.retryAgent(run.id, feedback));
    onDone();
  };
  return (
    <div className="flex items-end gap-1.5">
      <textarea
        autoFocus
        value={feedback}
        onChange={(e) => setFeedback(e.target.value)}
        {...focus.props}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void send();
          if (e.key === "Escape") {
            focus.release(e.currentTarget);
            onDone();
          }
        }}
        rows={2}
        placeholder={
          run.tests.passed === false
            ? "Optional: guidance for the fix. The failing test output is included automatically."
            : "What should it change? (Ctrl+Enter to send)"
        }
        className="min-w-0 flex-1 resize-none rounded-lg bg-white/7 p-2 text-[12px] text-white/85 outline-none placeholder:text-white/30"
      />
      <button
        type="button"
        onClick={() => void send()}
        className="h-8 shrink-0 rounded-lg bg-accent px-3 text-[12px] font-semibold text-on-accent"
      >
        Retry
      </button>
    </div>
  );
}

/**
 * Deletes the run after a second click: worktree, unshipped branch and
 * history all go, so the first click only arms it.
 */
function DeleteRun({ runId }: { runId: string }) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = window.setTimeout(() => setArmed(false), 3000);
    return () => window.clearTimeout(t);
  }, [armed]);
  return (
    <Action
      icon={Delete16Regular}
      label={armed ? "Delete run and its worktree?" : "Delete run"}
      danger={armed}
      onClick={() => (armed ? void attempt(() => api.deleteRun(runId)) : setArmed(true))}
    />
  );
}

function ClearDuds({ count, projectId }: { count: number; projectId: string | null }) {
  const [busy, setBusy] = useState(false);
  return (
    <div className="flex items-center justify-between px-2 text-[11px] text-white/40">
      <span>{count === 1 ? "1 run didn't finish" : `${count} runs didn't finish`}</span>
      <button
        type="button"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          await attempt(() => api.clearFailedRuns(projectId));
          setBusy(false);
        }}
        title="Delete failed, stopped and interrupted runs with their worktrees. Their tasks stay."
        className="rounded-md px-1.5 py-0.5 text-white/60 hover:bg-white/8 hover:text-white/90 disabled:opacity-40"
      >
        {busy ? "Clearing…" : "Clear"}
      </button>
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

function Action({
  icon: Icon,
  label,
  onClick,
  danger,
}: {
  icon: typeof Stop16Regular;
  label: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`inline-flex h-7 items-center gap-1.5 rounded-lg px-2 text-[12px] ${
        danger ? "text-status-red/90 hover:bg-status-red/12" : "text-white/70 hover:bg-white/8 hover:text-white/92"
      }`}
    >
      <Icon className="size-3.5" />
      {label}
    </button>
  );
}
