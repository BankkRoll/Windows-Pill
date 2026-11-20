/**
 * Model catalog access and display helpers shared by Settings, the chat
 * composer and the Agents tab.
 */

import { useEffect, useState } from "react";
import { api, isTauri, type AiProvider, type ModelCatalog, type ModelChoice, type ModelInfo, type PillConfig, type Project, type RunChoice } from "./native";

const EMPTY: ModelCatalog = { claude: [], claudeDefault: null, claudeError: null, codex: [], codexError: null };

let cached: ModelCatalog | null = null;
let pending: Promise<ModelCatalog> | null = null;
const listeners = new Set<(c: ModelCatalog) => void>();

function load(refresh: boolean): Promise<ModelCatalog> {
  if (!isTauri) return Promise.resolve(EMPTY);
  if (!refresh && cached) return Promise.resolve(cached);
  if (!refresh && pending) return pending;
  pending = api
    .modelCatalog(refresh)
    .then((c) => {
      cached = c;
      listeners.forEach((l) => l(c));
      return c;
    })
    .finally(() => {
      pending = null;
    });
  return pending;
}

/**
 * The model catalog, loaded once per window and shared by every caller.
 *
 * @returns The catalog (empty until loaded), a loading flag and `refresh`,
 * which asks both CLIs again.
 */
export function useModelCatalog() {
  const [catalog, setCatalog] = useState<ModelCatalog>(cached ?? EMPTY);
  const [loading, setLoading] = useState(!cached);
  useEffect(() => {
    listeners.add(setCatalog);
    void load(false).finally(() => setLoading(false));
    return () => {
      listeners.delete(setCatalog);
    };
  }, []);
  const refresh = () => {
    setLoading(true);
    void load(true).finally(() => setLoading(false));
  };
  return { catalog, loading, refresh };
}

/**
 * Human label for a model id, using the catalog when it knows the model.
 *
 * @param id - Alias (`opus`), full id (`claude-opus-5-5`) or Codex slug.
 * @param models - Catalog entries for the provider.
 * @returns A short label such as `Opus 5.5` or `GPT-6.2-Sol`.
 * @example modelLabel("claude-sonnet-5-5") // "Sonnet 5.5"
 */
export function modelLabel(id: string, models: readonly ModelInfo[] = []): string {
  const known = models.find((m) => m.id === id);
  if (known) return known.label;
  const claude = /^claude-([a-z]+)-(\d+)(?:-(\d{1,2}))?(?:-\d{8})?(\[1m\])?$/.exec(id);
  if (claude) {
    const [, family = "", major, minor, wide] = claude;
    const name = family.charAt(0).toUpperCase() + family.slice(1);
    return `${name} ${major}${minor ? `.${minor}` : ""}${wide ? " 1M" : ""}`;
  }
  return id;
}

const EFFORT_LABELS: Record<string, string> = {
  none: "None",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra high",
  max: "Max",
  ultra: "Ultra",
};

/**
 * Display name for an effort level.
 *
 * @param effort - Effort id such as `xhigh`.
 * @returns `Extra high` and similar; unknown ids are capitalized.
 */
export function effortLabel(effort: string): string {
  return EFFORT_LABELS[effort] ?? effort.charAt(0).toUpperCase() + effort.slice(1);
}

/** Where an inherited value comes from, for "Default" hints. */
export type ChoiceSource = "project" | "global" | "cli";

/**
 * What a chat or run would use when it leaves model and effort empty: the
 * project's override, else the global default, else the CLI's own choice.
 *
 * @param config - App config.
 * @param project - Project, or `null` when choosing a global default.
 * @param provider - Provider the choice applies to.
 * @returns The inherited choice and where each part came from.
 */
export function inheritedChoice(
  config: PillConfig,
  project: Project | null,
  provider: AiProvider,
): { model: string; effort: string; modelFrom: ChoiceSource; effortFrom: ChoiceSource } {
  const global = config.models[provider];
  // Project overrides belong to the project's own provider.
  const own = project?.settings.provider === provider ? project.settings : undefined;
  const pick = (p: string | undefined, g: string): [string, ChoiceSource] =>
    p ? [p, "project"] : g ? [g, "global"] : ["", "cli"];
  const [model, modelFrom] = pick(own?.model, global.model);
  const [effort, effortFrom] = pick(own?.effort, global.effort);
  return { model, effort, modelFrom, effortFrom };
}

/**
 * Efforts a picker should offer for `model`, as its CLI reported them. A
 * model without effort support gets none; an unknown or default model gets
 * the provider's usual set.
 *
 * @param provider - Provider.
 * @param model - Selected model id, or empty for the default.
 * @param catalog - Model catalog.
 * @returns Effort ids, lowest first.
 */
export function effortsFor(provider: AiProvider, model: string, catalog: ModelCatalog): string[] {
  const list = catalog[provider];
  const found = list.find((m) => m.id === model);
  if (found) return found.efforts;
  return provider === "claude" ? ["low", "medium", "high", "xhigh", "max"] : ["low", "medium", "high", "xhigh"];
}

/** An empty choice: inherit both model and effort. */
export const INHERIT: ModelChoice = { model: "", effort: "" };

/** An empty run choice: the project's provider, inheriting model and effort. */
export const INHERIT_RUN: RunChoice = { provider: null, model: "", effort: "" };
