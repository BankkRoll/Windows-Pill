/**
 * A run as a conversation: the brief the agent was given, its messages,
 * folded tool calls, Pill's pipeline steps and the outcome. Follows new
 * activity unless the reader has scrolled up.
 */

import {
  CheckmarkCircle16Filled,
  ChevronRight12Regular,
  DocumentText16Regular,
  ErrorCircle16Filled,
  Wrench16Regular,
} from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { handleMarkdownClick, renderChatMarkdown } from "../lib/markdown";
import { api, type LogLine } from "../lib/native";
import { toolSummary, transcriptBlocks, type TranscriptBlock } from "../lib/transcript";

/**
 * The transcript of one agent run.
 *
 * @param runId - Run whose brief to load.
 * @param lines - The run's activity, oldest first.
 * @param active - Whether a worker still owns the run.
 */
export function RunTranscript({ runId, lines, active }: { runId: string; lines: readonly LogLine[]; active: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const blocks = useMemo(() => transcriptBlocks(lines), [lines]);

  useEffect(() => {
    const el = ref.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [lines.length]);

  return (
    <div
      ref={ref}
      onScroll={(e) => {
        const el = e.currentTarget;
        pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      }}
      className="scroll-fade flex min-w-0 flex-1 flex-col gap-2.5 overflow-y-auto rounded-xl bg-white/[0.035] px-3 py-2.5 select-text"
    >
      <Brief runId={runId} />
      {blocks.length === 0 && <p className="text-[12px] text-white/35">{active ? "Starting…" : "No activity recorded."}</p>}
      {blocks.map((b, i) => (
        <Block key={i} block={b} live={active && i === blocks.length - 1} />
      ))}
    </div>
  );
}

function Block({ block, live }: { block: TranscriptBlock; live: boolean }) {
  switch (block.kind) {
    case "message":
      return <Message text={block.text} />;
    case "user":
      return (
        <div className="ml-auto max-w-[85%] rounded-2xl rounded-br-md bg-white/[0.09] px-3 py-1.5 text-[12.5px] leading-relaxed whitespace-pre-wrap text-white/90">
          {block.text}
        </div>
      );
    case "tools":
      return <Tools tools={block.tools} live={live} />;
    case "step":
      return (
        <p className="flex items-start gap-2 text-[11px] leading-snug text-white/40">
          <span className="mt-[0.6em] h-px w-3 shrink-0 bg-white/15" />
          <span className="min-w-0">{block.text}</span>
        </p>
      );
    case "output":
      return (
        <pre className="max-h-48 overflow-auto rounded-lg bg-status-red/8 p-2 font-mono text-[10.5px] leading-snug whitespace-pre-wrap text-status-red/90">
          {block.text}
        </pre>
      );
    case "result":
      return (
        <p className={`flex items-center gap-1.5 text-[12px] font-medium ${block.ok ? "text-status-green" : "text-status-red"}`}>
          {block.ok ? <CheckmarkCircle16Filled className="size-3.5 shrink-0" /> : <ErrorCircle16Filled className="size-3.5 shrink-0" />}
          {block.text}
        </p>
      );
  }
}

function Message({ text }: { text: string }) {
  const html = useMemo(() => renderChatMarkdown(text), [text]);
  return <article className="md chat text-[12.5px]" onClick={handleMarkdownClick} dangerouslySetInnerHTML={{ __html: html }} />;
}

function Tools({ tools, live }: { tools: readonly string[]; live: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <Disclosure
      open={open}
      onToggle={() => setOpen((o) => !o)}
      icon={<Wrench16Regular className="size-3 shrink-0" />}
      label={live ? <span className="animate-breathe">{tools[tools.length - 1]}</span> : toolSummary(tools)}
    >
      <ul className="ml-1 border-l border-white/[0.08] pl-3">
        {tools.map((t, i) => (
          <li key={i} className="truncate py-px font-mono text-[10.5px] text-white/45" title={t}>
            {t}
          </li>
        ))}
      </ul>
    </Disclosure>
  );
}

/** The prompt the agent started from, folded away until asked for. */
function Brief({ runId }: { runId: string }) {
  const [open, setOpen] = useState(false);
  const [prompt, setPrompt] = useState<string | null | undefined>(undefined);

  useEffect(() => {
    if (!open || prompt !== undefined) return;
    void api
      .runPrompt(runId)
      .then(setPrompt)
      .catch(() => setPrompt(null));
  }, [open, prompt, runId]);

  return (
    <Disclosure
      open={open}
      onToggle={() => setOpen((o) => !o)}
      icon={<DocumentText16Regular className="size-3 shrink-0" />}
      label="Instructions sent to the agent"
    >
      {prompt === undefined ? (
        <p className="pl-4 text-[11px] text-white/35">Loading…</p>
      ) : prompt === null ? (
        <p className="pl-4 text-[11px] text-white/35">Not saved for this run.</p>
      ) : (
        <pre className="max-h-64 overflow-auto rounded-lg bg-white/[0.04] p-2 font-mono text-[10.5px] leading-relaxed whitespace-pre-wrap text-white/60">
          {prompt.trim()}
        </pre>
      )}
    </Disclosure>
  );
}

function Disclosure({
  open,
  onToggle,
  icon,
  label,
  children,
}: {
  open: boolean;
  onToggle: () => void;
  icon: ReactNode;
  label: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={open}
        className="-ml-1.5 inline-flex w-fit max-w-full items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[11.5px] text-white/40 select-none hover:bg-white/5 hover:text-white/70"
      >
        <ChevronRight12Regular className={`size-3 shrink-0 transition-transform ${open ? "rotate-90" : ""}`} />
        {icon}
        <span className="truncate">{label}</span>
      </button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.15 }}
            className="overflow-hidden"
          >
            {children}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
