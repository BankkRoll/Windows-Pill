/**
 * The docs: one page with a section per topic, so every section has a
 * stable `#anchor` that GitHub Pages serves without a router.
 *
 * The material follows the repository's README.md, docs/plugins.md,
 * plugins/README.md and plugins/cve-watch/README.md; when those change,
 * this page should follow.
 */

import { ChevronDown12Regular } from "@fluentui/react-icons";
import { useEffect, useState, type ReactNode } from "react";
import { RELEASES_URL, REPO_URL } from "../lib/release";
import { repoLink, SiteFooter, SiteHeader } from "./Chrome";
import { DownloadButton } from "./Download";
import { Requirements } from "./Requirements";

const SECTIONS = [
  { id: "getting-started", title: "Getting started", body: GettingStarted },
  { id: "projects", title: "Projects & tasks", body: Projects },
  { id: "agents", title: "Agents", body: Agents },
  { id: "chat", title: "Chat", body: Chat },
  { id: "today", title: "Today", body: Today },
  { id: "plugins", title: "Plugins", body: Plugins },
  { id: "cve-watch", title: "CVE watch", body: CveWatch },
  { id: "updates", title: "Updates", body: Updates },
  { id: "faq", title: "FAQ", body: Faq },
] as const;

/** The docs page. */
export function Docs() {
  const active = useActiveSection();
  return (
    <>
      <a
        href="#docs-content"
        className="sr-only z-50 rounded-lg bg-accent px-3 py-2 text-on-accent focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
      >
        Skip to content
      </a>
      <SiteHeader current="docs" />
      <div className="mx-auto max-w-6xl px-4 sm:px-6 lg:grid lg:grid-cols-[200px_minmax(0,1fr)] lg:gap-16">
        <aside className="max-lg:contents lg:sticky lg:top-14 lg:h-[calc(100vh-3.5rem)] lg:overflow-y-auto lg:py-12">
          <MobileNav active={active} />
          <nav aria-label="Docs" className="hidden lg:block">
            <p className="mb-3 text-[12px] font-medium tracking-wide text-white/35 uppercase">Docs</p>
            <ul className="flex flex-col gap-0.5 text-[14px]">
              {SECTIONS.map((s) => (
                <li key={s.id}>
                  <a
                    href={`#${s.id}`}
                    aria-current={active === s.id ? "location" : undefined}
                    className={`block rounded-md px-2.5 py-1.5 transition-colors ${
                      active === s.id ? "bg-white/[0.07] text-white" : "text-white/55 hover:text-white/90"
                    }`}
                  >
                    {s.title}
                  </a>
                </li>
              ))}
            </ul>
          </nav>
        </aside>
        <main id="docs-content" className="min-w-0 pb-24 lg:pt-12">
          <h1 className="font-display text-[36px] font-semibold tracking-[-0.015em] text-white">Pill docs</h1>
          <p className="mt-3 max-w-2xl text-[17px] leading-relaxed text-white/60">
            How to install Pill, add your projects, run agents and write plugins. Source and issues are{" "}
            <a href={REPO_URL} className="text-accent hover:underline">
              on GitHub
            </a>
            .
          </p>
          {SECTIONS.map(({ id, title, body: Body }) => (
            <section
              key={id}
              id={id}
              aria-labelledby={`${id}-title`}
              className="prose mt-16 max-w-[720px] border-t border-white/[0.07] pt-10"
            >
              <h2 id={`${id}-title`}>{title}</h2>
              <Body />
            </section>
          ))}
        </main>
      </div>
      <SiteFooter />
    </>
  );
}

/**
 * The section being read: the last one whose heading has scrolled past the
 * sticky header. Also scrolls to the URL's #section once the page has
 * rendered, which the browser tried before the content existed.
 */
function useActiveSection(): string {
  const [active, setActive] = useState<string>(SECTIONS[0].id);
  useEffect(() => {
    const hash = decodeURIComponent(window.location.hash.slice(1));
    if (hash) document.getElementById(hash)?.scrollIntoView();
    let frame = 0;
    const update = () => {
      frame = 0;
      let current: string = SECTIONS[0].id;
      for (const s of SECTIONS) {
        const el = document.getElementById(s.id);
        if (el && el.getBoundingClientRect().top <= 140) current = s.id;
      }
      setActive(current);
    };
    const onScroll = () => {
      frame ||= requestAnimationFrame(update);
    };
    update();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      window.removeEventListener("scroll", onScroll);
      cancelAnimationFrame(frame);
    };
  }, []);
  return active;
}

/** On narrow screens the section list folds into one button under the header. */
function MobileNav({ active }: { active: string }) {
  const [open, setOpen] = useState(false);
  const current = SECTIONS.find((s) => s.id === active) ?? SECTIONS[0];
  return (
    <nav
      aria-label="Docs sections"
      className="sticky top-14 z-30 -mx-4 border-b border-white/[0.06] bg-page/90 px-4 backdrop-blur-xl lg:hidden"
    >
      <button
        type="button"
        aria-expanded={open}
        aria-controls="docs-mobile-sections"
        onClick={() => setOpen((v) => !v)}
        className="flex h-11 w-full items-center gap-2 text-left text-[14px] text-white/80"
      >
        <span className="text-white/40">On this page:</span>
        <span className="truncate font-medium">{current.title}</span>
        <ChevronDown12Regular className={`ml-auto size-3 text-white/50 transition-transform ${open ? "rotate-180" : ""}`} />
      </button>
      {open && (
        <ul id="docs-mobile-sections" className="flex flex-col pb-3 text-[14px]">
          {SECTIONS.map((s) => (
            <li key={s.id}>
              <a
                href={`#${s.id}`}
                onClick={() => setOpen(false)}
                className={`block rounded-md px-2 py-2 ${s.id === active ? "text-white" : "text-white/60"}`}
              >
                {s.title}
              </a>
            </li>
          ))}
        </ul>
      )}
    </nav>
  );
}

/** A code block with an optional file name above it. */
function Code({ file, children }: { file?: string; children: string }) {
  return (
    <figure className="my-4">
      {file && <figcaption className="mb-1.5 font-mono text-[12px] text-white/40">{file}</figcaption>}
      <pre className="!my-0">
        <code>{children.trim()}</code>
      </pre>
    </figure>
  );
}

function Ext({ href, children }: { href: string; children: ReactNode }) {
  return <a href={href}>{children}</a>;
}

// -----------------------------------------------------------------------------
// Sections
// -----------------------------------------------------------------------------

function GettingStarted() {
  return (
    <>
      <h3>Install</h3>
      <p>
        Download the latest <code>Pill_x.y.z_x64-setup.exe</code> from <Ext href={RELEASES_URL}>GitHub Releases</Ext> and run it. It
        installs for your user only, with no administrator prompt, and adds Pill to the Start menu.
      </p>
      <div className="not-prose my-6">
        <DownloadButton showMeta />
      </div>
      <p>
        <strong>SmartScreen.</strong> The installer isn't Authenticode-signed yet, so Windows SmartScreen may warn the first time you run
        it. Choose <strong>More info</strong>, then <strong>Run anyway</strong>. Updates are a different matter: Pill only installs updates
        signed with the project's key (see <a href="#updates">Updates</a>).
      </p>

      <h3>Requirements</h3>
      <div className="not-prose my-4">
        <Requirements />
      </div>
      <p>
        <strong>Settings › Accounts</strong> shows what Pill can find and the command for anything missing. Pill picks up newly installed
        tools without a restart.
      </p>

      <h3>First run</h3>
      <p>
        Pill opens Settings the first time. After that the notch sits at the top centre of your primary display. Hover it, or click it if
        you prefer, and it opens; move away and it closes. <strong>Ctrl+Shift+Space</strong> opens it from any app with the command bar
        focused, and <strong>Esc</strong> returns focus to where you were.
      </p>
      <p>
        <strong>Settings › General</strong> picks another display, the hover delays, hover or click to open, whether it stays on top, launch
        at startup, and the theme: Glass, Graphite or Black, in light or dark.
      </p>

      <h3>Add your project folders</h3>
      <p>
        In <strong>Settings › Projects</strong>, add one or more folders, or whole drives, as <strong>roots</strong>. Every git repository
        inside a root, up to four levels deep, becomes a project; dependency and build folders are skipped. Under{" "}
        <strong>Single repos</strong> you can add a repository without scanning its parent folder.
      </p>
      <p>
        New repositories inside a root are picked up on their own. Each project gets a colour and a <code>.pill/</code> folder for its tasks
        and notes.
      </p>
    </>
  );
}

function Projects() {
  return (
    <>
      <p>
        Every git repository Pill finds is a project. Tasks live in that repository's <code>.pill/</code> folder, and there is one global
        Inbox for everything else. Edits made outside Pill, by an editor, an agent or a <code>git pull</code>, show up live.
      </p>

      <h3>Adding tasks</h3>
      <p>
        Type into the command bar at the bottom of the panel, or press <strong>Ctrl+Shift+Space</strong> from anywhere. Plain text creates a
        task, and inline tokens become chips as you type:
      </p>
      <table>
        <thead>
          <tr>
            <th>Token</th>
            <th>Meaning</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>
              <code>#api</code>
            </td>
            <td>Project, matched loosely</td>
          </tr>
          <tr>
            <td>
              <code>!1</code> to <code>!3</code>
            </td>
            <td>Priority</td>
          </tr>
          <tr>
            <td>
              <code>@today</code>, <code>@fri</code>, <code>@2026-11-21</code>, <code>@+3d</code>
            </td>
            <td>Due date</td>
          </tr>
          <tr>
            <td>
              <code>+ci</code>
            </td>
            <td>Tag</td>
          </tr>
        </tbody>
      </table>
      <Code>{"Fix flaky login test #api !1 @fri +ci"}</Code>
      <p>
        <code>/</code> lists commands: go to a tab, open a project in your editor or Explorer, start or compare agents, start a chat, rescan
        and more. <code>?</code> asks Claude Code or Codex about your projects; anything it proposes, such as creating tasks or starting
        agents, is shown for you to approve or deny first. Dictation adds a task by voice, offline, with the Windows desktop speech
        recognizer.
      </p>

      <h3>Board</h3>
      <p>
        The <strong>Board</strong> tab has five columns, Inbox, To do, Doing, Review and Done, for one project or all of them, with drag and
        drop. A task's page holds its title, notes, priority, tags, due date, the agent-ready flag, note snippets pinned for agents, and
        every run on the task as a conversation.
      </p>
      <p>
        With the GitHub CLI signed in, GitHub issues assigned to you show up as cards, and pull request review and CI status are synced
        back.
      </p>

      <h3>Files</h3>
      <table>
        <thead>
          <tr>
            <th>File</th>
            <th>Contents</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>
              <code>.pill/project.json</code>
            </td>
            <td>
              Name, colour, base branch, test and lint commands, editor, agent rules, provider, model, ship mode. Meant to be committed.
            </td>
          </tr>
          <tr>
            <td>
              <code>.pill/notes.md</code>
            </td>
            <td>Project notes. Meant to be committed.</td>
          </tr>
          <tr>
            <td>
              <code>.pill/tasks.json</code>
            </td>
            <td>The project's tasks. Git-ignored unless you turn on "commit tasks" for the project.</td>
          </tr>
          <tr>
            <td>
              <code>.pill/runs/</code>, <code>.pill/chats/</code>
            </td>
            <td>Run records and chat history. Git-ignored.</td>
          </tr>
        </tbody>
      </table>
      <p>
        Pill adds the ignored entries to the repository's <code>.gitignore</code> itself, writes files atomically, and never overwrites a
        file it can't parse; it reports it instead. Global settings live in <code>%APPDATA%\Pill\</code>.
      </p>
    </>
  );
}

function Agents() {
  return (
    <>
      <p>
        An agent is Claude Code or Codex running headless on one task. Start one from a card's play button, a task page or the command bar.
        Runs keep going when the panel closes.
      </p>

      <h3>Set up Claude Code or Codex</h3>
      <ul>
        <li>
          <strong>Claude Code:</strong> install it as the <Ext href="https://code.claude.com/docs">Claude Code docs</Ext> describe, run{" "}
          <code>claude</code> once and sign in with <code>/login</code>. Use the native Windows install, not WSL.
        </li>
        <li>
          <strong>Codex:</strong> <code>npm install -g @openai/codex</code>, then <code>codex login</code>.
        </li>
        <li>
          <strong>GitHub CLI</strong>, for draft pull requests: <code>winget install --id GitHub.cli</code>, then <code>gh auth login</code>
          .
        </li>
      </ul>
      <p>
        Agents run on each CLI's own login. Pill removes <code>ANTHROPIC_API_KEY</code>, <code>OPENAI_API_KEY</code> and{" "}
        <code>CODEX_API_KEY</code> from their environment so a stray key is never billed by accident. The provider and model are a
        per-project default that each run and chat can change.
      </p>

      <h3>What a run does</h3>
      <ol>
        <li>
          <strong>Preflight.</strong> Checks that the CLI is installed and signed in, the folder is a git repository with a commit, and, for
          ship modes that push, that there is a remote and <code>gh</code> is signed in.
        </li>
        <li>
          <strong>Isolation.</strong> Fetches the base branch and creates a worktree on its own branch, such as{" "}
          <code>agent/&lt;task&gt;-&lt;slug&gt;</code>, under <code>.pill-worktrees</code> next to the repository. Your checkout and current
          branch are never modified.
        </li>
        <li>
          <strong>Run.</strong> Starts the CLI headless. Claude Code gets a tool allowlist and a deny list that always blocks{" "}
          <code>git push</code>, branch switching, <code>gh</code> and network tools; Codex runs in its workspace-write sandbox with network
          access off.
        </li>
        <li>
          <strong>Verification.</strong> Pill runs the project's test and lint commands itself.
        </li>
        <li>
          <strong>Ship.</strong> Opens a draft pull request, pushes the branch only, or keeps the commits local, as the project says.
        </li>
      </ol>

      <h3>Approvals</h3>
      <p>
        When an agent or a chat wants a tool it isn't allowed, the request appears in the notch with the exact command. Allow it once, for
        this run, or always (saved to the project's Claude settings or your Codex rules), or deny it with a message the agent sees.
      </p>

      <h3>Tests</h3>
      <p>
        The first time you start an agent on a project without a test command, Pill offers the one it finds in the project's files (npm,
        pnpm, Yarn, Bun, Cargo, pytest, uv, Poetry, Go, .NET, Deno or Make), offers to have an agent set tests up, or lets you go on
        without. Failing tests go back to the same session for a fix turn, within the limits you set, before the run fails. Without a test
        command, the change ships marked as not verified.
      </p>

      <h3>Reviewing and merging</h3>
      <p>
        The <strong>Agents</strong> tab shows each run as a conversation: the instructions it was given, its messages, folded tool calls,
        Pill's own steps and the outcome, next to the tests, the pull request's checks and review, the changed files, the cost and the
        branch. From there you can open the pull request, open the worktree in your editor, or <strong>Retry with feedback</strong>, which
        resumes the agent's own session with your notes and the failing output.
      </p>
      <p>
        Pill doesn't merge for you. You review and merge the draft pull request on GitHub as usual; the card moves to Review when the pull
        request opens and to Done when it merges, and Pill offers to clean up the worktree. Deleting a finished run removes its worktree, an
        unshipped branch and its history.
      </p>

      <h3>More ways to run</h3>
      <ul>
        <li>
          <strong>Plan mode</strong> stops after the agent writes a plan, so you can edit and approve it before it builds.
        </li>
        <li>
          <strong>Compare</strong> runs one task on several setups at once, Claude Code against Codex or two models, and shows diff size,
          tests and cost side by side. You ship the one you pick.
        </li>
        <li>
          <strong>Queue and autopilot:</strong> a parallel limit and a queue. Tasks marked agent-ready can start on their own inside a time
          window and move themselves across the board.
        </li>
        <li>
          <strong>Usage limits:</strong> when a CLI reports that your plan's limit is reached, Pill holds that CLI's queued runs until the
          reset time instead of failing them.
        </li>
      </ul>
    </>
  );
}

function Chat() {
  return (
    <>
      <p>
        Each project has chats backed by real Claude Code or Codex sessions, on your own login. A chat starts read-only in the repository.
        Switch it to editing and it moves into its own worktree on a <code>chat/&lt;id&gt;</code> branch, which you can ship as a draft pull
        request.
      </p>
      <ul>
        <li>Switch between Claude Code and Codex mid-conversation, and pick the model per chat.</li>
        <li>Replies that propose work come with tasks you can create, and start agents on, in one click.</li>
        <li>
          The chat sidebar holds the project's notes (<code>notes.md</code>, with a live preview) and the project pages of plugins such as
          CVE watch.
        </li>
        <li>
          History is saved per project under <code>.pill/chats/</code>, which is git-ignored.
        </li>
      </ul>
    </>
  );
}

function Today() {
  return (
    <>
      <p>
        The <strong>Today</strong> tab lists what is due today, or overdue, and everything in Doing, across projects, grouped by project.
        Drag rows to reorder them, tick one off, or press play to start an agent. The project picker in the header narrows it to one
        project.
      </p>
      <p>Above the tasks sit plugin cards:</p>
      <ul>
        <li>
          <strong>Now playing</strong> (built in, on by default) works with any app that uses the Windows media controls: artwork, transport
          controls, a seek bar, shuffle and repeat where the player supports them, the player's own volume from the Windows mixer, and a
          switcher when more than one app is playing.
        </li>
        <li>
          <strong>Windows notifications</strong> (built in, off by default) lists Action Center, newest first. Dismiss one or clear all;
          both act on Action Center itself. Windows asks for your permission the first time, and the text stays in memory only.
        </li>
        <li>
          <strong>External plugins</strong> add a card each, with their alerts. Past three, the rest fold into one row.
        </li>
      </ul>
      <p>
        While the notch is closed it shows the time and a status cluster: failed runs, approvals waiting, pull requests ready, running
        agents, tasks due today, notifications and plugin signals. Click a signal to open the tab it belongs to.
      </p>
    </>
  );
}

const HELLO_MANIFEST = `{
  "id": "hello",
  "name": "Hello",
  "description": "Counts how often you wave.",
  "command": "node",
  "args": ["plugin.mjs"],
  "capabilities": ["live", "widget"]
}`;

const HELLO_PLUGIN = `// plugin.mjs: a minimal Pill plugin.
import { createInterface } from "node:readline";

let waves = 0;

function send(message) {
  process.stdout.write(JSON.stringify(message) + "\\n");
}

function render() {
  send({
    type: "widget",
    title: "Hello",
    rows: [{ icon: "heart", text: waves === 0 ? "Nobody has waved yet" : \`Waves: \${waves}\` }],
    actions: [
      { id: "wave", label: "Wave", icon: "person" },
      { id: "reset", label: "Reset" },
    ],
  });
  if (waves > 0) send({ type: "live", icon: "heart", text: String(waves), tone: "accent" });
  else send({ type: "live", clear: true });
}

createInterface({ input: process.stdin }).on("line", (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  if (message.type === "hello") {
    console.error(\`Pill \${message.pill}, protocol \${message.protocol}\`);
  } else if (message.type === "action") {
    if (message.id === "wave") waves += 1;
    if (message.id === "reset") waves = 0;
    render();
  }
});

render();`;

function Plugins() {
  return (
    <>
      <p>
        A plugin is any program that reads JSON lines on stdin and writes JSON lines on stdout. Pill starts it, tells it a few things, and
        draws what it sends with its own components. A plugin never ships HTML, scripts or images; everything it shows is plain text,
        numbers and names from fixed lists.
      </p>
      <table>
        <thead>
          <tr>
            <th>Surface</th>
            <th>Where</th>
            <th>Message</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Signals</td>
            <td>Icons with a count in the collapsed notch</td>
            <td>
              <code>signal</code>
            </td>
          </tr>
          <tr>
            <td>Live item</td>
            <td>A few words or an icon inside the collapsed notch</td>
            <td>
              <code>live</code>
            </td>
          </tr>
          <tr>
            <td>Widget</td>
            <td>A card on Today with rows and buttons</td>
            <td>
              <code>widget</code>
            </td>
          </tr>
          <tr>
            <td>Feed</td>
            <td>Alerts on Today</td>
            <td>
              <code>feed</code>
            </td>
          </tr>
          <tr>
            <td>Tab</td>
            <td>A full page in the header, with a project sidebar</td>
            <td>
              <code>page</code>, <code>nav</code>
            </td>
          </tr>
        </tbody>
      </table>
      <p>
        Each surface sits behind a capability the manifest declares. A new plugin is off until you approve its exact command line and,
        separately, access to your project list; if either changes, Pill asks again. Plugins run inside a Windows Job Object without your
        API keys. Everything is managed in <strong>Settings › Plugins</strong>, where you can also drag plugins into the order they appear
        in.
      </p>
      <p>
        The <Ext href={repoLink("plugins/README.md")}>plugin specification</Ext> has every field, message, block, limit and security rule.
        The <Ext href={repoLink("docs/plugins.md")}>plugin walkthrough</Ext> explains how CVE watch is built.
      </p>

      <h3>Write your first plugin</h3>
      <p>
        This one puts a card with two buttons on Today and a counter in the notch. It needs Node.js. Create a folder{" "}
        <code>%APPDATA%\Pill\plugins\hello\</code> with two files:
      </p>
      <Code file="plugin.json">{HELLO_MANIFEST}</Code>
      <Code file="plugin.mjs">{HELLO_PLUGIN}</Code>
      <p>
        Open <strong>Settings › Plugins</strong>, press <strong>Reload</strong>, turn on <strong>Hello</strong> and confirm the command
        line. The card appears on Today; <strong>Wave</strong> puts a counter in the notch.
      </p>
      <p>You can try it without Pill by piping in the lines Pill would send (PowerShell):</p>
      <Code>{`'', '{"type":"action","id":"wave"}', '{"type":"action","id":"wave"}' | node plugin.mjs`}</Code>
      <p>
        Windows PowerShell 5.1 puts a byte order mark before piped input; the empty first line absorbs it. Write diagnostics to stderr,
        never stdout, send only what changed, and keep live text to a number or two words.
      </p>
    </>
  );
}

function CveWatch() {
  return (
    <>
      <p>
        CVE watch is the reference plugin, in <Ext href={`${REPO_URL}/tree/main/plugins/cve-watch`}>plugins/cve-watch</Ext>. It checks every
        project's dependencies against <Ext href="https://osv.dev">OSV.dev</Ext> and shows the advisories that affect them. It is plain
        Node.js with no dependencies.
      </p>

      <h3>Install</h3>
      <p>You need Node.js 20 or later on PATH. From a clone of the repository (PowerShell):</p>
      <Code>{`# Copy it:
Copy-Item -Recurse .\\plugins\\cve-watch "$env:APPDATA\\Pill\\plugins\\cve-watch"

# Or link it, so it stays current with the repository:
New-Item -ItemType Junction -Path "$env:APPDATA\\Pill\\plugins\\cve-watch" -Target (Resolve-Path .\\plugins\\cve-watch)`}</Code>
      <p>
        Then <strong>Settings › Plugins › Reload</strong>, turn on <strong>CVE watch</strong> and confirm. Pill shows the command it will
        run (<code>node plugin.mjs</code>), that it gets the name and folder of every project, and that it adds a <strong>Security</strong>{" "}
        tab.
      </p>

      <h3>What you see</h3>
      <ul>
        <li>
          <strong>A card on Today</strong> with what was found across all projects, when it last checked, and <strong>Check now</strong>.
        </li>
        <li>
          <strong>Alerts on Today</strong>, one per advisory, package and project, most severe first. Dismiss one, open it on osv.dev, or{" "}
          <strong>Fix with an agent</strong>, which adds a task such as "Update lodash to 4.17.21 (CVE-2021-23337)" and offers to start an
          agent on it.
        </li>
        <li>
          <strong>A shield in the notch</strong> while there are critical or high alerts.
        </li>
        <li>
          <strong>A Security tab</strong> with projects ranked by exposure. A project's page has its counts, an upgrade plan (per package,
          the smallest upgrade that fixes the most advisories, each with Fix with an agent, plus Fix everything), its findings grouped by
          package, ignore buttons, and the lockfiles it read.
        </li>
      </ul>

      <h3>What it reads and sends</h3>
      <p>
        Lockfiles up to four folders deep: npm (<code>package-lock.json</code>, <code>pnpm-lock.yaml</code>, <code>yarn.lock</code>,{" "}
        <code>bun.lock</code>), crates.io (<code>Cargo.lock</code>), PyPI (<code>poetry.lock</code>, <code>uv.lock</code>, pinned{" "}
        <code>requirements*.txt</code>), Go (<code>go.mod</code>), Packagist (<code>composer.lock</code>) and RubyGems (
        <code>Gemfile.lock</code>).
      </p>
      <p>
        Only each package's name, ecosystem and version go to OSV.dev. Never file paths, project names or anything else. It checks when Pill
        starts, when projects or lockfiles change, and every six hours; if OSV.dev can't be reached it keeps showing the last results and
        says when they're from.
      </p>
    </>
  );
}

function Updates() {
  return (
    <>
      <p>
        Pill updates itself from GitHub Releases. It checks once a day, downloads a newer version in the background, and puts a dot on the
        Settings gear when it is ready; <strong>Settings › General</strong> then offers <strong>Restart to update</strong>. You can also
        press <strong>Check for updates</strong> there at any time.
      </p>
      <p>
        Pill only installs updates signed with the project's updater key, whose public half ships inside the app. Builds you make yourself
        from source have no update address, so they don't update themselves.
      </p>
      <p>
        Every version is listed on <Ext href={RELEASES_URL}>GitHub Releases</Ext> with its installer and notes.
      </p>
    </>
  );
}

const FAQ: readonly { q: string; a: ReactNode }[] = [
  {
    q: "Does Pill need an API key?",
    a: "No. Agents and chats run on the login of the Claude Code or Codex CLI you already use, and Pill removes API key variables from their environment.",
  },
  {
    q: "Can an agent change my working copy?",
    a: "No. Agents work in a separate git worktree on their own branch, and Pill only ever pushes that branch. Your checkout and current branch are left alone.",
  },
  {
    q: "What does Pill send over the network?",
    a: "Apart from the git, gh and CLI commands it runs for you and the plugins you turn on, Pill only contacts GitHub Releases to check for updates.",
  },
  {
    q: "Why does Windows warn when I run the installer?",
    a: (
      <>
        The installer isn't code-signed yet, so SmartScreen doesn't recognise it. Choose <strong>More info</strong>, then{" "}
        <strong>Run anyway</strong>, or build Pill from source.
      </>
    ),
  },
  {
    q: "The notch is gone. Where did it go?",
    a: "Pill hides while a full-screen app owns its monitor and steps aside while you drag windows. It comes back on its own. Settings › General picks the display it lives on.",
  },
  {
    q: "Can I use Pill as a normal window?",
    a: "Yes. The pop-out button in the panel's header turns it into a window you can move, resize, maximize and pin on top.",
  },
  {
    q: "Does it run on macOS or Linux?",
    a: "No. Pill is Windows only. It needs Windows 11, or Windows 10 with less testing, on x64.",
  },
  {
    q: "Where are the logs?",
    a: (
      <>
        In <code>%LOCALAPPDATA%\dev.pill.app\logs</code>. Global settings are in <code>%APPDATA%\Pill\</code>; Settings › General › About
        shows the path.
      </>
    ),
  },
];

function Faq() {
  return (
    <>
      <dl>
        {FAQ.map(({ q, a }) => (
          <div key={q} className="mt-6 first:mt-2">
            <dt className="font-semibold text-white/90">{q}</dt>
            <dd className="mt-1.5 ml-0">{a}</dd>
          </div>
        ))}
      </dl>
      <p className="mt-8">
        Something else? <Ext href={`${REPO_URL}/issues`}>Open an issue</Ext> on GitHub.
      </p>
    </>
  );
}
