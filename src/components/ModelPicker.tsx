import { ArrowClockwise12Regular, Checkmark16Regular, ChevronDown12Regular } from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useDismiss } from "../hooks/useDismiss";
import { useKeyboardFocus } from "../hooks/useKeyboardFocus";
import { effortLabel, effortsFor, modelLabel, useModelCatalog, type ChoiceSource } from "../lib/models";
import { focusWindow, type AiProvider, type ModelChoice } from "../lib/native";
import { Segmented } from "./controls";

/** What an empty model or effort falls back to, shown on the "Default" row. */
export interface Inherited {
  model: string;
  effort: string;
  modelFrom: ChoiceSource;
  effortFrom: ChoiceSource;
}

const SOURCE: Record<ChoiceSource, string> = {
  project: "project setting",
  global: "your default",
  cli: "CLI default",
};

/**
 * Model and reasoning-effort picker in the style of Claude Code's own: a
 * compact trigger opening a menu of versioned models, a "Default" row that
 * inherits, an effort dot slider, and an optional custom model id.
 *
 * @param props.provider - Which CLI's models to list.
 * @param props.value - Current choice; empty strings mean "inherit".
 * @param props.inherited - What an empty choice resolves to, for labels.
 * @param props.onChange - Called with the new choice.
 * @param props.placement - Open the menu above or below the trigger.
 * @param props.variant - `chip` for the chat composer, `field` for settings rows.
 * @param props.onProvider - When set, the menu can switch providers too; the
 *   model and effort reset because ids don't carry across.
 */
export function ModelPicker({
  provider,
  value,
  inherited,
  onChange,
  placement = "down",
  variant = "field",
  onProvider,
}: {
  provider: AiProvider;
  value: ModelChoice;
  inherited: Inherited;
  onChange: (choice: ModelChoice) => void;
  placement?: "up" | "down";
  variant?: "chip" | "field";
  onProvider?: (p: AiProvider) => void;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const { catalog, loading, refresh } = useModelCatalog();
  const models = catalog[provider];
  const panel = useRef<HTMLDivElement>(null);
  const dialogId = useId();
  const onRootKey = useDismiss(open, () => setOpen(false), [root]);

  useEffect(() => {
    if (open) panel.current?.focus({ preventScroll: true });
  }, [open]);

  const effectiveModel = value.model || inherited.model;
  const effectiveEffort = value.effort || inherited.effort;
  const providerName = provider === "claude" ? "Claude Code" : "Codex";
  const efforts = effortsFor(provider, effectiveModel, catalog);

  return (
    <div ref={root} className="relative" onKeyDown={onRootKey}>
      <button
        type="button"
        onPointerDown={() => void focusWindow()}
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? dialogId : undefined}
        title="Model and effort"
        className={
          variant === "chip"
            ? `inline-flex h-7 items-center gap-1.5 rounded-lg px-2 text-[12px] ring-1 transition-colors ${
                open ? "bg-white/10 text-white ring-white/20" : "text-white/80 ring-white/[0.09] hover:bg-white/6 hover:text-white"
              }`
            : `inline-flex h-7 min-w-44 items-center justify-between gap-2 rounded-lg bg-white/7 px-2.5 text-[12px] text-white/85 hover:bg-white/10 ${
                open ? "ring-1 ring-accent/60" : ""
              }`
        }
      >
        <span className="truncate">
          {onProvider && <span className="mr-1 text-white/50">{provider === "claude" ? "Claude" : "Codex"}</span>}
          {effectiveModel ? modelLabel(effectiveModel, models) : "Default"}
          {effectiveEffort && <span className="ml-1 text-white/45">{effortLabel(effectiveEffort)}</span>}
        </span>
        <ChevronDown12Regular className={`size-3 shrink-0 text-white/40 transition-transform ${open ? "rotate-180" : ""}`} />
      </button>

      <AnimatePresence>
        {open && (
          <motion.div
            ref={panel}
            id={dialogId}
            role="dialog"
            aria-label="Model and effort"
            tabIndex={-1}
            initial={{ opacity: 0, y: placement === "up" ? 6 : -6, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: placement === "up" ? 6 : -6, scale: 0.98 }}
            transition={{ duration: 0.14, ease: [0.2, 0.8, 0.2, 1] }}
            className={`absolute z-40 flex w-80 flex-col overflow-hidden rounded-xl surface-menu outline-none shadow-[0_16px_48px] shadow-shade/65 ring-1 ring-white/[0.09] ${
              placement === "up" ? "bottom-full mb-2 origin-bottom" : "top-full mt-2 origin-top"
            } ${variant === "chip" ? "left-0" : "right-0"}`}
          >
            {onProvider && (
              <div className="px-2 pt-2">
                <Segmented
                  fill
                  label="AI"
                  options={[
                    { value: "claude", label: "Claude Code" },
                    { value: "codex", label: "Codex" },
                  ]}
                  value={provider}
                  onChange={onProvider}
                />
              </div>
            )}
            <div className="flex items-center justify-between px-3.5 pt-2.5 pb-1">
              <span className="text-[11.5px] text-white/40">Select a model</span>
              <button
                type="button"
                onClick={refresh}
                title={`Ask ${providerName} for its models again`}
                aria-label="Refresh models"
                className="grid size-5 place-items-center rounded text-white/40 hover:bg-white/8 hover:text-white/80"
              >
                <ArrowClockwise12Regular className={`size-3 ${loading ? "animate-spin" : ""}`} />
              </button>
            </div>

            <div
              role="radiogroup"
              aria-label="Model"
              className="scroll-fade flex max-h-[min(300px,45vh)] flex-col overflow-y-auto overscroll-contain px-1.5 pb-1.5"
            >
              <Option
                selected={!value.model}
                title="Default (recommended)"
                detail={
                  inherited.model
                    ? `${modelLabel(inherited.model, models)} · ${SOURCE[inherited.modelFrom]}`
                    : provider === "claude" && catalog.claudeDefault
                      ? catalog.claudeDefault
                      : `Whatever ${providerName} picks for your plan`
                }
                onClick={() => onChange({ ...value, model: "" })}
              />
              {models.map((m) => (
                <Option
                  key={m.id}
                  selected={value.model === m.id}
                  title={m.label}
                  detail={m.description}
                  onClick={() =>
                    onChange({
                      model: m.id,
                      // An effort the new model can't do would fail the run.
                      effort: value.effort && !m.efforts.includes(value.effort) ? "" : value.effort,
                    })
                  }
                />
              ))}
              {value.model && !models.some((m) => m.id === value.model) && (
                <Option selected title={value.model} detail="Custom model id" onClick={() => {}} />
              )}
              {models.length === 0 && (
                <p className="px-2.5 py-2 text-[11.5px] leading-snug text-white/45">
                  {loading
                    ? `Asking ${providerName} for its models…`
                    : ((provider === "claude" ? catalog.claudeError : catalog.codexError) ?? `${providerName} listed no models.`)}
                </p>
              )}
            </div>

            <div className="flex items-center justify-between gap-3 border-t border-white/[0.07] px-3.5 py-2.5">
              <span className="text-[12.5px] text-white/85">
                Effort{" "}
                <span className="text-white/40">
                  (
                  {value.effort
                    ? effortLabel(value.effort)
                    : inherited.effort
                      ? `${effortLabel(inherited.effort)}, ${SOURCE[inherited.effortFrom]}`
                      : "Auto"}
                  )
                </span>
              </span>
              {efforts.length > 0 ? (
                <EffortSlider levels={efforts} value={value.effort} onChange={(effort) => onChange({ ...value, effort })} />
              ) : (
                <span className="text-[11.5px] text-white/35">Not supported by this model</span>
              )}
            </div>

            <CustomModel onSubmit={(model) => onChange({ ...value, model })} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

/**
 * A row of dots with a sliding thumb. Clicking a dot picks that level;
 * clicking the picked dot again returns to Auto (inherit). Arrow keys step.
 */
function EffortSlider({ levels, value, onChange }: { levels: string[]; value: string; onChange: (v: string) => void }) {
  const index = levels.indexOf(value);
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      e.preventDefault();
      const next = Math.min(levels.length - 1, Math.max(0, (index < 0 ? -1 : index) + (e.key === "ArrowRight" ? 1 : -1)));
      onChange(levels[next] ?? "");
    }
    if (e.key === "Backspace" || e.key === "Delete") onChange("");
  };
  return (
    <div
      role="slider"
      tabIndex={0}
      aria-label="Reasoning effort"
      aria-valuemin={0}
      aria-valuemax={levels.length - 1}
      aria-valuenow={index < 0 ? undefined : index}
      aria-valuetext={value ? effortLabel(value) : "Auto"}
      onKeyDown={onKey}
      className="relative flex h-6 shrink-0 items-center rounded-full bg-white/[0.07] px-1 outline-none ring-white/30 focus-visible:ring-1"
    >
      {levels.map((l, i) => (
        <button
          key={l}
          type="button"
          tabIndex={-1}
          title={effortLabel(l)}
          onClick={() => onChange(l === value ? "" : l)}
          className="group relative grid size-5 place-items-center"
        >
          <span className={`size-1 rounded-full transition-colors ${i <= index ? "bg-white/70" : "bg-white/30"} group-hover:bg-white/80`} />
          {i === index && (
            <motion.span
              layoutId="effort-thumb"
              transition={{ type: "spring", stiffness: 520, damping: 34 }}
              className="absolute size-3.5 rounded-full bg-knob shadow-[0_1px_4px] shadow-shade/50"
            />
          )}
        </button>
      ))}
    </div>
  );
}

function Option({ selected, title, detail, onClick }: { selected: boolean; title: string; detail: string; onClick: () => void }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={onClick}
      className="flex items-center gap-3 rounded-lg px-2.5 py-[7px] text-left transition-colors hover:bg-white/[0.07]"
    >
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] text-white/92">{title}</span>
        {detail && <span className="block truncate text-[11.5px] text-white/40">{detail}</span>}
      </span>
      {selected && <Checkmark16Regular className="size-4 shrink-0 text-white/75" />}
    </button>
  );
}

function CustomModel({ onSubmit }: { onSubmit: (model: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState("");
  const focus = useKeyboardFocus();
  const valid = /^[A-Za-z0-9._:/[\]-]{1,80}$/.test(text) && !text.startsWith("-");
  const submit = () => {
    if (!valid) return;
    onSubmit(text);
    setText("");
    setEditing(false);
  };

  if (!editing) {
    return (
      <button
        type="button"
        onClick={() => setEditing(true)}
        className="border-t border-white/[0.07] px-3.5 py-2 text-left text-[11.5px] text-white/40 hover:bg-white/[0.04] hover:text-white/75"
      >
        Use a custom model id…
      </button>
    );
  }
  return (
    <div className="flex items-center gap-1.5 border-t border-white/[0.07] px-2 py-1.5">
      <input
        autoFocus
        value={text}
        onChange={(e) => setText(e.target.value.trim())}
        {...focus.props}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
          if (e.key === "Escape") {
            e.stopPropagation();
            setEditing(false);
          }
        }}
        placeholder="e.g. claude-opus-5-5[1m]"
        spellCheck={false}
        className="h-7 min-w-0 flex-1 rounded-md bg-white/5 px-2 font-mono text-[11px] text-white/85 outline-none placeholder:font-sans placeholder:text-white/30"
      />
      <button
        type="button"
        disabled={!valid}
        onClick={submit}
        className="h-7 rounded-md bg-white/10 px-2.5 text-[11.5px] text-white/85 hover:bg-white/15 disabled:opacity-30"
      >
        Use
      </button>
    </div>
  );
}
