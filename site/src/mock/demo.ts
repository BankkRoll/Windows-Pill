/**
 * Made-up data for the mockups: generic projects, tracks, notifications,
 * runs and advisories. The advisories are real public ones (Next.js, minimist,
 * lodash, path-to-regexp, axios) so their ids, versions and fixes are accurate.
 */

import type { PageBlock, PageChip } from "./PluginBlocks";

/** A project as the mockups show it. */
export interface DemoProject {
  id: string;
  name: string;
  /** From the app's project palette (workspace.rs). */
  color: string;
}

/** The demo projects. */
export const PROJECTS = {
  storefront: { id: "p_storefront", name: "storefront", color: "#2F7BFF" },
  gateway: { id: "p_gateway", name: "api-gateway", color: "#30D158" },
  docs: { id: "p_docs", name: "docs-site", color: "#FFB340" },
  mobile: { id: "p_mobile", name: "mobile-app", color: "#BF5AF2" },
  tokens: { id: "p_tokens", name: "design-tokens", color: "#64D2FF" },
} as const satisfies Record<string, DemoProject>;

/** The track on the Now playing card. */
export const TRACK = {
  app: "Media Player",
  title: "Slow Harbour",
  artist: "Northern Room",
  positionMs: 83_000,
  durationMs: 225_000,
  volume: 0.62,
};

/** A notification on the Windows notifications card. */
export interface DemoNotification {
  id: number;
  app: string;
  headline: string;
  body: string;
  age: string;
}

/** Action Center, newest first. */
export const NOTIFICATIONS: readonly DemoNotification[] = [
  { id: 1, app: "Calendar", headline: "Design review", body: "In 10 minutes · Room 4B", age: "2m" },
  { id: 2, app: "Snipping Tool", headline: "Screenshot copied to clipboard", body: "Select here to mark up and share.", age: "14m" },
  { id: 3, app: "Mail", headline: "Nightly build finished", body: "api-gateway: 412 tests passed, 0 failed.", age: "1h" },
];

/** A task row on the Today tab. */
export interface DemoTask {
  title: string;
  priority: 1 | 2 | 3 | null;
  due: { text: string; tone: "overdue" | "today" | "soon" | "later" } | null;
  tags: string[];
  status: "todo" | "doing";
  agent: boolean;
}

/** Today's tasks, grouped by project. */
export const TODAY: readonly { project: DemoProject; tasks: DemoTask[] }[] = [
  {
    project: PROJECTS.storefront,
    tasks: [
      { title: "Fix flaky checkout test", priority: 1, due: { text: "Today", tone: "today" }, tags: ["ci"], status: "doing", agent: true },
      {
        title: "Show stock levels on product cards",
        priority: 2,
        due: { text: "Today", tone: "today" },
        tags: [],
        status: "todo",
        agent: false,
      },
    ],
  },
  {
    project: PROJECTS.gateway,
    tasks: [
      { title: "Rate-limit the /search endpoint", priority: null, due: null, tags: ["perf"], status: "doing", agent: false },
      {
        title: "Rotate staging TLS certificates",
        priority: 1,
        due: { text: "Yesterday", tone: "overdue" },
        tags: [],
        status: "todo",
        agent: false,
      },
    ],
  },
  {
    project: PROJECTS.docs,
    tasks: [
      {
        title: "Update the install guide for v0.3",
        priority: 3,
        due: { text: "Today", tone: "today" },
        tags: ["docs"],
        status: "todo",
        agent: false,
      },
    ],
  },
];

/** An alert in CVE watch's Today card. */
export interface DemoAlert {
  severity: "critical" | "high" | "medium";
  title: string;
  detail: string;
  project: DemoProject;
}

/** CVE watch's alerts, most severe first. */
export const ALERTS: readonly DemoAlert[] = [
  {
    severity: "critical",
    title: "Authorization Bypass in Next.js Middleware",
    detail: "next 14.2.3 → 14.2.25 · CVE-2025-29927",
    project: PROJECTS.storefront,
  },
  {
    severity: "critical",
    title: "Prototype pollution in minimist",
    detail: "minimist 1.2.5 → 1.2.6 · CVE-2021-44906",
    project: PROJECTS.gateway,
  },
  {
    severity: "high",
    title: "Command Injection in lodash",
    detail: "lodash 4.17.20 → 4.17.21 · CVE-2021-23337",
    project: PROJECTS.docs,
  },
];

/** A run in the Agents tab's list. */
export interface DemoRun {
  id: string;
  title: string;
  project: DemoProject;
  state: "running" | "pr_open" | "committed" | "failed" | "awaiting_approval";
  label: string;
}

/** The Agents tab's runs, newest first. */
export const RUNS: readonly DemoRun[] = [
  { id: "r1", title: "Fix flaky checkout test", project: PROJECTS.storefront, state: "pr_open", label: "PR open" },
  { id: "r2", title: "Rate-limit the /search endpoint", project: PROJECTS.gateway, state: "running", label: "Running" },
  { id: "r3", title: "Add dark mode tokens", project: PROJECTS.tokens, state: "awaiting_approval", label: "Plan ready" },
  { id: "r4", title: "Update lodash to 4.17.21 (CVE-2021-23337)", project: PROJECTS.docs, state: "committed", label: "Committed" },
  { id: "r5", title: "Migrate push tokens table", project: PROJECTS.mobile, state: "failed", label: "Failed" },
];

/** A block of the selected run's transcript. */
export type DemoTranscript =
  | { kind: "brief" }
  | { kind: "step"; text: string }
  | { kind: "tools"; summary: string }
  | { kind: "message"; html: string }
  | { kind: "result"; ok: boolean; text: string };

/** The selected run, as its transcript reads after it shipped. */
export const RUN_DETAIL = {
  title: "Fix flaky checkout test",
  branch: "agent/t_8f2c-fix-flaky-checkout-test",
  setup: "Claude Code · Opus",
  duration: "6m 41s",
  turns: 23,
  diff: { insertions: 38, deletions: 12, files: 3 },
  cost: "$0.84",
  files: ["src/checkout/cart.ts", "src/checkout/cart.test.ts", "src/checkout/useCartTotal.ts"],
  transcript: [
    { kind: "brief" },
    { kind: "step", text: "Checking Claude Code, gh and project settings" },
    { kind: "step", text: "Fetching origin/main" },
    { kind: "step", text: "Creating worktree on agent/t_8f2c-fix-flaky-checkout-test from origin/main" },
    { kind: "step", text: "Claude Code is working" },
    { kind: "tools", summary: "Read 4 · Grep 2 · Bash" },
    {
      kind: "message",
      html: "The test asserts the total before the debounced price refresh lands, so it fails when CI is slow. I made <code>useCartTotal</code> expose a <code>ready</code> promise and the test now awaits it instead of a fixed timeout.",
    },
    { kind: "tools", summary: "Edit 3 · Bash 2" },
    { kind: "step", text: "Verifying: pnpm test (found in package.json)" },
    { kind: "result", ok: true, text: "Tests passed" },
    { kind: "step", text: "Pushing agent/t_8f2c-fix-flaky-checkout-test to origin" },
    { kind: "step", text: "Opening a draft pull request" },
    { kind: "result", ok: true, text: "Draft PR: github.com/acme/storefront/pull/482" },
  ] satisfies DemoTranscript[],
};

/** The Security tab's sidebar: every project with CVE watch's badge, most exposed first. */
export const SECURITY_NAV: readonly { project: DemoProject | null; badge: PageChip | null }[] = [
  { project: null, badge: { text: "2 critical", tone: "red" } },
  { project: PROJECTS.storefront, badge: { text: "1 critical", tone: "red" } },
  { project: PROJECTS.gateway, badge: { text: "1 critical", tone: "red" } },
  { project: PROJECTS.docs, badge: { text: "2 high", tone: "amber" } },
  { project: PROJECTS.mobile, badge: { text: "1", tone: "neutral" } },
  { project: PROJECTS.tokens, badge: null },
];

/** CVE watch's page for storefront, in the shape the plugin sends. */
export const SECURITY_PAGE: readonly PageBlock[] = [
  {
    type: "header",
    title: "storefront",
    subtitle: "1 critical · 2 high · 1 more in 3 packages",
    tone: "red",
    actions: [{ label: "Check now", icon: "shield" }],
  },
  {
    type: "stats",
    items: [
      { label: "Critical", value: "1", tone: "red" },
      { label: "High", value: "2", tone: "amber" },
      { label: "Medium & low", value: "1", tone: "neutral" },
      { label: "Packages", value: "1,184", detail: "in 1 lockfile", tone: "neutral" },
    ],
  },
  {
    type: "section",
    title: "Upgrade plan",
    count: 3,
    blocks: [
      {
        type: "notice",
        tone: "accent",
        icon: "bot",
        title: "3 upgrades fix 4 advisories",
        text: "One task for an agent to apply every upgrade below, then run the tests.",
        actions: [{ label: "Fix everything", icon: "bot", primary: true }],
      },
      {
        type: "list",
        rows: [
          {
            title: "Upgrade next 14.2.3 → 14.2.25",
            detail: "Fixes 2 advisories (1 critical, 1 high)",
            severity: "critical",
            actions: [{ label: "Fix with an agent", icon: "bot", primary: true }],
          },
          {
            title: "Upgrade path-to-regexp 0.1.7 → 0.1.10",
            detail: "Fixes 1 high advisory",
            severity: "high",
            chips: [{ text: "Transitive", tone: "neutral" }],
            actions: [{ label: "Fix with an agent", icon: "bot", primary: true }],
          },
          {
            title: "Upgrade axios 1.5.1 → 1.6.0",
            detail: "Fixes 1 advisory",
            severity: "medium",
            actions: [{ label: "Fix with an agent", icon: "bot", primary: true }],
          },
        ],
      },
    ],
  },
  {
    type: "section",
    title: "next 14.2.3",
    count: 2,
    blocks: [
      {
        type: "list",
        rows: [
          {
            title: "next 14.2.3",
            detail: "npm · direct · pnpm-lock.yaml",
            severity: "critical",
            chips: [{ text: "Upgrade to 14.2.25", tone: "accent" }],
            actions: [{ label: "Ignore this package" }],
          },
          {
            title: "Authorization Bypass in Next.js Middleware",
            detail: "GHSA-f82v-jwr5-mffw · CVE-2025-29927 · 14.2.3 → 14.2.25",
            severity: "critical",
            chips: [{ text: "CVSS 9.1", tone: "neutral" }],
          },
          {
            title: "Next.js Cache Poisoning",
            detail: "GHSA-gp8f-8m3g-qvj9 · CVE-2024-46982 · 14.2.3 → 14.2.10",
            severity: "high",
            chips: [{ text: "CVSS 7.5", tone: "neutral" }],
          },
        ],
      },
    ],
  },
];
