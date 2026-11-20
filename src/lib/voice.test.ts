import { describe, expect, it } from "vitest";
import { toVoiceError, withTranscript } from "./voice";

describe("withTranscript", () => {
  it("appends after typed text so typed chips still apply", () => {
    expect(withTranscript("", "Fix login")).toBe("Fix login");
    expect(withTranscript("#web ", "Fix login")).toBe("#web Fix login");
    expect(withTranscript("#web", "  ")).toBe("#web");
  });
});

describe("toVoiceError", () => {
  it("keeps errors from the core and wraps anything else", () => {
    expect(toVoiceError({ code: "micDenied", message: "Microphone access is off.", fix: "microphone" })).toEqual({
      code: "micDenied",
      message: "Microphone access is off.",
      fix: "microphone",
    });
    expect(toVoiceError("IPC broke")).toEqual({ code: "failed", message: "IPC broke", fix: null });
    expect(toVoiceError({ code: 3 })).toMatchObject({ code: "failed" });
  });
});
