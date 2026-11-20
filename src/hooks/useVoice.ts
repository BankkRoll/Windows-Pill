import { useCallback, useEffect, useId, useRef, useState } from "react";
import { EVENTS, subscribe, voiceApi, type VoiceError, type VoiceUpdate } from "../lib/native";
import { toVoiceError, withTranscript } from "../lib/voice";
import { usePillStore } from "../store";

/** Where dictation is. */
export type VoiceState =
  | { phase: "idle" }
  /** `heard` is the best guess so far; empty until speech starts. */
  | { phase: "listening"; heard: string }
  | { phase: "error"; error: VoiceError };

/** How long a dictation error stays before the input goes back to normal. */
const ERROR_MS = 8_000;

/**
 * Offline dictation into a text field. While listening the panel is held
 * open, partial text streams into the field after what was already typed,
 * and the final transcript replaces it. Cancelling restores the typed text.
 *
 * @param setText - Writes the field's value.
 * @returns The state plus `start`, `finish` (keep what was heard),
 * `cancel` and `dismiss` (clear an error).
 */
export function useVoice(setText: (text: string) => void) {
  const [state, setState] = useState<VoiceState>({ phase: "idle" });
  const hold = usePillStore((s) => s.hold);
  const holdKey = `voice${useId()}`;
  const typed = useRef("");
  // Events can land after the transcript resolved; only a live session may write.
  const active = useRef(false);
  const setTextRef = useRef(setText);
  setTextRef.current = setText;
  const listening = state.phase === "listening";

  useEffect(() => {
    hold(holdKey, listening);
  }, [hold, holdKey, listening]);

  useEffect(
    () => () => {
      hold(holdKey, false);
      // The mic is shared by every window; only stop a session this field
      // started, or closing one window would cancel dictation in another.
      if (active.current) void voiceApi.stop(false).catch(() => {});
    },
    [hold, holdKey],
  );

  useEffect(() => {
    if (!listening) return;
    return subscribe<VoiceUpdate>(EVENTS.voice, (u) => {
      if (u.state !== "hearing" || !active.current) return;
      setState({ phase: "listening", heard: u.text });
      setTextRef.current(withTranscript(typed.current, u.text));
    });
  }, [listening]);

  useEffect(() => {
    if (state.phase !== "error") return;
    const t = window.setTimeout(() => setState({ phase: "idle" }), ERROR_MS);
    return () => window.clearTimeout(t);
  }, [state]);

  const start = useCallback(async (current: string) => {
    typed.current = current;
    active.current = true;
    setState({ phase: "listening", heard: "" });
    try {
      const transcript = await voiceApi.listen();
      active.current = false;
      setTextRef.current(withTranscript(typed.current, transcript));
      setState({ phase: "idle" });
    } catch (e) {
      const error = toVoiceError(e);
      active.current = false;
      // Busy means another bar owns the mic; leave this field's text alone.
      if (error.code !== "busy") setTextRef.current(typed.current);
      setState(error.code === "cancelled" ? { phase: "idle" } : { phase: "error", error });
    }
  }, []);

  const finish = useCallback(() => void voiceApi.stop(true).catch(() => {}), []);
  const cancel = useCallback(() => void voiceApi.stop(false).catch(() => {}), []);
  const dismiss = useCallback(() => setState((s) => (s.phase === "error" ? { phase: "idle" } : s)), []);

  return { state, start, finish, cancel, dismiss };
}
