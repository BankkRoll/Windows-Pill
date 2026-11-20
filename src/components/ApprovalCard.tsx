import { ChevronDown12Regular, Edit16Regular, Globe16Regular, ShieldKeyhole16Regular, WindowConsole20Regular } from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useRef, useState } from "react";
import { useDismiss } from "../hooks/useDismiss";
import { useKeyboardFocus } from "../hooks/useKeyboardFocus";
import { reportError } from "../lib/actions";
import { api, focusWindow, type Approval, type ApprovalAnswer } from "../lib/native";

/** Props for {@link ApprovalCard}. */
export interface ApprovalCardProps {
  approval: Approval;
  /** What "for this session" covers, as the user sees it. */
  scope: "chat" | "run";
  /** Show where the request came from (task or chat title). */
  showOrigin?: boolean;
}

function ToolIcon({ tool }: { tool: string }) {
  if (tool === "Bash" || tool === "PowerShell" || tool === "Shell") return <WindowConsole20Regular className="size-4" />;
  if (/^(Edit|MultiEdit|Write|NotebookEdit)$/.test(tool)) return <Edit16Regular className="size-4" />;
  if (tool.startsWith("Web")) return <Globe16Regular className="size-4" />;
  return <ShieldKeyhole16Regular className="size-4" />;
}

/**
 * A tool use the agent wants to make, with the user's choices: allow once,
 * allow for the rest of the chat or run, always allow (saved to the
 * project's Claude settings, personal or shared), or deny with a note.
 */
export function ApprovalCard({ approval: a, scope, showOrigin }: ApprovalCardProps) {
  const [busy, setBusy] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [alwaysOpen, setAlwaysOpen] = useState(false);
  const [denying, setDenying] = useState(false);
  const [note, setNote] = useState("");
  const focus = useKeyboardFocus();

  const answer = async (ans: ApprovalAnswer) => {
    setBusy(true);
    try {
      await api.answerApproval(a.id, ans);
    } catch (e) {
      reportError(e);
      setBusy(false);
    }
  };

  const longDetail = !!a.detail && (a.detail.length > 140 || a.detail.includes("\n"));

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0 }}
      className="rounded-xl bg-status-amber/[0.06] p-2.5 ring-1 ring-status-amber/25"
    >
      <div className="flex items-start gap-2.5">
        <span className="mt-0.5 grid size-6 shrink-0 place-items-center rounded-md bg-status-amber/15 text-status-amber">
          <ToolIcon tool={a.tool} />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-[11px] font-medium text-status-amber/90">
            {a.provider === "claude" ? "Claude" : "Codex"} wants to {verb(a.tool)}
            {showOrigin && <span className="font-normal text-white/40"> · {a.origin}</span>}
          </p>
          <p className="mt-0.5 font-mono text-[12px] break-all text-white/90 select-text">{a.summary}</p>
          {a.detail && (
            <button
              type="button"
              onClick={() => longDetail && setExpanded((v) => !v)}
              className={`mt-1 block w-full text-left ${longDetail ? "" : "cursor-default"}`}
            >
              <pre
                className={`overflow-hidden rounded-md bg-well/25 px-2 py-1 font-mono text-[11px] whitespace-pre-wrap text-white/55 select-text ${
                  expanded ? "max-h-60 overflow-y-auto" : "max-h-[3.2em]"
                }`}
              >
                {a.detail}
              </pre>
            </button>
          )}
        </div>
      </div>

      {denying ? (
        <form
          className="mt-2 flex items-center gap-1.5"
          onSubmit={(e) => {
            e.preventDefault();
            void answer({ kind: "deny", message: note.trim() || null });
          }}
        >
          <input
            autoFocus
            value={note}
            onChange={(e) => setNote(e.target.value)}
            {...focus.props}
            placeholder="Optional: tell it what to do instead"
            className="h-7 min-w-0 flex-1 rounded-lg bg-white/6 px-2.5 text-[12px] text-white/90 outline-none placeholder:text-white/30 focus:bg-white/9"
          />
          <button
            type="button"
            onClick={() => setDenying(false)}
            className="h-7 rounded-lg px-2.5 text-[12px] text-white/55 hover:bg-white/8"
          >
            Back
          </button>
          <button
            type="submit"
            disabled={busy}
            className="h-7 rounded-lg bg-status-red/80 px-3 text-[12px] font-semibold text-on-accent disabled:opacity-40"
          >
            Deny
          </button>
        </form>
      ) : (
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            disabled={busy}
            onClick={() => setDenying(true)}
            className="h-7 rounded-lg px-2.5 text-[12px] text-white/60 hover:bg-white/8 hover:text-white/90 disabled:opacity-40"
          >
            Deny
          </button>
          <span className="flex-1" />
          {a.session && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void answer({ kind: "session" })}
              className="h-7 rounded-lg bg-white/8 px-2.5 text-[12px] text-white/80 hover:bg-white/12 disabled:opacity-40"
            >
              Allow for this {scope}
            </button>
          )}
          {a.rule && a.provider === "claude" && (
            <AlwaysButton
              rule={a.rule}
              busy={busy}
              open={alwaysOpen}
              setOpen={setAlwaysOpen}
              onPick={(shared) => void answer({ kind: "always", shared })}
            />
          )}
          {a.rule && a.provider === "codex" && (
            <button
              type="button"
              disabled={busy}
              title={`Saves a rule allowing commands starting with “${a.rule}” to your Codex rules`}
              onClick={() => void answer({ kind: "always", shared: false })}
              className="h-7 rounded-lg bg-white/8 px-2.5 text-[12px] text-white/80 hover:bg-white/12 disabled:opacity-40"
            >
              Always allow
            </button>
          )}
          <button
            type="button"
            disabled={busy}
            onClick={() => void answer({ kind: "once" })}
            className="h-7 rounded-lg bg-accent px-3 text-[12px] font-semibold text-on-accent disabled:opacity-40"
          >
            Allow once
          </button>
        </div>
      )}
    </motion.div>
  );
}

function verb(tool: string) {
  if (tool === "Bash" || tool === "PowerShell" || tool === "Shell") return "run a command";
  if (tool === "Write") return "write a file";
  if (/^(Edit|MultiEdit|NotebookEdit)$/.test(tool)) return "edit a file";
  if (tool === "WebFetch") return "fetch a page";
  if (tool === "WebSearch") return "search the web";
  return `use ${tool}`;
}

function AlwaysButton({
  rule,
  busy,
  open,
  setOpen,
  onPick,
}: {
  rule: string;
  busy: boolean;
  open: boolean;
  setOpen: (v: boolean) => void;
  onPick: (shared: boolean) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const onKeyDown = useDismiss(open, () => setOpen(false), [ref]);

  return (
    <div ref={ref} className="relative" onKeyDown={onKeyDown}>
      <button
        type="button"
        disabled={busy}
        onPointerDown={() => void focusWindow()}
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        className="inline-flex h-7 items-center gap-1 rounded-lg bg-white/8 px-2.5 text-[12px] text-white/80 hover:bg-white/12 disabled:opacity-40"
      >
        Always allow
        <ChevronDown12Regular className="size-3 text-white/45" />
      </button>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: 4, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 4, scale: 0.98 }}
            transition={{ duration: 0.12 }}
            className="surface-menu absolute right-0 bottom-full z-40 mb-1.5 w-72 origin-bottom-right rounded-xl p-1 shadow-[0_16px_48px] shadow-shade/55 ring-1 ring-white/10"
          >
            <p className="px-2.5 pt-1.5 pb-1 text-[10.5px] text-white/40">
              Saves <code className="rounded bg-white/8 px-1 font-mono text-white/70">{rule}</code> to this project's Claude Code settings,
              so the terminal and editor use it too.
            </p>
            <MenuRow title="Just me" hint=".claude/settings.local.json, not committed" onClick={() => onPick(false)} />
            <MenuRow title="Everyone on the repo" hint=".claude/settings.json, committed with the code" onClick={() => onPick(true)} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function MenuRow({ title, hint, onClick }: { title: string; hint: string; onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} className="flex w-full flex-col rounded-lg px-2.5 py-1.5 text-left hover:bg-white/8">
      <span className="text-[12.5px] text-white/90">{title}</span>
      <span className="text-[11px] text-white/40">{hint}</span>
    </button>
  );
}
