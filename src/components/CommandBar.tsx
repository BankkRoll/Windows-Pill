import {
  Add16Regular,
  ArrowEnter20Regular,
  Checkmark16Regular,
  Dismiss12Regular,
  Mic16Filled,
  Mic16Regular,
  MicOff16Regular,
  Search16Regular,
  Sparkle16Regular,
  Stop16Filled,
} from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useKeyboardFocus } from "../hooks/useKeyboardFocus";
import { useVoice } from "../hooks/useVoice";
import { addTask, openSystemSettings, reportError } from "../lib/actions";
import { askAssistant, describeAction, runActions, stopAssistant, type AssistantAction, type AssistantPlan } from "../lib/assistant";
import { buildCommands, rankCommands, type Command } from "../lib/commands";
import { usePalette } from "../lib/theme";
import { dueLabel } from "../lib/format";
import { handleMarkdownClick, renderChatMarkdown } from "../lib/markdown";
import { modelLabel } from "../lib/models";
import { EVENTS, focusWindow, isTauri, subscribe, type Project } from "../lib/native";
import { fuzzyMatch, parseQuickAdd } from "../lib/quickadd";
import { toastTimeLeft } from "../lib/toast";
import { usePillStore } from "../store";
import { Checkbox } from "./controls";
import { Toast } from "./Toast";
import { Chip, priorityTone, type ChipTone } from "./ui";

/** Props for {@link CommandBar}. */
export interface CommandBarProps {
  /** `notch`: the hotkey-driven bar that closes when done. `panel`: the bar at the bottom of the panel. */
  variant: "notch" | "panel";
  /** Called when the notch variant should close. */
  onClose?: () => void;
}

const DUE_TONE: Record<string, ChipTone> = { overdue: "red", today: "amber", soon: "accent", later: "neutral" };

type Item = { kind: "task" } | { kind: "ask" } | { kind: "command"; command: Command };

type Assistant =
  | { state: "idle" }
  | { state: "working"; question: string; activity: string }
  | { state: "plan"; question: string; plan: AssistantPlan; picked: boolean[] }
  | { state: "error"; question: string; message: string };

/**
 * One input for everything: typing offers to add it as a task, to run a
 * matching command, or to ask the AI, which answers with actions to approve
 * or deny. `/` limits the list to commands; `?` asks the AI straight away.
 * Task shortcuts (`#project`, `!1`, `@friday`, `+tag`) show as chips.
 * The mic dictates into the input offline, so spoken shortcuts make chips too.
 */
export function CommandBar({ variant, onClose }: CommandBarProps) {
  const snapshot = usePillStore((s) => s.snapshot);
  const config = usePillStore((s) => s.config);
  const selectedId = usePillStore((s) => s.selectedProjectId);
  const setCommandTall = usePillStore((s) => s.setCommandTall);
  const showToast = usePillStore((s) => s.showToast);
  const toastUp = usePillStore((s) => s.toast !== null && toastTimeLeft(s.toast, Date.now()) > 0);
  const [text, setText] = useState("");
  const [index, setIndex] = useState(0);
  const [focused, setFocused] = useState(variant === "notch");
  const [assistant, setAssistant] = useState<Assistant>({ state: "idle" });
  const [shake, setShake] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const focus = useKeyboardFocus();
  const notch = variant === "notch";
  const voice = useVoice(setText);
  const listening = voice.state.phase === "listening";
  const voiceError = voice.state.phase === "error" ? voice.state.error : null;

  const selected = snapshot.projects.find((p) => p.id === selectedId) ?? null;
  const mode: "commands" | "ask" | "mixed" = text.startsWith("/") ? "commands" : text.startsWith("?") ? "ask" : "mixed";
  const query = mode === "mixed" ? text : text.slice(1).trimStart();

  const parsed = useMemo(() => parseQuickAdd(query), [query]);
  const matched = useMemo(
    () => (parsed.projectQuery ? fuzzyMatch(parsed.projectQuery, snapshot.projects) : null),
    [parsed.projectQuery, snapshot.projects],
  );
  const target: Project | null = parsed.projectQuery ? matched : notch ? null : selected;

  const palette = usePalette();
  const commands = useMemo(() => buildCommands(snapshot, config, selected, palette), [snapshot, config, selected, palette]);
  const ranked = useMemo(
    () => (query.trim() ? rankCommands(query, commands, mode === "commands" ? 10 : 5) : mode === "commands" ? commands.slice(0, 10) : []),
    [query, commands, mode],
  );

  const items: Item[] = useMemo(() => {
    if (mode === "commands") return ranked.map((command) => ({ kind: "command", command }));
    if (!query.trim()) return [];
    const head: Item[] = mode === "ask" ? [{ kind: "ask" }, { kind: "task" }] : [{ kind: "task" }, { kind: "ask" }];
    return [...head, ...ranked.map((command): Item => ({ kind: "command", command }))];
  }, [mode, query, ranked]);

  const listOpen = (focused && items.length > 0) || assistant.state !== "idle";
  useEffect(() => {
    if (notch) setCommandTall(listOpen);
  }, [notch, listOpen, setCommandTall]);
  useEffect(() => () => setCommandTall(false), [setCommandTall]);
  useEffect(() => setIndex(0), [text]);

  // Closing the bar (quick-add dismissed, panel collapsed) while the AI is
  // still thinking would leave it running with nobody to read the answer.
  const working = useRef(false);
  working.current = assistant.state === "working";
  useEffect(
    () => () => {
      if (working.current) void stopAssistant().catch(() => {});
    },
    [],
  );

  useEffect(
    () =>
      subscribe<{ activity: string }>(EVENTS.assistant, ({ activity }) =>
        setAssistant((a) => (a.state === "working" ? { ...a, activity } : a)),
      ),
    [],
  );

  const reset = () => {
    setText("");
    setAssistant({ state: "idle" });
  };
  const close = () => {
    reset();
    if (notch) onClose?.();
    else focus.release(inputRef.current);
  };

  const addAsTask = async () => {
    if (!parsed.title) {
      setShake((n) => n + 1);
      return;
    }
    const submitted = text;
    // Clear optimistically so typing the next task right away isn't wiped
    // when the save finishes; put the text back only if the save failed.
    setText("");
    const created = await addTask(target, { title: parsed.title, priority: parsed.priority, due: parsed.due, tags: parsed.tags });
    if (!created) {
      setText((current) => current || submitted);
      return;
    }
    if (notch) onClose?.();
  };

  const ask = async (question = query) => {
    const q = question.trim();
    if (!q) return;
    setAssistant({ state: "working", question: q, activity: "Thinking…" });
    try {
      const plan = await askAssistant(q, null);
      setAssistant({ state: "plan", question: q, plan, picked: plan.actions.map(() => true) });
    } catch (e) {
      const message = String(e);
      setAssistant(message === "Stopped" ? { state: "idle" } : { state: "error", question: q, message });
    }
  };

  const runItem = (item: Item | undefined) => {
    if (!item) return;
    if (item.kind === "task") void addAsTask();
    else if (item.kind === "ask") void ask();
    else {
      void Promise.resolve(item.command.run()).catch(reportError);
      close();
    }
  };

  const approve = async (plan: AssistantPlan, picked: boolean[]) => {
    const chosen = plan.actions.filter((_, i) => picked[i]);
    const done = await runActions(chosen);
    showToast({ message: `Done: ${done} of ${chosen.length} action${chosen.length === 1 ? "" : "s"}`, tone: "info" });
    close();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (listening) {
      if (e.key === "Escape" || e.key === "Enter") {
        e.preventDefault();
        if (e.key === "Escape") voice.cancel();
        else voice.finish();
      }
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      if (assistant.state === "working") void stopAssistant();
      else if (assistant.state !== "idle") setAssistant({ state: "idle" });
      else close();
    } else if (assistant.state === "plan" && e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void approve(assistant.plan, assistant.picked);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (assistant.state !== "idle") return;
      runItem(items[index] ?? (query.trim() ? { kind: "task" } : undefined));
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      if (items.length === 0) return;
      e.preventDefault();
      setIndex((i) => (i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length);
    } else if (e.key === "Tab" && query.trim() && assistant.state === "idle") {
      e.preventDefault();
      void ask();
    }
  };

  const input = (
    <motion.div
      key={shake}
      animate={shake ? { x: [0, -6, 6, -4, 4, 0] } : undefined}
      transition={{ duration: 0.3 }}
      className="flex h-9 shrink-0 items-center gap-2 rounded-xl bg-white/6 px-3 focus-within:bg-white/9"
    >
      {mode === "commands" ? (
        <Search16Regular className="size-4 shrink-0 text-white/40" />
      ) : mode === "ask" ? (
        <Sparkle16Regular className="size-4 shrink-0 text-accent" />
      ) : (
        <Add16Regular className="size-4 shrink-0 text-white/40" />
      )}
      <input
        ref={inputRef}
        autoFocus={notch}
        value={text}
        readOnly={listening}
        onChange={(e) => {
          voice.dismiss();
          setText(e.target.value);
        }}
        onKeyDown={onKeyDown}
        {...focus.props}
        onFocus={() => {
          focus.props.onFocus();
          setFocused(true);
        }}
        onBlur={(e) => {
          focus.props.onBlur();
          // Clicks inside the list blur the input first; let them land.
          window.setTimeout(() => setFocused(false), 150);
          if (notch && !e.currentTarget.value && assistant.state === "idle" && !listening) onClose?.();
        }}
        spellCheck={false}
        placeholder={listening ? "Listening… Enter to finish, Esc to cancel" : "Add a task, type / for commands, or ? to ask AI"}
        aria-label="Command bar"
        className="min-w-0 flex-1 bg-transparent text-[13px] text-white/92 outline-none placeholder:text-white/30"
      />
      {voiceError ? (
        <span className="flex min-w-0 shrink items-center gap-1.5 text-[11.5px] text-status-red/90" title={voiceError.message}>
          <MicOff16Regular className="size-3.5 shrink-0" />
          <span className="max-w-[300px] truncate">{voiceError.message}</span>
          {voiceError.fix && (
            <button
              type="button"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => openSystemSettings(voiceError.fix!)}
              className="h-5 shrink-0 rounded-md bg-white/8 px-1.5 text-white/80 hover:bg-white/12 hover:text-white"
            >
              Settings
            </button>
          )}
        </span>
      ) : notch && toastUp ? (
        <Toast variant="inline" />
      ) : (
        mode !== "commands" &&
        query && (
          <div className="flex shrink-0 items-center gap-1">
            {parsed.projectQuery && !matched ? (
              <Chip tone="amber">No match · Inbox</Chip>
            ) : (
              <Chip dot={target?.color}>{target?.name ?? "Inbox"}</Chip>
            )}
            {parsed.priority && <Chip tone={priorityTone(parsed.priority)}>P{parsed.priority}</Chip>}
            {parsed.due && <Chip tone={DUE_TONE[dueLabel(parsed.due).tone]}>{dueLabel(parsed.due).text}</Chip>}
            {parsed.tags.slice(0, 2).map((t) => (
              <Chip key={t}>{t}</Chip>
            ))}
          </div>
        )
      )}
      {isTauri && (
        <button
          type="button"
          title={listening ? "Stop and keep what you said" : "Dictate a task (offline)"}
          aria-label={listening ? "Stop dictating" : "Dictate a task"}
          aria-pressed={listening}
          // Keep the caret in the input so Enter and Esc reach it.
          onPointerDown={(e) => {
            e.preventDefault();
            void focusWindow();
          }}
          onClick={() => {
            if (listening) {
              voice.finish();
              return;
            }
            voice.dismiss();
            inputRef.current?.focus();
            void voice.start(text);
          }}
          className={`grid size-6 shrink-0 place-items-center rounded-md transition-colors ${
            listening ? "bg-accent text-on-accent" : "text-white/40 hover:bg-white/8 hover:text-white/85"
          }`}
        >
          {listening ? <Mic16Filled className="size-3.5 animate-pulse" /> : <Mic16Regular className="size-3.5" />}
        </button>
      )}
    </motion.div>
  );

  const panel = (
    <div className={`flex min-h-0 flex-col ${notch ? "mt-1.5 flex-1" : ""}`}>
      {assistant.state === "idle" ? (
        <ItemList items={items} index={index} setIndex={setIndex} run={runItem} query={query} target={target} />
      ) : (
        <AssistantView
          assistant={assistant}
          onPick={(picked) => assistant.state === "plan" && setAssistant({ ...assistant, picked })}
          onApprove={() => assistant.state === "plan" && void approve(assistant.plan, assistant.picked)}
          onDeny={() => setAssistant({ state: "idle" })}
          onRetry={() => assistant.state === "error" && void ask(assistant.question)}
        />
      )}
      <footer className="flex shrink-0 items-center gap-3 border-t border-white/[0.06] px-3 py-1.5 text-[10.5px] text-white/30">
        {assistant.state === "plan" ? (
          <span>Ctrl+Enter approve · Esc deny</span>
        ) : assistant.state === "working" ? (
          <span>Esc to stop</span>
        ) : (
          <>
            <span>↑↓ choose · Enter run · Tab ask AI · Esc close</span>
            <span className="ml-auto">#project · !1 priority · @fri due · +tag</span>
          </>
        )}
      </footer>
    </div>
  );

  if (notch) {
    return (
      <div className="flex h-full w-[600px] flex-col px-2.5 pt-[9px]">
        {input}
        {listOpen && panel}
      </div>
    );
  }
  return (
    <div className="relative">
      <AnimatePresence>
        {listOpen && (
          <motion.div
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 6 }}
            transition={{ duration: 0.14 }}
            className="absolute right-0 bottom-full left-0 z-30 mb-2 flex max-h-[330px] flex-col overflow-hidden rounded-xl surface-menu shadow-[0_16px_48px] shadow-shade/60 ring-1 ring-white/10"
            onMouseDown={(e) => e.preventDefault()}
          >
            {panel}
          </motion.div>
        )}
      </AnimatePresence>
      {input}
    </div>
  );
}

function ItemList({
  items,
  index,
  setIndex,
  run,
  query,
  target,
}: {
  items: Item[];
  index: number;
  setIndex: (i: number) => void;
  run: (item: Item) => void;
  query: string;
  target: Project | null;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  let lastGroup = "";
  return (
    <div ref={ref} className="scroll-fade flex min-h-0 flex-1 flex-col overflow-y-auto p-1">
      {items.map((item, i) => {
        const heading = item.kind === "command" && item.command.group !== lastGroup ? item.command.group : null;
        if (item.kind === "command") lastGroup = item.command.group;
        return (
          <div key={item.kind === "command" ? item.command.id : item.kind}>
            {heading && <p className="px-2.5 pt-2 pb-0.5 text-[10.5px] font-semibold tracking-wider text-white/30 uppercase">{heading}</p>}
            <Row
              index={i}
              active={i === index}
              onHover={() => setIndex(i)}
              onClick={() => run(item)}
              icon={
                item.kind === "task" ? (
                  <Add16Regular className="size-4" />
                ) : item.kind === "ask" ? (
                  <Sparkle16Regular className="size-4 text-accent" />
                ) : item.command.color ? (
                  <span className="grid size-4 place-items-center">
                    <span className="size-2 rounded-full" style={{ background: item.command.color }} />
                  </span>
                ) : (
                  <ArrowEnter20Regular className="size-4" />
                )
              }
              title={
                item.kind === "task" ? (
                  <>
                    Add task <span className="text-white/55">“{parseQuickAdd(query).title || query}”</span>
                  </>
                ) : item.kind === "ask" ? (
                  <>
                    Ask AI <span className="text-white/55">“{query}”</span>
                  </>
                ) : (
                  item.command.title
                )
              }
              hint={
                item.kind === "task"
                  ? `to ${target?.name ?? "Inbox"}`
                  : item.kind === "ask"
                    ? "Reads your projects, then proposes tasks and agents to approve"
                    : item.command.hint
              }
              shortcut={item.kind === "ask" ? "Tab" : i === index ? "Enter" : undefined}
            />
          </div>
        );
      })}
    </div>
  );
}

function Row({
  index,
  active,
  onHover,
  onClick,
  icon,
  title,
  hint,
  shortcut,
}: {
  index: number;
  active: boolean;
  onHover: () => void;
  onClick: () => void;
  icon: ReactNode;
  title: ReactNode;
  hint?: string;
  shortcut?: string;
}) {
  return (
    <button
      type="button"
      data-index={index}
      onMouseMove={onHover}
      onClick={onClick}
      className={`flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left transition-colors ${active ? "bg-white/10" : ""}`}
    >
      <span className={active ? "text-white/85" : "text-white/45"}>{icon}</span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[12.5px] text-white/90">{title}</span>
        {hint && <span className="block truncate text-[11px] text-white/40">{hint}</span>}
      </span>
      {shortcut && <kbd className="shrink-0 rounded bg-white/8 px-1.5 py-px font-sans text-[10px] text-white/50">{shortcut}</kbd>}
    </button>
  );
}

function AssistantView({
  assistant,
  onPick,
  onApprove,
  onDeny,
  onRetry,
}: {
  assistant: Exclude<Assistant, { state: "idle" }>;
  onPick: (picked: boolean[]) => void;
  onApprove: () => void;
  onDeny: () => void;
  onRetry: () => void;
}) {
  const snapshot = usePillStore((s) => s.snapshot);
  const html = useMemo(() => (assistant.state === "plan" ? renderChatMarkdown(assistant.plan.reply) : ""), [assistant]);
  const [approving, setApproving] = useState(false);

  return (
    <div className="scroll-fade flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-3">
      <p className="flex items-center gap-1.5 text-[11.5px] text-white/45">
        <Sparkle16Regular className="size-3.5 text-accent" />
        <span className="truncate">{assistant.question}</span>
      </p>

      {assistant.state === "working" && (
        <div className="flex items-center gap-2 text-[12px] text-white/60">
          <span className="size-1.5 shrink-0 animate-pulse rounded-full bg-accent" />
          <span className="min-w-0 flex-1 truncate font-mono text-[11px]">{assistant.activity}</span>
          <button
            type="button"
            onClick={() => void stopAssistant()}
            className="inline-flex h-6 items-center gap-1 rounded-md px-2 text-[11px] text-white/60 hover:bg-white/8 hover:text-white"
          >
            <Stop16Filled className="size-2.5" /> Stop
          </button>
        </div>
      )}

      {assistant.state === "error" && (
        <div className="flex items-start gap-2 rounded-lg bg-status-red/10 px-2.5 py-2 ring-1 ring-status-red/20">
          <p className="min-w-0 flex-1 text-[12px] text-status-red/95 select-text">{assistant.message}</p>
          <button type="button" onClick={onRetry} className="h-6 shrink-0 rounded-md px-2 text-[11.5px] text-white/80 hover:bg-white/10">
            Retry
          </button>
        </div>
      )}

      {assistant.state === "plan" && (
        <>
          {assistant.plan.reply && (
            <article className="md chat text-[12.5px]" onClick={handleMarkdownClick} dangerouslySetInnerHTML={{ __html: html }} />
          )}
          {assistant.plan.actions.length > 0 ? (
            <ul className="flex flex-col gap-0.5 rounded-xl bg-white/[0.035] p-1 ring-1 ring-white/[0.07]">
              {assistant.plan.actions.map((a, i) => (
                <ActionRow
                  key={i}
                  action={a}
                  checked={assistant.picked[i] ?? false}
                  onToggle={(v) => onPick(assistant.picked.map((p, j) => (j === i ? v : p)))}
                  snapshot={snapshot}
                />
              ))}
            </ul>
          ) : (
            <p className="text-[11.5px] text-white/40">No actions proposed.</p>
          )}
          <div className="flex items-center gap-1.5">
            <span className="text-[10.5px] text-white/30">
              {assistant.plan.provider === "claude" ? "Claude Code" : "Codex"}
              {assistant.plan.model ? ` · ${modelLabel(assistant.plan.model)}` : ""}
            </span>
            <span className="flex-1" />
            <button
              type="button"
              onClick={onDeny}
              className="inline-flex h-7 items-center gap-1 rounded-lg px-2.5 text-[12px] text-white/65 hover:bg-white/8"
            >
              <Dismiss12Regular className="size-3" /> Deny
            </button>
            {assistant.plan.actions.length > 0 && (
              <button
                type="button"
                disabled={approving || !assistant.picked.some(Boolean)}
                onClick={() => {
                  setApproving(true);
                  onApprove();
                }}
                className="inline-flex h-7 items-center gap-1 rounded-lg bg-accent px-3 text-[12px] font-semibold text-on-accent disabled:opacity-40"
              >
                <Checkmark16Regular className="size-3.5" />
                {approving ? "Working…" : `Approve ${assistant.picked.filter(Boolean).length}`}
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}

const START_LABEL = { none: null, agent: "Agent", compare: "Compare" } as const;

function ActionRow({
  action,
  checked,
  onToggle,
  snapshot,
}: {
  action: AssistantAction;
  checked: boolean;
  onToggle: (v: boolean) => void;
  snapshot: ReturnType<typeof usePillStore.getState>["snapshot"];
}) {
  const d = describeAction(action, snapshot);
  const badge =
    action.type === "create_task"
      ? START_LABEL[action.start]
      : action.type === "start_agent"
        ? action.compare
          ? "Compare"
          : "Agent"
        : "Chat";
  return (
    <li>
      <label className="flex cursor-pointer items-start gap-2.5 rounded-lg px-2 py-1.5 hover:bg-white/[0.04]">
        <Checkbox checked={checked} onChange={onToggle} className="mt-0.5" />
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-1.5">
            <span className="truncate text-[12.5px] text-white/90">{d.title}</span>
            {action.type === "create_task" && action.priority && <Chip tone={priorityTone(action.priority)}>P{action.priority}</Chip>}
            {badge && <Chip tone="accent">{badge}</Chip>}
          </span>
          <span className="flex items-center gap-1.5 text-[11px] text-white/40">
            {d.color && <span className="size-1.5 rounded-full" style={{ background: d.color }} />}
            {d.where}
            {action.type === "create_task" && action.notes && <span className="truncate">· {action.notes}</span>}
          </span>
        </span>
      </label>
    </li>
  );
}
