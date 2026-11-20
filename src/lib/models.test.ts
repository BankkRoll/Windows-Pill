import { describe, expect, it } from "vitest";
import { DEFAULT_CONFIG, type Project } from "./native";
import { effortLabel, effortsFor, inheritedChoice, modelLabel } from "./models";

describe("modelLabel", () => {
  it.each([
    ["claude-opus-5-5", "Opus 5.5"],
    ["claude-haiku-5-5", "Haiku 5.5"],
    ["claude-sonnet-4-5-20250929", "Sonnet 4.5"],
    ["claude-opus-5-5[1m]", "Opus 5.5 1M"],
    ["gpt-6.2-sol", "gpt-6.2-sol"],
  ])("%s → %s", (id, label) => {
    expect(modelLabel(id)).toBe(label);
  });

  it("prefers the catalog label", () => {
    expect(modelLabel("gpt-6.2-sol", [{ id: "gpt-6.2-sol", label: "GPT-6.2-Sol", description: "", efforts: [], defaultEffort: null }])).toBe(
      "GPT-6.2-Sol",
    );
  });
});

describe("inheritedChoice", () => {
  const config = { ...DEFAULT_CONFIG, models: { claude: { model: "sonnet", effort: "" }, codex: { model: "", effort: "" } } };
  const project = (model: string, effort: string) => ({ settings: { model, effort, provider: "claude" } }) as unknown as Project;

  it("takes the project over the global default", () => {
    expect(inheritedChoice(config, project("opus", "high"), "claude")).toEqual({
      model: "opus",
      effort: "high",
      modelFrom: "project",
      effortFrom: "project",
    });
  });

  it("ignores the project override for the other provider", () => {
    expect(inheritedChoice(config, project("opus", "high"), "codex")).toEqual({
      model: "",
      effort: "",
      modelFrom: "cli",
      effortFrom: "cli",
    });
  });

  it("falls back to global, then to the CLI", () => {
    expect(inheritedChoice(config, project("", ""), "claude")).toEqual({
      model: "sonnet",
      effort: "",
      modelFrom: "global",
      effortFrom: "cli",
    });
  });
});

describe("effortsFor", () => {
  const catalog = {
    claude: [{ id: "claude-haiku-4-5", label: "Haiku 4.5", description: "", efforts: [], defaultEffort: null }],
    claudeDefault: null,
    claudeError: null,
    codex: [{ id: "gpt-x", label: "X", description: "", efforts: ["low", "ultra"], defaultEffort: "low" }],
    codexError: null,
  };
  it("uses the model's own efforts", () => {
    expect(effortsFor("codex", "gpt-x", catalog)).toEqual(["low", "ultra"]);
  });
  it("falls back to the provider set", () => {
    expect(effortsFor("claude", "opus", catalog)).toEqual(["low", "medium", "high", "xhigh", "max"]);
  });
  it("offers nothing for models without effort", () => {
    expect(effortsFor("claude", "claude-haiku-4-5", catalog)).toEqual([]);
  });
  it("labels efforts", () => {
    expect(effortLabel("xhigh")).toBe("Extra high");
  });
});
