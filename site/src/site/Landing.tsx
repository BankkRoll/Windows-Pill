/**
 * The landing page: what Pill is, drawn with the app's own UI, and how to
 * get it.
 */

import type { ReactNode } from "react";
import { CardShot, HeroNotch, PanelShot } from "../mock/Shots";
import { CveWatchCard, NotificationsCard, NowPlayingCard } from "../mock/Today";
import { REPO_URL } from "../lib/release";
import { DOCS, repoLink, SiteFooter, SiteHeader } from "./Chrome";
import { DownloadButton } from "./Download";
import { Requirements } from "./Requirements";

/** The whole landing page. */
export function Landing() {
  return (
    <>
      <a
        href="#main"
        className="sr-only z-50 rounded-lg bg-accent px-3 py-2 text-on-accent focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
      >
        Skip to content
      </a>
      <SiteHeader current="home" />
      <main id="main">
        <Hero />
        <TodaySection />
        <AgentsSection />
        <PluginsSection />
        <MoreSection />
        <GetSection />
      </main>
      <SiteFooter />
    </>
  );
}

function Hero() {
  return (
    <section aria-labelledby="hero-title" className="overflow-hidden">
      <div className="mx-auto max-w-6xl px-4 pt-16 pb-12 sm:px-6 sm:pt-24">
        <p className="text-[14px] font-medium text-white/50">For Windows 11 · Free and open source</p>
        <h1
          id="hero-title"
          className="mt-3 max-w-3xl font-display text-[40px] leading-[1.08] font-semibold tracking-[-0.02em] text-white sm:text-[56px]"
        >
          Your projects, in the notch.
        </h1>
        <p className="mt-5 max-w-2xl text-[17px] leading-relaxed text-white/65 sm:text-[19px]">
          Pill sits at the top centre of your screen. Hover it to see what's due today, what's playing and what your coding agents are
          doing, across every git repository on your machine.
        </p>
        <div className="mt-8 flex flex-wrap items-start gap-x-4 gap-y-3">
          <DownloadButton showMeta />
          <a
            href={DOCS}
            className="inline-flex h-11 items-center rounded-xl px-5 text-[15px] font-medium text-white/80 ring-1 ring-white/12 transition-colors hover:bg-white/6 hover:text-white"
          >
            Read the docs
          </a>
        </div>
      </div>
      <div className="mx-auto max-w-[1180px] px-4 pb-24 sm:px-6">
        <HeroNotch />
      </div>
    </section>
  );
}

/** A section's heading and lead paragraph. */
function Intro({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <div className="max-w-2xl">
      <h2 id={id} className="font-display text-[30px] leading-tight font-semibold tracking-[-0.015em] text-white sm:text-[36px]">
        {title}
      </h2>
      <div className="mt-4 text-[17px] leading-relaxed text-white/60">{children}</div>
    </div>
  );
}

function Section({ labelledBy, children }: { labelledBy: string; children: ReactNode }) {
  return (
    <section aria-labelledby={labelledBy} className="border-t border-white/[0.06]">
      <div className="mx-auto max-w-6xl px-4 py-20 sm:px-6 sm:py-28">{children}</div>
    </section>
  );
}

function Caption({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="mt-4 px-1">
      <h3 className="text-[15px] font-semibold text-white/90">{title}</h3>
      <p className="mt-1 text-[14px] leading-relaxed text-white/50">{children}</p>
    </div>
  );
}

function TodaySection() {
  return (
    <Section labelledBy="today-title">
      <Intro id="today-title" title="Today, at a glance.">
        <p>
          The first tab gathers what needs you now: tasks due today and in progress across every project, the track that's playing, Windows
          notifications, and cards from your plugins. While the notch is closed, it shows the time and a few coloured signals.
        </p>
      </Intro>
      <div className="mt-12 grid items-start gap-10 md:grid-cols-3 md:gap-5">
        <div>
          <CardShot label="The Now playing card: a track with artwork, a seek bar, transport controls, shuffle, repeat and the player's own volume.">
            <NowPlayingCard />
          </CardShot>
          <Caption title="Now playing">
            Any app that uses the Windows media controls, with seeking, shuffle, repeat and that app's own volume.
          </Caption>
        </div>
        <div>
          <CardShot label="The Windows notifications card listing three notifications from Calendar, Snipping Tool and Mail.">
            <NotificationsCard />
          </CardShot>
          <Caption title="Notifications">Read, dismiss or clear Action Center without leaving what you're doing.</Caption>
        </div>
        <div>
          <CardShot label="The CVE watch card: 2 critical and 4 high advisories in 4 projects, with three alerts listed.">
            <CveWatchCard />
          </CardShot>
          <Caption title="Plugin cards">
            Plugins add a card and alerts. Each alert can be dismissed, opened, or handed to an agent as a task.
          </Caption>
        </div>
      </div>
    </Section>
  );
}

const STEPS = [
  ["Preflight", "Checks the CLI is installed and signed in, and that the folder is a git repository with a commit."],
  ["Isolation", "Creates a worktree on its own branch next to the repository. Your checkout is never touched."],
  ["Run", "Starts Claude Code or Codex headless. Anything it isn't allowed to do waits for your approval in the notch."],
  ["Verification", "Pill runs your test and lint commands itself and sends failures back to the agent for a fix."],
  ["Ship", "Per project: a draft pull request, a pushed branch, or commits kept local."],
] as const;

function AgentsSection() {
  return (
    <Section labelledBy="agents-title">
      <Intro id="agents-title" title="Hand a task to Claude Code or Codex.">
        <p>
          Start an agent from any task. It works on its own branch while you keep working, and the Agents tab shows the run as it happens:
          what it read and changed, the tests, the cost and the pull request.
        </p>
      </Intro>
      <div className="mt-12">
        <PanelShot
          tab="agents"
          label="The Agents tab: a list of five runs, and the selected run's transcript ending in Tests passed and a draft pull request, next to its tests, pull request checks and changed files."
        />
      </div>
      <ol className="mt-12 grid gap-x-8 gap-y-6 sm:grid-cols-2 lg:grid-cols-5">
        {STEPS.map(([title, text], i) => (
          <li key={title}>
            <p className="text-[13px] font-medium text-white/35 tabular-nums">{String(i + 1).padStart(2, "0")}</p>
            <h3 className="mt-1 text-[15px] font-semibold text-white/90">{title}</h3>
            <p className="mt-1 text-[14px] leading-relaxed text-white/50">{text}</p>
          </li>
        ))}
      </ol>
    </Section>
  );
}

function PluginsSection() {
  return (
    <Section labelledBy="plugins-title">
      <Intro id="plugins-title" title="Plugins that draw real pages.">
        <p>
          A plugin is any program that reads and writes lines of JSON. It can put a signal or a few words in the notch, add a card and
          alerts to Today, or a whole tab with typed pages that Pill draws with its own components.
        </p>
        <p className="mt-4">
          CVE watch, the reference plugin, checks every project's lockfiles against{" "}
          <a href="https://osv.dev" className="text-white/80 underline-offset-2 hover:underline">
            OSV.dev
          </a>{" "}
          and plans the smallest upgrades that fix what it finds.
        </p>
      </Intro>
      <div className="mt-12">
        <PanelShot
          tab="security"
          label="The Security tab from CVE watch: a sidebar of projects ranked by exposure, and the storefront project's page with its counts, an upgrade plan and the advisories for the next package."
        />
      </div>
      <p className="mt-8 flex flex-wrap gap-x-6 gap-y-2 text-[15px]">
        <a href={`${DOCS}#plugins`} className="font-medium text-accent hover:underline">
          Write your first plugin
        </a>
        <a href={repoLink("plugins/README.md")} className="text-white/60 hover:text-white">
          Plugin specification
        </a>
        <a href={`${DOCS}#cve-watch`} className="text-white/60 hover:text-white">
          Install CVE watch
        </a>
      </p>
    </Section>
  );
}

const MORE = [
  ["Board", "Five columns per project or for all of them, with drag and drop. GitHub issues assigned to you show up as cards."],
  ["Chat", "Per-project chats backed by real Claude Code or Codex sessions. Read-only until you let one edit in its own worktree."],
  ["Command bar", "Type a task with #project, !1, @fri and +tag. Press / for commands, ? to ask about your projects."],
  [
    "Approvals",
    "When an agent wants a tool it isn't allowed, the notch shows the exact command. Allow once, for the run, always, or deny.",
  ],
  [
    "Your own login",
    "Agents run on each CLI's own sign-in. Pill removes API keys from their environment so nothing is billed by accident.",
  ],
  ["Plain files", "Tasks and notes live in each repository's .pill folder. Edits from an editor or git pull show up live."],
] as const;

function MoreSection() {
  return (
    <Section labelledBy="more-title">
      <Intro id="more-title" title="And the rest of the panel.">
        <p>Everything lives in the same notch, one hover or Ctrl+Shift+Space away.</p>
      </Intro>
      <dl className="mt-12 grid gap-x-10 gap-y-8 sm:grid-cols-2 lg:grid-cols-3">
        {MORE.map(([title, text]) => (
          <div key={title} className="border-t border-white/[0.08] pt-5">
            <dt className="text-[15px] font-semibold text-white/90">{title}</dt>
            <dd className="mt-1.5 text-[14px] leading-relaxed text-white/50">{text}</dd>
          </div>
        ))}
      </dl>
    </Section>
  );
}

function GetSection() {
  return (
    <Section labelledBy="get-title">
      <div className="grid gap-12 lg:grid-cols-[1fr_1.1fr] lg:gap-16">
        <div>
          <Intro id="get-title" title="Get Pill.">
            <p>
              One installer, per user, with no administrator prompt. Pill updates itself from GitHub Releases and only installs updates
              signed with the project's key.
            </p>
          </Intro>
          <div className="mt-8">
            <DownloadButton showMeta />
          </div>
          <p className="mt-6 max-w-xl text-[14px] leading-relaxed text-white/45">
            The installer isn't code-signed yet, so Windows SmartScreen may warn the first time. Choose{" "}
            <strong className="text-white/70">More info</strong>, then <strong className="text-white/70">Run anyway</strong>. You can also{" "}
            <a href={`${REPO_URL}#building-from-source`} className="text-white/65 underline-offset-2 hover:text-white hover:underline">
              build it from source
            </a>
            .
          </p>
        </div>
        <Requirements />
      </div>
    </Section>
  );
}
