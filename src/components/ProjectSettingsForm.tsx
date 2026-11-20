/**
 * Per-project settings page, editing `.pill/project.json`:
 *
 * - Repository: live, read-only status (git or plain folder, remote, branch,
 *   GitHub CLI) so the Git choices below can explain what will work.
 * - General: name, color and editor.
 * - Git: base branch, what a finished agent run ships, branch prefix,
 *   whether the task board is committed, and (on GitHub) whether assigned
 *   issues show on the board, with the sync status.
 * - Checks: the test and lint commands Pill runs before shipping.
 * - Agents and chat: provider, model, plan mode and project rules.
 *
 * Edits stay in a draft until saved through the floating save bar.
 */
import { ArrowClockwise16Regular, Code16Regular, FolderOpen16Regular, Open12Regular, Warning16Regular } from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useKeyboardFocus } from "../hooks/useKeyboardFocus";
import { attempt, openEditor, openFolder, openLink } from "../lib/actions";
import { rebaseDraft } from "../lib/draft";
import { syncText } from "../lib/github";
import { inheritedChoice } from "../lib/models";
import {
  api,
  type AiProvider,
  type EditorChoice,
  type Project,
  type ProjectSettings,
  type RepoInfo,
  type ShipMode,
  type DetectedChecks,
  type FoundCommand,
} from "../lib/native";
import { usePillStore } from "../store";
import { Select, type SelectOption } from "./controls";
import { ModelPicker } from "./ModelPicker";
import { Group, PageHeader, Row, SettingsFooterContext, SoftButton } from "./settings/kit";
import { Segmented, Toggle } from "./controls";
import { IconButton } from "./ui";

const SWATCHES = ["#2F7BFF", "#30D158", "#FFB340", "#FF453A", "#BF5AF2", "#64D2FF", "#FF9F0A", "#FF375F", "#5E5CE6", "#AC8E68"];

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/**
 * Loads {@link RepoInfo} for a project and reloads it on demand.
 * `null` while the first load is in flight or when it failed.
 */
function useRepoInfo(projectId: string) {
  const [info, setInfo] = useState<RepoInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const load = useCallback(() => {
    let live = true;
    setLoading(true);
    api
      .projectGitInfo(projectId)
      .then((r) => live && setInfo(r))
      .catch(() => live && setInfo(null))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [projectId]);
  useEffect(() => {
    setInfo(null);
    return load();
  }, [load]);
  return { info, loading, reload: load };
}

/**
 * Editor for a project's `.pill/project.json`. Edits stay in a draft until
 * saved, and a save bar appears while there are unsaved changes.
 *
 * @param props.project - Project being edited.
 */
export function ProjectSettingsForm({ project }: { project: Project }) {
  const [draft, setDraft] = useState<ProjectSettings>(project.settings);
  const [editors, setEditors] = useState({ cursor: false, code: false });
  const [saving, setSaving] = useState(false);
  const repo = useRepoInfo(project.id);
  const showToast = usePillStore((s) => s.showToast);
  const config = usePillStore((s) => s.config);
  const dirty = JSON.stringify(draft) !== JSON.stringify(project.settings);
  const footer = useContext(SettingsFooterContext);

  useEffect(() => {
    if (!dirty) return;
    // The save bar slides in over the bottom of the pane; keep the field being edited above it.
    const t = window.setTimeout(() => {
      const el = document.activeElement;
      if (el instanceof HTMLElement && el !== document.body) el.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }, 160);
    return () => window.clearTimeout(t);
  }, [dirty]);

  // Every snapshot brings a new settings object; only real changes made
  // elsewhere may reach the draft, and never over unsaved edits.
  const adopted = useRef(project.settings);
  useEffect(() => {
    const prev = adopted.current;
    adopted.current = project.settings;
    setDraft((d) => rebaseDraft(d, prev, project.settings));
  }, [project.settings]);
  const [detected, setDetected] = useState<DetectedChecks | null>(null);
  useEffect(() => {
    void api.detectChecks(project.id).then(setDetected, () => setDetected(null));
  }, [project.id]);
  useEffect(() => {
    void api.detectEditors().then(setEditors);
  }, []);

  const set = <K extends keyof ProjectSettings>(key: K, value: ProjectSettings[K]) => setDraft((d) => ({ ...d, [key]: value }));

  const save = async () => {
    setSaving(true);
    const ok = await attempt(() => api.saveProjectSettings(project.id, draft).then(() => true));
    setSaving(false);
    if (ok)
      showToast({
        message: `Saved ${draft.name || project.name}`,
        tone: "info",
      });
  };

  const autoEditor = editors.cursor ? "Cursor" : editors.code ? "VS Code" : "Explorer";
  const color = draft.color || project.color;
  const openTasks = project.tasks.filter((t) => t.status !== "done").length;
  const info = repo.info;
  const plainFolder = info !== null && !info.isRepo;

  return (
    <div className="relative flex flex-col gap-4 pb-14">
      <PageHeader
        title={draft.name || project.name}
        subtitle={plural(openTasks, "open task")}
        action={
          <div className="flex gap-1">
            <SoftButton icon={<FolderOpen16Regular className="size-4" />} onClick={() => void openFolder(project)}>
              Folder
            </SoftButton>
            <SoftButton icon={<Code16Regular className="size-4" />} onClick={() => void openEditor(project)}>
              Editor
            </SoftButton>
          </div>
        }
      />

      {project.errors.map((e) => (
        <p key={e} className="flex items-start gap-2 rounded-xl bg-status-red/12 px-3 py-2 text-[12px] text-status-red">
          <Warning16Regular className="mt-px size-4 shrink-0" />
          {e}
        </p>
      ))}

      <RepositoryGroup path={project.path} info={info} loading={repo.loading} onRefresh={repo.reload} />

      <Group title="General">
        <Row label="Name">
          <TextInput value={draft.name} placeholder={project.path.split("\\").pop()} onChange={(v) => set("name", v)} />
        </Row>
        <Row label="Color" hint="Dot on tasks, cards and the project picker">
          <div className="flex items-center gap-1.5" role="radiogroup" aria-label="Project color">
            {SWATCHES.map((c) => {
              const on = color.toUpperCase() === c;
              return (
                <button
                  key={c}
                  type="button"
                  role="radio"
                  aria-checked={on}
                  aria-label={c}
                  onClick={() => set("color", c)}
                  className={`size-3.5 rounded-full outline-offset-2 transition-transform hover:scale-110 ${on ? "outline-2 outline-white/80" : ""}`}
                  style={{ background: c }}
                />
              );
            })}
          </div>
        </Row>
        <Row label="Open in" hint="Where the Editor button and “Open in editor” go">
          <Segmented<EditorChoice>
            label="Editor"
            options={[
              { value: "auto", label: `Auto · ${autoEditor}` },
              ...(editors.cursor ? [{ value: "cursor" as const, label: "Cursor" }] : []),
              ...(editors.code ? [{ value: "code" as const, label: "VS Code" }] : []),
              { value: "explorer", label: "Explorer" },
            ]}
            value={draft.editor}
            onChange={(v) => set("editor", v)}
          />
        </Row>
      </Group>

      {!plainFolder && <GitGroup projectId={project.id} draft={draft} saved={project.settings} info={info} set={set} />}

      <Group title="Checks" hint="Pill runs these in the agent's worktree before shipping its work">
        <Row
          label="Test command"
          hint={
            draft.testCommand.trim() ? (
              "Must pass, or the run fails"
            ) : detected?.test ? (
              <Suggest found={detected.test} onUse={(c) => set("testCommand", c)} />
            ) : (
              "None found. Agents set one up, or ask from a task, and their changes aren't checked until then."
            )
          }
        >
          <TextInput value={draft.testCommand} placeholder="pnpm test" onChange={(v) => set("testCommand", v)} mono />
        </Row>
        <Row
          label="Lint command"
          hint={
            !draft.lintCommand.trim() && detected?.lint ? (
              <Suggest found={detected.lint} onUse={(c) => set("lintCommand", c)} />
            ) : (
              "Optional. Runs after the tests and must pass too."
            )
          }
        >
          <TextInput value={draft.lintCommand} placeholder="pnpm lint" onChange={(v) => set("lintCommand", v)} mono />
        </Row>
      </Group>

      <Group title="Agents and chat">
        <Row label="AI" hint="Uses that CLI's own login; see Accounts">
          <Segmented<AiProvider>
            label="AI"
            options={[
              { value: "claude", label: "Claude Code" },
              { value: "codex", label: "Codex" },
            ]}
            value={draft.provider}
            // Model ids don't carry across providers.
            onChange={(v) =>
              setDraft((d) => ({
                ...d,
                provider: v,
                model: v === d.provider ? d.model : "",
                effort: v === d.provider ? d.effort : "",
              }))
            }
          />
        </Row>
        <Row label="Model" hint="Chats can still switch per conversation">
          <ModelPicker
            provider={draft.provider}
            value={{ model: draft.model, effort: draft.effort }}
            inherited={inheritedChoice(config, null, draft.provider)}
            onChange={(c) => setDraft((d) => ({ ...d, model: c.model, effort: c.effort }))}
          />
        </Row>
        <Row label="Plan first" hint="Agents propose a plan and wait for your approval">
          <Toggle checked={draft.planMode} onChange={(v) => set("planMode", v)} />
        </Row>
        <Block label="Rules" hint="Added to every agent prompt and chat in this project">
          <TextArea
            value={draft.agentRules}
            placeholder="e.g. Use PixiJS 8 APIs only. Never touch /math. Keep components under 200 lines."
            onChange={(v) => set("agentRules", v)}
          />
        </Block>
      </Group>

      {footer &&
        createPortal(
          <AnimatePresence>
            {dirty && (
              <motion.div
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: 8 }}
                transition={{ duration: 0.14 }}
                className="surface-menu flex items-center gap-2 rounded-xl px-3 py-2 shadow-[0_8px_24px] shadow-shade/50 ring-1 ring-white/10"
              >
                <span className="size-1.5 rounded-full bg-status-amber" />
                <span className="flex-1 text-[12px] text-white/70">Unsaved changes</span>
                <button
                  type="button"
                  disabled={saving}
                  onClick={() => setDraft(project.settings)}
                  className="h-7 rounded-lg px-3 text-[12px] text-white/60 hover:bg-white/8 disabled:opacity-30"
                >
                  Discard
                </button>
                <button
                  type="button"
                  disabled={saving}
                  onClick={() => void save()}
                  className="h-7 rounded-lg bg-accent px-3 text-[12px] font-semibold text-on-accent disabled:opacity-40"
                >
                  {saving ? "Saving…" : "Save"}
                </button>
              </motion.div>
            )}
          </AnimatePresence>,
          footer,
        )}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Repository status
// -----------------------------------------------------------------------------

type Tone = "green" | "amber" | "red" | "neutral";

const DOT: Record<Tone, string> = {
  green: "bg-status-green",
  amber: "bg-status-amber",
  red: "bg-status-red",
  neutral: "bg-white/35",
};

function Status({ tone, children }: { tone: Tone; children: ReactNode }) {
  return (
    <span className="inline-flex items-center gap-1.5 text-[12px] text-white/80">
      <span className={`size-1.5 shrink-0 rounded-full ${DOT[tone]}`} />
      {children}
    </span>
  );
}

/** Sentence describing the checked-out branch's state, e.g. "2 changes · 1 ahead of origin/main". */
function branchSummary(info: RepoInfo): string {
  if (info.unborn) return "No commits yet. Agents need a first commit to branch from.";
  const parts = [info.changes ? plural(info.changes, "uncommitted change") : "Clean"];
  if (!info.upstream) parts.push(info.remote ? "not tracking a remote branch" : "");
  else if (info.ahead || info.behind) {
    const ab = [info.ahead && `${info.ahead} ahead`, info.behind && `${info.behind} behind`].filter(Boolean).join(", ");
    parts.push(`${ab} ${info.upstream}`);
  } else parts.push(`up to date with ${info.upstream}`);
  return parts.filter(Boolean).join(" · ");
}

function RepositoryGroup({
  path,
  info,
  loading,
  onRefresh,
}: {
  path: string;
  info: RepoInfo | null;
  loading: boolean;
  onRefresh: () => void;
}) {
  return (
    <Group
      title="Repository"
      hint={path}
      action={
        <IconButton
          icon={ArrowClockwise16Regular}
          label="Check again"
          disabled={loading}
          onClick={onRefresh}
          className={loading ? "[&>svg]:animate-spin" : ""}
        />
      }
    >
      {!info ? (
        <Row label="Status">
          <Status tone="neutral">{loading ? "Checking…" : "Couldn't read git status"}</Status>
        </Row>
      ) : !info.isRepo ? (
        <Row
          label="Plain folder"
          hint="Not a git repository. Agents work on their own branch, so they need git: run git init and commit once."
        >
          <Status tone="neutral">No git</Status>
        </Row>
      ) : (
        <>
          <Row
            label="Remote"
            hint={
              info.remote
                ? info.remote.name === "origin"
                  ? undefined
                  : `Agents fetch from and push to “${info.remote.name}”`
                : "Local only. Agents can commit on a branch but can't push or open PRs."
            }
          >
            {info.remote ? (
              info.remote.webUrl ? (
                <button
                  type="button"
                  title={info.remote.webUrl}
                  onClick={() => openLink(info.remote!.webUrl!)}
                  className="inline-flex max-w-64 items-center gap-1 rounded-md px-1 text-[12px] text-accent hover:underline"
                >
                  <span className="truncate">{info.remote.display}</span>
                  <Open12Regular className="size-3 shrink-0" />
                </button>
              ) : (
                <span className="block max-w-64 truncate font-mono text-[11.5px] text-white/75" title={info.remote.display}>
                  {info.remote.display}
                </span>
              )
            ) : (
              <Status tone="neutral">Local only</Status>
            )}
          </Row>
          <Row label="Checked out" hint={branchSummary(info)}>
            {info.branch ? (
              <span className="block max-w-56 truncate font-mono text-[11.5px] text-white/80" title={info.branch}>
                {info.branch}
              </span>
            ) : (
              <Status tone="amber">Detached HEAD</Status>
            )}
          </Row>
          {info.remote && info.gh && (
            <Row
              label="GitHub CLI"
              hint={
                !info.gh.ok
                  ? info.gh.message
                  : info.remote.github
                    ? "Opens draft PRs for agent branches"
                    : "This remote isn't on GitHub, so gh can't open PRs for it"
              }
            >
              <Status tone={info.gh.ok ? (info.remote.github ? "green" : "neutral") : "amber"}>
                {info.gh.ok ? "Signed in" : info.gh.message.includes("isn't installed") ? "Not installed" : "Signed out"}
              </Status>
            </Row>
          )}
        </>
      )}
    </Group>
  );
}

// -----------------------------------------------------------------------------
// Git settings
// -----------------------------------------------------------------------------

interface ShipChoice {
  value: ShipMode;
  title: string;
  detail: string;
}

const SHIP_CHOICES: ShipChoice[] = [
  {
    value: "pr",
    title: "Open a draft PR",
    detail: "Push the branch and open a draft pull request with gh",
  },
  {
    value: "push",
    title: "Push the branch",
    detail: "Push it to the remote and open the PR yourself, if you want one",
  },
  {
    value: "local",
    title: "Keep it local",
    detail: "Commit on the agent branch; nothing leaves this computer",
  },
];

/** Why a ship mode can't work with this repository, or `null` when it can. */
function shipBlocker(mode: ShipMode, info: RepoInfo | null): string | null {
  if (!info || mode === "local") return null;
  if (!info.remote) return "Needs a remote; this repository is local only";
  if (mode === "push") return null;
  if (!info.remote.github) return "gh opens PRs on GitHub only";
  if (info.gh && !info.gh.ok) return info.gh.message.includes("isn't installed") ? "Needs the GitHub CLI (gh)" : "Needs gh to be signed in";
  return null;
}

function GitGroup({
  projectId,
  draft,
  saved,
  info,
  set,
}: {
  projectId: string;
  draft: ProjectSettings;
  saved: ProjectSettings;
  info: RepoInfo | null;
  set: <K extends keyof ProjectSettings>(key: K, value: ProjectSettings[K]) => void;
}) {
  const auto = info?.defaultBranch ?? "main";
  const local = info?.localBranches ?? [];
  const remote = info?.remoteBranches ?? [];
  const hasRemote = Boolean(info?.remote);
  const remoteOnly = remote.filter((b) => !local.includes(b));
  const known = new Set([...local, ...remote]);
  const base = draft.baseBranch || auto;

  const branchOptions: SelectOption<string>[] = [
    { value: "", label: `Auto · ${auto}` },
    ...local.map((b) => ({
      value: b,
      label: b,
      detail: hasRemote && !remote.includes(b) ? "local only" : undefined,
    })),
    ...remoteOnly.map((b) => ({ value: b, label: b, detail: "on remote" })),
  ];
  if (draft.baseBranch && !known.has(draft.baseBranch))
    branchOptions.push({
      value: draft.baseBranch,
      label: draft.baseBranch,
      detail: "not found",
    });

  let baseWarning: string | null = null;
  if (info?.isRepo && !info.unborn) {
    if (!known.has(base)) baseWarning = `No branch named ${base} here`;
    else if (hasRemote && !remote.includes(base))
      baseWarning = `${base} isn't on ${info.remote?.name ?? "the remote"}; agents start from your local ${base}`;
  }

  // Mirrors agent::ship_mode: without a remote, work stays local.
  const localOnly = !!info && !info.remote;
  const effective: ShipMode = localOnly ? "local" : draft.ship;
  const blocked = shipBlocker(effective, info);
  const prefix = draft.branchPrefix.trim() || "agent/";

  return (
    <Group title="Git">
      <Row
        label="Base branch"
        hint={
          baseWarning ? (
            <span className="text-status-amber">{baseWarning}</span>
          ) : draft.ship === "pr" ? (
            "Agents branch from it, and their PRs target it"
          ) : (
            "Agents branch from it"
          )
        }
      >
        <Select<string>
          label="Base branch"
          value={draft.baseBranch}
          options={branchOptions}
          onChange={(v) => set("baseBranch", v)}
          align="end"
          className="w-56 font-mono text-[11.5px]"
        />
      </Row>

      <Block
        label="When an agent finishes"
        hint={
          localOnly
            ? "No remote yet, so work stays on the agent's local branch. This applies once you add one."
            : "After its checks pass, the work is shipped like this"
        }
      >
        <div role="radiogroup" aria-label="When an agent finishes" className="flex flex-col gap-1">
          {SHIP_CHOICES.map((c) => (
            <ShipOption
              key={c.value}
              choice={c}
              checked={effective === c.value}
              blocker={shipBlocker(c.value, info)}
              onPick={() => set("ship", c.value)}
            />
          ))}
        </div>
        {blocked && (
          <p className="flex items-start gap-1.5 text-[11px] leading-snug text-status-amber">
            <Warning16Regular className="mt-px size-3.5 shrink-0" />
            {saved.ship === effective
              ? `Agents in this project will stop before starting: ${blocked.toLowerCase()}.`
              : `This won't work yet: ${blocked.toLowerCase()}.`}
          </p>
        )}
      </Block>

      <Row
        label="Branch prefix"
        hint={
          <>
            Agent branches look like <span className="font-mono text-white/55">{prefix}t_8f2k1-fix-login</span>
          </>
        }
      >
        <TextInput value={draft.branchPrefix} placeholder="agent/" onChange={(v) => set("branchPrefix", v)} mono />
      </Row>

      <Row label="Commit tasks.json" hint="Share this task board through git instead of ignoring it">
        <Toggle checked={draft.commitTasks} onChange={(v) => set("commitTasks", v)} />
      </Row>

      {info?.remote?.github && (
        <GithubIssuesRow projectId={projectId} checked={draft.githubIssues} onChange={(v) => set("githubIssues", v)} />
      )}
    </Group>
  );
}

/**
 * "Show my GitHub issues" with the project's live sync status under it and a
 * button to sync now. PR review and CI status sync either way.
 */
function GithubIssuesRow({ projectId, checked, onChange }: { projectId: string; checked: boolean; onChange: (on: boolean) => void }) {
  const github = usePillStore((s) => s.github[projectId]);
  const status = github ? syncText(github.status, github.syncing) : "Not synced yet";
  const trouble = github && ["no_gh", "signed_out", "rate_limited", "error"].includes(github.status.kind);
  return (
    <Row label="Show my GitHub issues" hint={trouble ? <span className="text-status-amber">{status}</span> : status}>
      <div className="flex items-center gap-1">
        <IconButton
          icon={ArrowClockwise16Regular}
          label="Sync now"
          disabled={github?.syncing}
          onClick={() => void attempt(() => api.githubRefresh(projectId))}
          className={github?.syncing ? "[&>svg]:animate-spin" : ""}
        />
        <Toggle checked={checked} onChange={onChange} />
      </div>
    </Row>
  );
}

function ShipOption({
  choice,
  checked,
  blocker,
  onPick,
}: {
  choice: ShipChoice;
  checked: boolean;
  blocker: string | null;
  onPick: () => void;
}) {
  // NOTE: A blocked mode that is already saved stays focusable and shown as
  // checked, so the user sees what is set and why it fails.
  const disabled = blocker !== null && !checked;
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      aria-disabled={disabled}
      disabled={disabled}
      title={blocker ?? undefined}
      onClick={() => !checked && onPick()}
      className={`flex items-start gap-2.5 rounded-lg px-2.5 py-2 text-left transition-colors ${
        checked ? "bg-accent/12 ring-1 ring-accent/45" : "hover:bg-white/5"
      } disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-transparent`}
    >
      <span
        className={`mt-0.5 grid size-3.5 shrink-0 place-items-center rounded-full ring-1 ${
          checked ? "bg-accent ring-accent" : "ring-white/30"
        }`}
      >
        {checked && <span className="size-1.5 rounded-full bg-on-accent" />}
      </span>
      <span className="min-w-0">
        <span className="block text-[12.5px] text-white/90">{choice.title}</span>
        <span className="block text-[11px] leading-snug text-white/45">{blocker ?? choice.detail}</span>
      </span>
    </button>
  );
}

// -----------------------------------------------------------------------------
// Inputs
// -----------------------------------------------------------------------------

/** A full-width row: label and hint above, the control below. */
function Block({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5 px-3 py-2">
      <span>
        <span className="block text-[12.5px] text-white/85">{label}</span>
        {hint && <span className="block text-[11px] text-white/40">{hint}</span>}
      </span>
      {children}
    </div>
  );
}

function TextInput({
  value,
  placeholder,
  onChange,
  mono,
}: {
  value: string;
  placeholder?: string;
  onChange: (v: string) => void;
  mono?: boolean;
}) {
  const focus = useKeyboardFocus();
  return (
    <input
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      {...focus.props}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === "Escape") focus.release(e.currentTarget);
      }}
      spellCheck={false}
      className={`h-7 w-56 rounded-lg bg-white/7 px-2 text-[12px] text-white/85 outline-none placeholder:text-white/30 focus:bg-white/10 focus:ring-1 focus:ring-accent/60 ${
        mono ? "font-mono text-[11.5px]" : ""
      }`}
    />
  );
}

function TextArea({ value, placeholder, onChange }: { value: string; placeholder: string; onChange: (v: string) => void }) {
  const focus = useKeyboardFocus();
  return (
    <textarea
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      {...focus.props}
      onKeyDown={(e) => {
        if (e.key === "Escape") focus.release(e.currentTarget);
      }}
      rows={3}
      className="w-full resize-none rounded-lg bg-white/7 px-2 py-1.5 text-[12px] leading-relaxed text-white/85 outline-none placeholder:text-white/30 focus:bg-white/10 focus:ring-1 focus:ring-accent/60"
    />
  );
}

/** "Found `pnpm test` in package.json · Use" under an empty command field. */
function Suggest({ found, onUse }: { found: FoundCommand; onUse: (command: string) => void }) {
  return (
    <span>
      Found <code className="text-white/80">{found.command}</code> in {found.source} ·{" "}
      <button type="button" onClick={() => onUse(found.command)} className="text-accent hover:underline">
        Use
      </button>
    </span>
  );
}
