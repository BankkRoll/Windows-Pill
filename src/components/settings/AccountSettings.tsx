import { ArrowClockwise16Regular, CheckmarkCircle16Filled, Copy16Regular, ErrorCircle16Filled } from "@fluentui/react-icons";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useEffect, useState } from "react";
import { attempt } from "../../lib/actions";
import { api, type Check } from "../../lib/native";
import { usePillStore } from "../../store";
import { Group, PageHeader } from "./kit";

interface Tool {
  title: string;
  /** Check names from `cli_status` that belong to this tool. */
  prefix: string;
  purpose: string;
  install: string;
  login: string;
}

const TOOLS: readonly Tool[] = [
  {
    title: "Git",
    prefix: "Git",
    purpose: "Worktrees, branches and commits for every agent run",
    install: "winget install --id Git.Git",
    login: "git --version",
  },
  {
    title: "Claude Code",
    prefix: "Claude",
    purpose: "Agents and chats in projects set to Claude Code",
    install: "npm install -g @anthropic-ai/claude-code",
    login: "claude",
  },
  {
    title: "Codex",
    prefix: "Codex",
    purpose: "Agents and chats in projects set to Codex",
    install: "npm install -g @openai/codex",
    login: "codex login",
  },
  {
    title: "GitHub CLI",
    prefix: "GitHub",
    purpose: "Pushing agent branches and opening draft PRs",
    install: "winget install --id GitHub.cli",
    login: "gh auth login",
  },
];

/**
 * Install and login status of every CLI Pill drives, with the exact command
 * that fixes each problem. Pill never stores credentials of its own.
 */
export function AccountSettings() {
  const [checks, setChecks] = useState<Check[] | null>(null);
  const projects = usePillStore((s) => s.snapshot.projects);
  const load = () => {
    setChecks(null);
    void api.cliStatus().then(setChecks).catch(() => setChecks([]));
  };
  useEffect(load, []);

  const usage = (prefix: string) => {
    if (prefix === "GitHub" || prefix === "Git") return null;
    const provider = prefix === "Claude" ? "claude" : "codex";
    const n = projects.filter((p) => p.settings.provider === provider).length;
    return `${n} project${n === 1 ? "" : "s"}`;
  };

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Accounts"
        subtitle="Pill uses each CLI's own login. No keys are stored here."
        action={
          <button
            type="button"
            onClick={load}
            disabled={!checks}
            className="inline-flex h-7 items-center gap-1.5 rounded-lg px-2 text-[12px] text-white/60 hover:bg-white/8 hover:text-white/90 disabled:opacity-60"
          >
            <ArrowClockwise16Regular className={`size-3.5 ${checks ? "" : "animate-spin"}`} />
            {checks ? "Check again" : "Checking…"}
          </button>
        }
      />

      {TOOLS.map((tool) => {
        // "Git" must not pick up "GitHub CLI": match the name or the name plus a word.
        const own = (checks ?? []).filter((c) => c.name === tool.prefix || c.name.startsWith(`${tool.prefix} `));
        const installed = own[0]?.ok ?? false;
        const ok = own.length > 0 && own.every((c) => c.ok);
        const fix = !checks || ok ? null : installed ? tool.login : tool.install;
        return (
          <Group key={tool.title} title={tool.title} hint={tool.purpose} action={<Usage text={usage(tool.prefix)} />}>
            {!checks && <p className="px-3 py-2.5 text-[12px] text-white/35">Checking…</p>}
            {own.map((c, i) => (
              <div key={c.name} className="flex items-start gap-2.5 px-3 py-2">
                {c.ok ? (
                  <CheckmarkCircle16Filled className="mt-px size-4 shrink-0 text-status-green" />
                ) : (
                  <ErrorCircle16Filled className="mt-px size-4 shrink-0 text-status-red" />
                )}
                <span className="w-24 shrink-0 text-[12.5px] text-white/85">{own.length === 1 ? "Status" : i === 0 ? "CLI" : "Login"}</span>
                <span className={`min-w-0 text-[12px] select-text ${c.ok ? "text-white/50" : "text-status-red/90"}`}>{c.message}</span>
              </div>
            ))}
            {fix && <FixCommand command={fix} />}
          </Group>
        );
      })}
    </div>
  );
}

function Usage({ text }: { text: string | null }) {
  if (!text) return null;
  return <span className="pb-px text-[11px] text-white/35 tabular-nums">{text}</span>;
}

function FixCommand({ command }: { command: string }) {
  const showToast = usePillStore((s) => s.showToast);
  return (
    <div className="flex items-center gap-2 px-3 py-2">
      <span className="text-[11.5px] text-white/45">Run in a terminal</span>
      <code className="min-w-0 flex-1 truncate rounded-md bg-well/40 px-2 py-1 font-mono text-[11.5px] text-white/80 select-text">{command}</code>
      <button
        type="button"
        aria-label="Copy command"
        title="Copy"
        onClick={() =>
          void attempt(() => writeText(command).then(() => true)).then((ok) => ok && showToast({ message: "Command copied", tone: "info" }))
        }
        className="grid size-7 place-items-center rounded-md text-white/50 hover:bg-white/8 hover:text-white/90"
      >
        <Copy16Regular className="size-4" />
      </button>
    </div>
  );
}
