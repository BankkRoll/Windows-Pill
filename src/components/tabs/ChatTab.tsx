import {
  Add16Regular,
  ArrowClockwise12Regular,
  ArrowDown16Regular,
  ArrowUp16Filled,
  BranchFork16Regular,
  Checkmark16Regular,
  ChevronRight12Regular,
  ChevronUp16Regular,
  Copy16Regular,
  Delete16Regular,
  Dismiss12Regular,
  Edit16Regular,
  Eye16Regular,
  MoreHorizontal16Regular,
  Notepad16Regular,
  Open16Regular,
  Sparkle16Regular,
  Stop16Filled,
  Wrench16Regular,
} from "@fluentui/react-icons";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useChatStore } from "../../chatStore";
import { useKeyboardFocus } from "../../hooks/useKeyboardFocus";
import { attempt, openLink, reportError } from "../../lib/actions";
import { handleMarkdownClick, renderChatMarkdown } from "../../lib/markdown";
import { toolSummary } from "../../lib/transcript";
import { INHERIT_RUN, inheritedChoice, modelLabel, useModelCatalog } from "../../lib/models";
import { api, chatApi, isDetached, type Chat, type ChatMessage, type ChatMode, type Project, type ProposedTask } from "../../lib/native";
import { usePillStore } from "../../store";
import { ApprovalCard } from "../ApprovalCard";
import { Checkbox } from "../controls";
import { ModelPicker } from "../ModelPicker";
import { NotesEditor } from "../NotesEditor";
import { hasProjectPages, navBadge, pluginTabId, pluginTabs, tabPlugin, type TabPlugin } from "../../lib/pages";
import { foldList } from "../../lib/plugins";
import { PageChipView } from "../PluginBlocks";
import { PluginIcon } from "../PluginIcon";
import { PluginPageView } from "../PluginTab";
import { ProjectChooser } from "../ProjectChooser";
import { Chip, priorityTone } from "../ui";

/** Sidebar entry for the project's notes; stored in place of a chat id. */
const NOTES = "notes";
// Plugin pages use their header tab id (`plugin:<id>`) the same way.

/** Plugin page entries listed under the project's notes before the rest fold into "+N more". */
const PAGE_ENTRIES = 4;

const SUGGESTIONS: readonly { text: string; mode: ChatMode }[] = [
  {
    text: "Find bugs in this project and propose a fix for each",
    mode: "read",
  },
  { text: "Explain how this codebase is structured", mode: "read" },
  { text: "What would you improve first, and why?", mode: "read" },
  { text: "Add tests for the least-tested module", mode: "edit" },
];

/**
 * Conversations with the project's AI about the selected project. Read-only
 * chats explore and plan; edit chats change code in their own worktree and
 * can ship a draft PR. Proposed tasks can be added or handed to agents.
 */
export function ChatTab() {
  const projects = usePillStore((s) => s.snapshot.projects);
  const selectedId = usePillStore((s) => s.selectedProjectId);
  const project = projects.find((p) => p.id === selectedId);
  if (!project) return <ProjectChooser purpose="Chats are about one project" />;
  return <ProjectChats key={project.id} project={project} />;
}

// -----------------------------------------------------------------------------
// Chat list
// -----------------------------------------------------------------------------

function ProjectChats({ project }: { project: Project }) {
  const chats = useChatStore((s) => s.byProject[project.id]);
  const openId = useChatStore((s) => s.openChat[project.id] ?? null);
  const refresh = useChatStore((s) => s.refresh);
  const open = useChatStore((s) => s.open);

  useEffect(() => {
    void refresh(project.id).catch(reportError);
  }, [project.id, refresh]);

  const plugins = usePillStore((s) => s.plugins);
  const pagePlugins = useMemo(() => pluginTabs(plugins).filter(hasProjectPages), [plugins]);
  const chat = chats?.find((c) => c.id === openId) ?? null;

  // A new chat is created by its first send, which then opens it. Keying
  // the conversation by chat id would remount it mid-send, dropping the
  // message being sent and the busy state that blocks a second send, so the
  // chat born from a draft keeps the draft's key. Starting another new chat
  // gets a fresh key.
  const [draft, setDraft] = useState<{ n: number; born: string | null }>({ n: 0, born: null });
  const [lastOpenId, setLastOpenId] = useState(openId);
  if (lastOpenId !== openId) {
    setLastOpenId(openId);
    if (openId === null) setDraft((d) => ({ n: d.n + 1, born: null }));
  }
  const conversationKey = openId === null || openId === draft.born ? `draft:${draft.n}` : openId;

  const notesOpen = openId === NOTES;
  const pagePlugin = openId ? (pagePlugins.find((p) => p.id === tabPlugin(openId)) ?? null) : null;
  const [allPages, setAllPages] = useState(false);
  const pageEntries = useMemo(
    () => foldList(pagePlugins, PAGE_ENTRIES, { keep: (p) => p.id === pagePlugin?.id, minHidden: 2 }),
    [pagePlugins, pagePlugin?.id],
  );
  const listedPages = allPages ? pagePlugins : pageEntries.shown;
  const hasNotes = project.notes.trim().length > 0;

  return (
    <div className="flex h-full gap-1 pb-1">
      <aside className={`flex shrink-0 flex-col gap-1 border-r border-white/[0.06] pr-2 ${isDetached ? "w-60" : "w-44"}`}>
        <button
          type="button"
          onClick={() => open(project.id, null)}
          className={`flex h-8 shrink-0 items-center gap-2 rounded-lg px-2 text-[12.5px] transition-colors ${
            openId === null ? "bg-white/10 text-white" : "text-white/70 hover:bg-white/6 hover:text-white/90"
          }`}
        >
          <Add16Regular className="size-4" /> New chat
        </button>
        <button
          type="button"
          onClick={() => open(project.id, NOTES)}
          title="Context, links and decisions. Agents and chats read these."
          className={`flex h-8 shrink-0 items-center gap-2 rounded-lg px-2 text-[12.5px] transition-colors ${
            notesOpen ? "bg-white/10 text-white" : "text-white/70 hover:bg-white/6 hover:text-white/90"
          }`}
        >
          <Notepad16Regular className="size-4" />
          <span className="flex-1 text-left">Project notes</span>
          {hasNotes && <span className="size-1.5 rounded-full bg-white/30" aria-label="Has notes" />}
        </button>
        {listedPages.map((p) => (
          <PageEntry
            key={p.id}
            plugin={p}
            project={project}
            active={pagePlugin?.id === p.id}
            onOpen={() => open(project.id, pluginTabId(p.id))}
          />
        ))}
        {pageEntries.hidden.length > 0 && (
          <button
            type="button"
            onClick={() => setAllPages((v) => !v)}
            aria-expanded={allPages}
            title={allPages ? undefined : pageEntries.hidden.map((p) => p.tab.title).join(", ")}
            className="flex h-7 shrink-0 items-center gap-2 rounded-lg px-2 text-[12px] text-white/45 transition-colors hover:bg-white/6 hover:text-white/80"
          >
            {allPages ? <ChevronUp16Regular className="size-4" /> : <MoreHorizontal16Regular className="size-4" />}
            <span className="flex-1 text-left">{allPages ? "Show fewer" : `${pageEntries.hidden.length} more`}</span>
          </button>
        )}
        <ChatList project={project} chats={chats ?? []} openId={openId} />
      </aside>
      {notesOpen ? (
        <div className="flex min-w-0 flex-1 flex-col px-2">
          <NotesEditor project={project} />
        </div>
      ) : pagePlugin ? (
        <div className="scroll-fade min-w-0 flex-1 overflow-y-auto px-2 pt-1 pb-3">
          <PluginPageView plugin={pagePlugin} project={project} />
        </div>
      ) : (
        <Conversation key={conversationKey} project={project} chat={chat} onCreated={(id) => setDraft((d) => ({ ...d, born: id }))} />
      )}
    </div>
  );
}

/** A plugin's page for this project, listed under the project's notes. */
function PageEntry({ plugin, project, active, onOpen }: { plugin: TabPlugin; project: Project; active: boolean; onOpen: () => void }) {
  const badge = navBadge(plugin.nav, project.id);
  return (
    <button
      type="button"
      onClick={onOpen}
      title={`${plugin.tab.title} for ${project.name}, from ${plugin.name}`}
      className={`flex h-8 shrink-0 items-center gap-2 rounded-lg px-2 text-[12.5px] transition-colors ${
        active ? "bg-white/10 text-white" : "text-white/70 hover:bg-white/6 hover:text-white/90"
      }`}
    >
      <PluginIcon icon={plugin.tab.icon} className="size-4" />
      <span className="min-w-0 flex-1 truncate text-left">{plugin.tab.title}</span>
      {badge && <PageChipView chip={badge} />}
    </button>
  );
}

function dayGroup(iso: string, now = new Date()): string {
  const d = new Date(iso);
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(now) - start(d)) / 86_400_000);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return "This week";
  return "Older";
}

function shortTime(iso: string): string {
  const d = new Date(iso);
  const sameDay = new Date().toDateString() === d.toDateString();
  return sameDay
    ? d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })
    : d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function ChatList({ project, chats, openId }: { project: Project; chats: readonly Chat[]; openId: string | null }) {
  const open = useChatStore((s) => s.open);
  const refresh = useChatStore((s) => s.refresh);
  const groups = useMemo(() => {
    const out = new Map<string, Chat[]>();
    for (const c of chats) {
      const g = dayGroup(c.updatedAt);
      out.set(g, [...(out.get(g) ?? []), c]);
    }
    return [...out.entries()];
  }, [chats]);

  const remove = async (c: Chat) => {
    const ok = await attempt(() => chatApi.remove(project.id, c.id).then(() => true));
    if (!ok) return;
    if (c.id === openId) open(project.id, null);
    await refresh(project.id);
  };

  if (chats.length === 0) {
    return (
      <p className="px-2 pt-2 text-[11.5px] leading-snug text-white/30">
        Chats you start appear here. They're saved in this repo's .pill folder.
      </p>
    );
  }

  return (
    <div className="scroll-fade flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto">
      {groups.map(([label, items]) => (
        <section key={label} className="flex flex-col gap-px">
          <h3 className="px-2 pt-1 pb-0.5 text-[10.5px] font-semibold tracking-wider text-white/30 uppercase">{label}</h3>
          {items.map((c) => (
            <div
              key={c.id}
              className={`group relative flex items-center rounded-lg transition-colors ${
                c.id === openId ? "bg-white/10" : "hover:bg-white/5"
              }`}
            >
              <button type="button" onClick={() => open(project.id, c.id)} className="flex min-w-0 flex-1 flex-col px-2 py-1.5 text-left">
                <span className={`truncate text-[12.5px] ${c.id === openId ? "text-white" : "text-white/80"}`}>{c.title}</span>
                <span className="flex items-center gap-1.5 text-[10.5px] text-white/35">
                  {c.mode === "edit" ? <Edit16Regular className="size-3" /> : <Eye16Regular className="size-3" />}
                  <span>{shortTime(c.updatedAt)}</span>
                  {c.busy && (
                    <span className="flex items-center gap-1 text-accent">
                      <span className="size-1.5 animate-pulse rounded-full bg-accent" /> answering
                    </span>
                  )}
                  {c.prUrl && <span className="text-status-green">PR open</span>}
                </span>
              </button>
              <button
                type="button"
                aria-label={`Delete ${c.title}`}
                title="Delete chat"
                disabled={c.busy}
                onClick={() => void remove(c)}
                className="mr-1 grid size-6 shrink-0 place-items-center rounded-md text-white/35 opacity-0 group-hover:opacity-100 hover:bg-white/10 hover:text-status-red focus-visible:opacity-100 disabled:hidden"
              >
                <Dismiss12Regular className="size-3" />
              </button>
            </div>
          ))}
        </section>
      ))}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Conversation
// -----------------------------------------------------------------------------

function Conversation({ project, chat, onCreated }: { project: Project; chat: Chat | null; onCreated: (chatId: string) => void }) {
  const created = useRef<string | null>(null);
  const streaming = useChatStore((s) => (chat ? s.streaming[chat.id] : undefined));
  const draftMode = useChatStore((s) => s.draftMode);
  const setDraftMode = useChatStore((s) => s.setDraftMode);
  const choiceKey = chat ? chat.id : `new:${project.id}`;
  const choice = useChatStore((s) => s.choices[choiceKey]) ?? chat?.choice ?? INHERIT_RUN;
  const setChoice = useChatStore((s) => s.setChoice);
  const refresh = useChatStore((s) => s.refresh);
  const open = useChatStore((s) => s.open);
  const config = usePillStore((s) => s.config);
  const [sending, setSending] = useState(false);
  const busy = !!streaming || !!chat?.busy || sending;
  // The picked provider wins; otherwise whoever answered last, else the project's.
  const provider = choice.provider ?? chat?.provider ?? project.settings.provider;
  const mode = chat?.mode ?? draftMode;
  const providerName = provider === "claude" ? "Claude" : "Codex";

  const messages = useMemo(() => chat?.messages ?? [], [chat]);
  const approvals = usePillStore((s) => s.approvals);
  const waiting = useMemo(() => approvals.filter((a) => chat && a.chatId === chat.id), [approvals, chat]);
  const { scroller, atBottom, toBottom } = useStickyScroll([messages.length, streaming?.text, streaming?.tools.length, waiting.length]);

  const send = async (body: string, startMode: ChatMode = mode) => {
    const message = body.trim();
    if (!message || busy) return false;
    setSending(true);
    try {
      // The chat list may not include a chat this conversation just created
      // yet; sending again must not create a second one.
      let chatId = chat?.id ?? created.current;
      if (!chatId) {
        chatId = (await chatApi.create(project.id, startMode)).id;
        created.current = chatId;
        setChoice(chatId, choice);
        onCreated(chatId);
        open(project.id, chatId);
      }
      await chatApi.send(project.id, chatId, message, choice);
      await refresh(project.id);
      toBottom();
      return true;
    } catch (e) {
      reportError(e);
      return false;
    } finally {
      setSending(false);
    }
  };

  const lastUser = [...messages].reverse().find((m) => m.role === "user");

  return (
    <section className="relative flex min-w-0 flex-1 flex-col">
      {chat && <ChatHeader project={project} chat={chat} busy={busy} />}

      <div ref={scroller} className="scroll-fade min-h-0 flex-1 overflow-y-auto px-3">
        <div className={`mx-auto flex w-full flex-col gap-4 py-3 ${isDetached ? "max-w-[760px]" : ""}`}>
          {!chat && (
            <EmptyState
              project={project}
              providerName={providerName}
              mode={mode}
              onPick={(s) => {
                setDraftMode(s.mode);
                void send(s.text, s.mode);
              }}
            />
          )}
          {messages.map((m, i) => (
            <Message
              key={m.id}
              message={m}
              project={project}
              provider={m.provider ?? provider}
              onRetry={
                i === messages.length - 1 && m.error && m.error !== "Stopped" && lastUser ? () => void send(lastUser.text) : undefined
              }
            />
          ))}
          {streaming && <Message message={streaming} project={project} provider={provider} live />}
          {sending && !streaming && (
            <div className="flex items-center gap-2 text-[12px] text-white/40">
              <span className="size-1.5 animate-pulse rounded-full bg-white/50" /> Starting {providerName}…
            </div>
          )}
          <AnimatePresence initial={false}>
            {waiting.map((a) => (
              <ApprovalCard key={a.id} approval={a} scope="chat" />
            ))}
          </AnimatePresence>
        </div>
      </div>

      <AnimatePresence>
        {!atBottom && (
          <motion.button
            type="button"
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 6 }}
            onClick={toBottom}
            className="absolute bottom-[92px] left-1/2 z-10 inline-flex h-7 -translate-x-1/2 items-center gap-1 rounded-full surface-menu px-3 text-[11.5px] text-white/80 shadow-[0_6px_20px] shadow-shade/50 ring-1 ring-white/10 hover:text-white"
          >
            <ArrowDown16Regular className="size-3.5" /> {busy ? "Follow reply" : "Latest"}
          </motion.button>
        )}
      </AnimatePresence>

      <div className={`px-3 pt-1 ${isDetached ? "pb-1" : ""}`}>
        <div className={`mx-auto w-full ${isDetached ? "max-w-[760px]" : ""}`}>
          <Composer
            placeholder={chat ? `Reply to ${providerName}…` : `Ask ${providerName} about ${project.name}…`}
            busy={busy}
            canStop={!!chat && busy}
            onStop={() => chat && void attempt(() => chatApi.stop(chat.id))}
            onSend={send}
            mode={mode}
            modeLocked={busy}
            onMode={(m) => {
              if (!chat) return setDraftMode(m);
              void attempt(() => chatApi.setMode(project.id, chat.id, m)).then(() => refresh(project.id));
            }}
            picker={
              <ModelPicker
                variant="chip"
                placement="up"
                provider={provider}
                value={choice}
                inherited={inheritedChoice(config, project, provider)}
                onChange={(c) => setChoice(choiceKey, { ...choice, ...c })}
                onProvider={(p) => setChoice(choiceKey, { provider: p, model: "", effort: "" })}
              />
            }
          />
        </div>
      </div>
    </section>
  );
}

/**
 * Keeps a scroller pinned to the bottom while new content arrives, unless
 * the user has scrolled up to read; then it stays put and offers a button.
 */
function useStickyScroll(deps: unknown[]) {
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const [atBottom, setAtBottom] = useState(true);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onScroll = () => {
      const near = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      pinned.current = near;
      setAtBottom(near);
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
    // eslint-disable-next-line react-hooks/exhaustive-deps -- callers pass content signals
  }, deps);

  const toBottom = () => {
    const el = scroller.current;
    if (!el) return;
    pinned.current = true;
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  };
  return { scroller, atBottom, toBottom };
}

function EmptyState({
  project,
  providerName,
  mode,
  onPick,
}: {
  project: Project;
  providerName: string;
  mode: ChatMode;
  onPick: (s: (typeof SUGGESTIONS)[number]) => void;
}) {
  return (
    <div className="flex flex-col items-center gap-4 pt-[6vh] text-center">
      <span className="grid size-10 place-items-center rounded-2xl bg-white/[0.06] ring-1 ring-white/[0.08]">
        <Sparkle16Regular className="size-5 text-white/70" />
      </span>
      <div>
        <h2 className="text-[16px] font-semibold text-white/92">Ask about {project.name}</h2>
        <p className="mx-auto mt-1 max-w-sm text-[12px] leading-relaxed text-white/45">
          {mode === "read"
            ? `${providerName} reads the code and answers without changing anything. It can propose tasks for agents.`
            : `${providerName} edits code in its own worktree, never your checkout, and can ship it as a draft PR.`}
        </p>
      </div>
      <div className="grid w-full max-w-lg grid-cols-2 gap-1.5">
        {SUGGESTIONS.map((s) => (
          <button
            key={s.text}
            type="button"
            onClick={() => onPick(s)}
            className="group flex items-start gap-2 rounded-xl bg-white/[0.04] px-3 py-2.5 text-left ring-1 ring-white/[0.05] transition-colors hover:bg-white/[0.07]"
          >
            {s.mode === "edit" ? (
              <Edit16Regular className="mt-px size-3.5 shrink-0 text-white/40 group-hover:text-white/70" />
            ) : (
              <Eye16Regular className="mt-px size-3.5 shrink-0 text-white/40 group-hover:text-white/70" />
            )}
            <span className="text-[12px] leading-snug text-white/70 group-hover:text-white/90">{s.text}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

// -----------------------------------------------------------------------------
// Composer
// -----------------------------------------------------------------------------

function Composer({
  placeholder,
  busy,
  canStop,
  onStop,
  onSend,
  mode,
  modeLocked,
  onMode,
  picker,
}: {
  placeholder: string;
  busy: boolean;
  canStop: boolean;
  onStop: () => void;
  onSend: (text: string) => Promise<boolean>;
  mode: ChatMode;
  modeLocked: boolean;
  onMode: (m: ChatMode) => void;
  picker: React.ReactNode;
}) {
  const [text, setText] = useState("");
  const focus = useKeyboardFocus();
  const area = useRef<HTMLTextAreaElement>(null);

  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, isDetached ? 220 : 120)}px`;
  }, [text]);

  const submit = async () => {
    const body = text;
    if (!body.trim() || busy) return;
    setText("");
    if (!(await onSend(body))) setText(body);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void submit();
    }
    if (e.key === "Escape") focus.release(e.currentTarget);
  };

  return (
    <div className="flex flex-col rounded-2xl bg-white/[0.06] ring-1 ring-white/[0.07] transition-shadow focus-within:ring-white/[0.16]">
      <textarea
        ref={area}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={onKeyDown}
        {...focus.props}
        rows={1}
        placeholder={placeholder}
        className="min-h-[38px] w-full resize-none bg-transparent px-3.5 pt-2.5 pb-1 text-[13px] leading-relaxed text-white/92 outline-none placeholder:text-white/30"
      />
      <div className="flex items-center gap-0.5 px-1.5 pb-1.5">
        <button
          type="button"
          disabled={modeLocked}
          onClick={() => onMode(mode === "read" ? "edit" : "read")}
          title={
            modeLocked ? "Wait for the reply to finish" : mode === "read" ? "Let it edit files (in its own branch)" : "Back to read-only"
          }
          className={`inline-flex h-7 items-center gap-1.5 rounded-lg px-2 text-[12px] transition-colors disabled:cursor-default ${
            mode === "edit" ? "text-status-amber" : "text-white/60"
          } ${modeLocked ? "" : "hover:bg-white/8"}`}
        >
          {mode === "edit" ? <Edit16Regular className="size-3.5" /> : <Eye16Regular className="size-3.5" />}
          {mode === "edit" ? "Can edit" : "Read-only"}
        </button>
        {picker}
        <span className="flex-1" />
        {!busy && text && <span className="mr-1.5 text-[10.5px] text-white/25">Enter to send · Shift+Enter for a new line</span>}
        {canStop ? (
          <button
            type="button"
            aria-label="Stop"
            title="Stop"
            onClick={onStop}
            className="grid size-7 place-items-center rounded-full bg-white/90 text-black transition-transform hover:scale-105"
          >
            <Stop16Filled className="size-3" />
          </button>
        ) : (
          <button
            type="button"
            aria-label="Send"
            title="Send"
            disabled={!text.trim() || busy}
            onClick={() => void submit()}
            className="grid size-7 place-items-center rounded-full bg-accent text-on-accent transition-[transform,opacity] hover:scale-105 disabled:scale-100 disabled:bg-white/10 disabled:text-white/30"
          >
            <ArrowUp16Filled className="size-4" />
          </button>
        )}
      </div>
    </div>
  );
}

// -----------------------------------------------------------------------------
// Header and messages
// -----------------------------------------------------------------------------

function ChatHeader({ project, chat, busy }: { project: Project; chat: Chat; busy: boolean }) {
  const refresh = useChatStore((s) => s.refresh);
  const open = useChatStore((s) => s.open);
  const [shipping, setShipping] = useState(false);
  return (
    <header className="flex h-10 shrink-0 items-center gap-2 border-b border-white/[0.06] px-3">
      <h2 className="min-w-0 flex-1 truncate text-[13px] font-semibold text-white/90" title={chat.title}>
        {chat.title}
      </h2>
      {chat.branch && (
        <span className="hidden items-center gap-1 text-[11px] text-white/40 sm:inline-flex" title="Branch with this chat's edits">
          <BranchFork16Regular className="size-3" />
          <code className="max-w-40 truncate">{chat.branch}</code>
        </span>
      )}
      {chat.prUrl ? (
        <button
          type="button"
          onClick={() => openLink(chat.prUrl!)}
          className="inline-flex h-7 items-center gap-1 rounded-lg bg-status-green/12 px-2.5 text-[12px] text-status-green hover:bg-status-green/18"
        >
          <Open16Regular className="size-3.5" /> View PR
        </button>
      ) : (
        chat.mode === "edit" &&
        chat.worktree && (
          <button
            type="button"
            disabled={busy || shipping}
            onClick={async () => {
              setShipping(true);
              const url = await attempt(() => chatApi.ship(project.id, chat.id));
              setShipping(false);
              if (url) await refresh(project.id);
            }}
            className="h-7 rounded-lg bg-accent px-2.5 text-[12px] font-semibold text-on-accent disabled:opacity-40"
          >
            {shipping ? "Opening PR…" : "Open draft PR"}
          </button>
        )
      )}
      <button
        type="button"
        aria-label="Delete chat"
        title="Delete chat"
        disabled={busy}
        onClick={async () => {
          const ok = await attempt(() => chatApi.remove(project.id, chat.id).then(() => true));
          if (ok) {
            open(project.id, null);
            await refresh(project.id);
          }
        }}
        className="grid size-7 place-items-center rounded-lg text-white/40 hover:bg-white/8 hover:text-status-red disabled:opacity-30"
      >
        <Delete16Regular className="size-3.5" />
      </button>
    </header>
  );
}

function Message({
  message,
  project,
  provider,
  live,
  onRetry,
}: {
  message: ChatMessage;
  project: Project;
  provider: Chat["provider"];
  live?: boolean;
  onRetry?: () => void;
}) {
  const [toolsOpen, setToolsOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const html = useMemo(() => renderChatMarkdown(message.text), [message.text]);
  const { catalog } = useModelCatalog();

  if (message.role === "user") {
    return (
      <div className="ml-auto max-w-[82%] rounded-2xl rounded-br-md bg-white/[0.09] px-3.5 py-2 text-[13px] leading-relaxed whitespace-pre-wrap text-white/92 select-text">
        {message.text}
      </div>
    );
  }

  const copyAll = async () => {
    if (await attempt(() => writeText(message.text).then(() => true))) {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    }
  };

  return (
    <div className="group flex flex-col gap-1.5">
      {message.tools.length > 0 && (
        <div className="flex flex-col gap-1">
          <button
            type="button"
            onClick={() => setToolsOpen((o) => !o)}
            className="inline-flex w-fit max-w-full items-center gap-1.5 rounded-md px-1.5 py-0.5 -ml-1.5 text-[11.5px] text-white/40 hover:bg-white/5 hover:text-white/70"
          >
            <ChevronRight12Regular className={`size-3 shrink-0 transition-transform ${toolsOpen ? "rotate-90" : ""}`} />
            <Wrench16Regular className="size-3 shrink-0" />
            {live ? (
              <span className="truncate">
                <span className="animate-breathe">{message.tools[message.tools.length - 1]}</span>
              </span>
            ) : (
              <span className="truncate">{toolSummary(message.tools)}</span>
            )}
          </button>
          <AnimatePresence initial={false}>
            {toolsOpen && (
              <motion.ul
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.15 }}
                className="ml-1 overflow-hidden border-l border-white/[0.08] pl-3"
              >
                {message.tools.map((t, i) => (
                  <li key={i} className="truncate py-px font-mono text-[10.5px] text-white/45 select-text" title={t}>
                    {t}
                  </li>
                ))}
              </motion.ul>
            )}
          </AnimatePresence>
        </div>
      )}

      {message.text ? (
        <article
          className={`md chat select-text ${live ? "streaming" : ""}`}
          onClick={handleMarkdownClick}
          dangerouslySetInnerHTML={{ __html: html }}
        />
      ) : (
        live && (
          <div className="flex items-center gap-1 py-1" aria-label="Thinking">
            {[0, 1, 2].map((i) => (
              <motion.span
                key={i}
                className="size-1.5 rounded-full bg-white/50"
                animate={{ opacity: [0.25, 1, 0.25] }}
                transition={{
                  duration: 1.1,
                  repeat: Infinity,
                  delay: i * 0.18,
                }}
              />
            ))}
          </div>
        )
      )}

      {message.error && message.error !== "Stopped" && (
        <div className="flex items-start gap-2 rounded-xl bg-status-red/10 px-3 py-2 ring-1 ring-status-red/20">
          <p className="min-w-0 flex-1 text-[12px] leading-snug text-status-red/95 select-text">{message.error}</p>
          {onRetry && (
            <button
              type="button"
              onClick={onRetry}
              className="inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-2 text-[11.5px] text-white/80 hover:bg-white/10"
            >
              <ArrowClockwise12Regular className="size-3" /> Retry
            </button>
          )}
        </div>
      )}
      {message.error === "Stopped" && <p className="text-[11.5px] text-white/35">Stopped</p>}
      {message.proposals.length > 0 && <Proposals project={project} proposals={message.proposals} />}

      {!live && (
        <div className="flex h-6 items-center gap-2 text-[11px] text-white/30 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          {(message.model || message.provider) && (
            <span>
              {message.provider === "codex" ? "Codex" : message.provider === "claude" ? "Claude" : ""}
              {message.model && message.provider ? " · " : ""}
              {message.model && modelLabel(message.model, catalog[provider])}
            </span>
          )}
          <span>{shortTime(message.at)}</span>
          {message.text && (
            <button
              type="button"
              onClick={() => void copyAll()}
              aria-label="Copy reply"
              title="Copy reply"
              className="grid size-6 place-items-center rounded-md hover:bg-white/8 hover:text-white/80"
            >
              {copied ? <Checkmark16Regular className="size-3.5 text-status-green" /> : <Copy16Regular className="size-3.5" />}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function Proposals({ project, proposals }: { project: Project; proposals: ProposedTask[] }) {
  const [picked, setPicked] = useState<boolean[]>(() => proposals.map(() => true));
  const [done, setDone] = useState<null | "added" | "started">(null);
  const [working, setWorking] = useState(false);
  const showToast = usePillStore((s) => s.showToast);
  const count = picked.filter(Boolean).length;

  const add = async (start: boolean) => {
    setWorking(true);
    let created = 0;
    for (const [i, p] of proposals.entries()) {
      if (!picked[i]) continue;
      const task = await attempt(() =>
        api.createTask(project.id, {
          title: p.title,
          notes: p.notes,
          priority: p.priority,
          status: "todo",
        }),
      );
      if (!task) continue;
      created++;
      if (start) await attempt(() => api.startAgent(project.id, task.id));
    }
    setWorking(false);
    setDone(start ? "started" : "added");
    showToast({
      message: start ? `Started ${created} agent${created === 1 ? "" : "s"}` : `Added ${created} task${created === 1 ? "" : "s"}`,
      tone: "info",
    });
  };

  return (
    <div className="flex flex-col overflow-hidden rounded-xl bg-white/[0.035] ring-1 ring-white/[0.08]">
      <header className="flex items-center justify-between px-3 pt-2.5 pb-1">
        <span className="text-[11px] font-semibold tracking-wider text-white/45 uppercase">
          {proposals.length} proposed task{proposals.length === 1 ? "" : "s"}
        </span>
        {!done && proposals.length > 1 && (
          <button
            type="button"
            onClick={() => setPicked(proposals.map(() => count !== proposals.length))}
            className="text-[11px] text-white/45 hover:text-white/80"
          >
            {count === proposals.length ? "Select none" : "Select all"}
          </button>
        )}
      </header>
      <ul className="flex flex-col px-1">
        {proposals.map((p, i) => (
          <li key={i}>
            <label className={`flex items-start gap-2.5 rounded-lg px-2 py-1.5 ${done ? "" : "cursor-pointer hover:bg-white/[0.04]"}`}>
              <Checkbox
                checked={picked[i] ?? false}
                disabled={done !== null}
                onChange={(on) => setPicked((prev) => prev.map((v, j) => (j === i ? on : v)))}
                className="mt-0.5"
              />
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-1.5">
                  <span className="text-[12.5px] text-white/90">{p.title}</span>
                  {p.priority && <Chip tone={priorityTone(p.priority)}>P{p.priority}</Chip>}
                </span>
                {p.notes && <span className="line-clamp-2 block text-[11.5px] leading-snug text-white/45">{p.notes}</span>}
              </span>
            </label>
          </li>
        ))}
      </ul>
      <footer className="flex items-center justify-end gap-1.5 border-t border-white/[0.06] px-2 py-2">
        {done ? (
          <p className="mr-auto px-1 text-[11.5px] text-white/55">
            {done === "started" ? "Agents are on it. Follow them in the Agents tab." : "Added to To do."}
          </p>
        ) : (
          <>
            <button
              type="button"
              disabled={count === 0 || working}
              onClick={() => void add(false)}
              className="h-7 rounded-lg px-2.5 text-[12px] text-white/75 hover:bg-white/8 disabled:opacity-30"
            >
              Add {count} task{count === 1 ? "" : "s"}
            </button>
            <button
              type="button"
              disabled={count === 0 || working}
              onClick={() => void add(true)}
              className="h-7 rounded-lg bg-accent px-2.5 text-[12px] font-semibold text-on-accent disabled:opacity-30"
            >
              {working ? "Starting…" : `Start ${count} agent${count === 1 ? "" : "s"}`}
            </button>
          </>
        )}
      </footer>
    </div>
  );
}
