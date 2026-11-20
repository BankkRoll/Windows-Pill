/**
 * Chat state: chats per project, the open chat, and replies streaming in.
 * Chats themselves live on disk in each repo; this mirrors what the core sends.
 */

import { create } from "zustand";
import { chatApi, type Chat, type ChatMessage, type ChatMode, type ChatUpdate, type RunChoice } from "./lib/native";

interface ChatStore {
  /** Chats per project id, most recent first. */
  byProject: Readonly<Record<string, readonly Chat[]>>;
  /** Open chat per project; `null` means a fresh, not-yet-created chat. */
  openChat: Readonly<Record<string, string | null>>;
  /** Mode chosen for the next new chat. */
  draftMode: ChatMode;
  /** Provider and model picked per chat id (or `new:<projectId>` for a chat not yet created). */
  choices: Readonly<Record<string, RunChoice>>;
  setChoice: (key: string, choice: RunChoice) => void;
  /** Assistant message being built, per chat id. */
  streaming: Readonly<Record<string, ChatMessage>>;
  refresh: (projectId: string) => Promise<void>;
  open: (projectId: string, chatId: string | null) => void;
  setDraftMode: (mode: ChatMode) => void;
  apply: (update: ChatUpdate) => void;
}

/** Global chat store. */
export const useChatStore = create<ChatStore>((set, get) => ({
  byProject: {},
  openChat: {},
  draftMode: "read",
  choices: {},
  setChoice: (key, choice) => set((s) => ({ choices: { ...s.choices, [key]: choice } })),
  streaming: {},
  refresh: async (projectId) => {
    const chats = await chatApi.list(projectId);
    set((s) => ({ byProject: { ...s.byProject, [projectId]: chats } }));
  },
  open: (projectId, chatId) => set((s) => ({ openChat: { ...s.openChat, [projectId]: chatId } })),
  setDraftMode: (draftMode) => set({ draftMode }),
  apply: ({ chatId, message, busy }) => {
    if (busy) {
      set((s) => ({ streaming: { ...s.streaming, [chatId]: message } }));
      return;
    }
    const projectId = Object.entries(get().byProject).find(([, chats]) => chats.some((c) => c.id === chatId))?.[0];
    set((s) => {
      const streaming = { ...s.streaming };
      delete streaming[chatId];
      return { streaming };
    });
    if (projectId) void get().refresh(projectId);
  },
}));
