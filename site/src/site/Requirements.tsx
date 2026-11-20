/**
 * What Pill needs on the machine, shared by the landing page and the docs.
 */

import type { ReactNode } from "react";

/** One requirement: what, whether it's needed, and how to get it. */
interface Requirement {
  name: string;
  need: "Required" | "For agents" | "Optional";
  note: ReactNode;
}

const REQUIREMENTS: readonly Requirement[] = [
  { name: "Windows 11, x64", need: "Required", note: "Windows 10 works but is less tested." },
  { name: "Microsoft Edge WebView2", need: "Required", note: "Part of Windows 11. The installer fetches it if it's missing." },
  { name: "Git for Windows", need: "Required", note: <code>winget install --id Git.Git</code> },
  {
    name: "Claude Code and/or Codex CLI",
    need: "For agents",
    note: (
      <>
        Installed and signed in. Pill uses your own login. See the{" "}
        <a href="https://code.claude.com/docs" className="text-white/70 underline-offset-2 hover:text-white hover:underline">
          Claude Code docs
        </a>{" "}
        or run <code>npm install -g @openai/codex</code>, then <code>codex login</code>.
      </>
    ),
  },
  { name: "GitHub CLI", need: "Optional", note: <>For draft pull requests and issues on the board.</> },
  { name: "Node.js 20 or later", need: "Optional", note: "Only for plugins written for Node, such as CVE watch." },
];

/** The requirements as a definition list. */
export function Requirements() {
  return (
    <dl className="divide-y divide-white/[0.07] rounded-2xl bg-white/[0.025] ring-1 ring-white/[0.07]">
      {REQUIREMENTS.map((r) => (
        <div key={r.name} className="grid gap-1 px-5 py-4 sm:grid-cols-[1fr_auto] sm:gap-x-6">
          <dt className="text-[15px] font-medium text-white/90">{r.name}</dt>
          <dd className="text-[12px] font-medium text-white/40 sm:row-span-2 sm:pt-0.5 sm:text-right">{r.need}</dd>
          <dd className="text-[14px] leading-relaxed text-white/50 [&_code]:rounded-[5px] [&_code]:bg-white/7 [&_code]:px-1.5 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[12.5px] [&_code]:text-white/75">
            {r.note}
          </dd>
        </div>
      ))}
    </dl>
  );
}
