/**
 * Pure helpers for voice quick-add: merging a transcript into what was
 * typed, and normalizing failures from the core.
 */

import { errorMessage, type VoiceError } from "./native";

/**
 * Puts a dictated transcript after whatever was already typed, so a project
 * chip typed first (`#web`) keeps applying.
 *
 * @param typed - Input text before dictation started.
 * @param transcript - What was heard.
 * @example withTranscript("#web", "Fix login") // "#web Fix login"
 */
export function withTranscript(typed: string, transcript: string): string {
  const t = transcript.trim();
  if (!t) return typed;
  const base = typed.trimEnd();
  return base ? `${base} ${t}` : t;
}

/**
 * Turns whatever `voice_listen` rejected with into a {@link VoiceError}.
 *
 * @param e - Rejection value: a serialized `VoiceError`, or an IPC error.
 */
export function toVoiceError(e: unknown): VoiceError {
  if (typeof e === "object" && e !== null && "code" in e && "message" in e) {
    const v = e as Partial<VoiceError>;
    if (typeof v.code === "string" && typeof v.message === "string") {
      return { code: v.code, message: v.message, fix: v.fix ?? null };
    }
  }
  return { code: "failed", message: errorMessage(e), fix: null };
}
