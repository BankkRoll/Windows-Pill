/**
 * Small GitHub pieces shared by the board, the task page and the Agents tab:
 * the calm PR indicator (CI mark plus a word or two) and the issue card.
 */
import { Checkmark12Filled, Dismiss12Filled, Play16Filled } from "@fluentui/react-icons";
import type { DragEvent } from "react";
import { startIssueAgent } from "../lib/actions";
import { labelColor, prSignal, reviewText, type CheckMark, type IssueItem } from "../lib/github";
import type { PrStatus } from "../lib/native";
import { usePillStore } from "../store";
import { Chip } from "./ui";

/** The CI icon: a check, a cross, or a dot while checks run. */
export function CheckIcon({ mark }: { mark: CheckMark }) {
  if (mark === "pass") return <Checkmark12Filled className="size-3 shrink-0 text-status-green/80" />;
  if (mark === "fail") return <Dismiss12Filled className="size-3 shrink-0 text-status-red/80" />;
  if (mark === "pending") return <span className="mx-[3px] size-1.5 shrink-0 rounded-full bg-status-amber/70" />;
  return null;
}

/**
 * A PR's CI and review state in a few quiet words, e.g. a check and
 * "Approved", or a cross and "Changes requested". The full picture is in
 * the tooltip.
 *
 * @param props.pr - PR status from the sync.
 */
export function PrIndicator({ pr }: { pr: PrStatus }) {
  const signal = prSignal(pr);
  const review = reviewText(pr);
  const title = [`PR #${pr.number}`, pr.state === "open" ? signal.checks : signal.label, pr.state === "open" ? review : null]
    .filter(Boolean)
    .join(" · ");
  if (!signal.mark && !signal.label) return null;
  return (
    <span title={title} className="inline-flex h-5 shrink-0 items-center gap-1 text-[11px] text-white/50">
      <CheckIcon mark={signal.mark} />
      {signal.label}
    </span>
  );
}

/**
 * Two-line PR summary for detail views: state, CI and review each on their
 * own, so nothing hides in a tooltip.
 *
 * @param props.pr - PR status from the sync.
 */
export function PrDetails({ pr }: { pr: PrStatus }) {
  const signal = prSignal(pr);
  const review = reviewText(pr);
  const state = pr.state === "merged" ? "Merged" : pr.state === "closed" ? "Closed without merging" : pr.draft ? "Draft" : "Open";
  return (
    <div className="flex flex-col gap-0.5 text-[11.5px] text-white/60">
      <span>
        PR #{pr.number} · {state}
      </span>
      {pr.state === "open" && (
        <span className="inline-flex items-center gap-1.5">
          <CheckIcon mark={signal.mark} />
          {signal.checks}
        </span>
      )}
      {pr.state === "open" && review && <span>{review}</span>}
    </div>
  );
}

/** Props for {@link IssueCard}. */
export interface IssueCardProps {
  item: IssueItem;
  /** Show the project name (All projects view). */
  showProject: boolean;
  dragging: boolean;
  onDragStart: (e: DragEvent<HTMLDivElement>) => void;
  onDragEnd: () => void;
}

/**
 * A GitHub issue assigned to the user that isn't a task yet: number, title
 * and labels. Click opens it; dragging it into a column adds it as a task;
 * the play button starts an agent on it.
 */
export function IssueCard({ item, showProject, dragging, onDragStart, onDragEnd }: IssueCardProps) {
  const openIssue = usePillStore((s) => s.openIssue);
  const { project, issue } = item;
  return (
    <div
      draggable
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onClick={() => openIssue({ projectId: project.id, number: issue.number })}
      className={`group relative cursor-pointer rounded-[10px] px-2.5 py-2 ring-1 ring-white/[0.07] transition-[background-color,opacity] ring-inset hover:bg-white/5 ${
        dragging ? "opacity-35" : ""
      }`}
    >
      <p className="line-clamp-2 text-[12.5px] leading-snug text-white/80">
        <span className="mr-1 text-white/40 tabular-nums">#{issue.number}</span>
        {issue.title}
      </p>
      <button
        type="button"
        title="Start agent on this issue"
        aria-label="Start agent on this issue"
        onClick={(e) => {
          e.stopPropagation();
          void startIssueAgent(project, issue);
        }}
        className="surface-menu absolute top-1.5 right-1.5 grid size-6 place-items-center rounded-md text-white/70 opacity-0 shadow-sm ring-1 ring-white/10 transition-opacity group-hover:opacity-100 hover:text-white focus-visible:opacity-100"
      >
        <Play16Filled className="size-3" />
      </button>
      {(showProject || issue.labels.length > 0) && (
        <div className="mt-1.5 flex flex-wrap items-center gap-1">
          {showProject && (
            <span className="mr-0.5 inline-flex max-w-24 items-center gap-1 text-[11px] text-white/45">
              <span className="size-1.5 shrink-0 rounded-full" style={{ background: project.color }} />
              <span className="truncate">{project.name}</span>
            </span>
          )}
          {issue.labels.slice(0, 3).map((l) => (
            <Chip key={l.name} dot={labelColor(l.color)}>
              {l.name}
            </Chip>
          ))}
          {issue.labels.length > 3 && <span className="text-[11px] text-white/35">+{issue.labels.length - 3}</span>}
        </div>
      )}
    </div>
  );
}
