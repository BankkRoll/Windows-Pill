# Roadmap

Every feature, fix and edge case needed for Pill v1. Items are checked off in
the same commit that completes them. Milestones set build order only;
nothing ships as a stub.

## M1 — The notch shell

- [x] Transparent, frameless, always-on-top window with no taskbar button
- [x] Notch attached to the top edge, with soft S-curve shoulders (single SVG path)
- [x] Click-through outside the visible shape (Win32 layered + transparent, 60 Hz hit test)
- [x] Rounded-corner-accurate hit testing
- [x] Never steals focus (`WS_EX_NOACTIVATE`), with explicit take/release focus
- [x] Hover intent: opens after 120 ms, closes 350 ms after leaving, ignores fast swipes
- [x] Spring morph of width, height, radius and shoulders together; content fades in after and out before
- [x] Reduced motion: shape snaps and only fades remain
- [x] Clock and date with tabular numbers, re-aligned each minute and after sleep
- [x] Hides while a full-screen app owns the notch's monitor
- [x] Screen-capture tools and topmost overlays never count as full-screen
- [x] Per-monitor DPI; re-placed when displays change
- [x] Monitor choice from config
- [x] Single instance; tray icon with Quit
- [x] Peek state with color-coded status cluster (failed / PR / agent / due / notification)
- [x] Drag guard: keep input while a drag that began inside is held

## M2 — Projects and tasks

- [x] Global config: roots and known projects
- [x] Root scan for git repos (depth-limited, skips heavy folders, no console flashes)
- [x] Projects added by hand outside the roots
- [x] `.pill/project.json` with defaults (name, color, base branch, test/lint command, editor, agent rules, `commitTasks`)
- [x] `.pill/tasks.json` schema v1 that preserves unknown fields on rewrite
- [x] Global Inbox stored in `%APPDATA%\Pill\inbox.json`
- [x] Atomic writes; edits Pill made itself don't echo back through the watcher
- [x] `.gitignore` management: always `.pill/runs/`; `.pill/tasks.json` unless `commitTasks`
- [x] File watcher on every `.pill` folder and root, debounced; hand edits show up live
- [x] Periodic rescan for new repos
- [x] Recently active projects float to the top (last commit, last task change)
- [x] Task CRUD: create, update, move between projects and Inbox, delete, reorder
- [x] Quick-add parser: `#project` (fuzzy), `!1`–`!3`, `@today` / `@tomorrow` / `@fri` / `@2026-11-21` / `@+3d`, `+tag`
- [x] Global hotkey (configurable, default Ctrl+Shift+Space) opens quick-add with focus
- [x] Quick-add mode: live chips for project, priority and due; Enter saves, Esc cancels, focus returns to the previous app
- [x] Panel chrome: tab bar, project picker, settings gear, quick-add bar *(Board, Notes and Agents tabs land with their milestones)*
- [x] Today tab: due today plus Doing, across projects, grouped by project; check off, reorder
- [x] "Due today" signal feeds the status cluster
- [x] First run: no roots set, so the panel opens Settings with a folder picker
- [x] Settings: roots (add/remove with native picker), rescan
- [x] Malformed `tasks.json` / `project.json` surfaces a clear error and is never overwritten
- [x] Hotkey recorder in Settings, with conflict errors and rollback to the previous hotkey
- [x] File logging (`%LOCALAPPDATA%\dev.pill.app\logs`) for hotkey, focus and hover diagnostics
- [x] Fix: Tauri `set_focus` rewrote window styles and broke the overlay; focus is now taken with Win32

## M3 — Board and notes

- [x] Kanban board with 5 columns (Inbox → To do → Doing → Review → Done), all fitting the panel
- [x] One project or All projects, with project color stripes on cards
- [x] Drag and drop between columns and within a column
- [x] Card hover actions: ▶ Start agent, Open folder, Open in editor, Copy task as prompt
- [x] Card detail: title, notes, priority, tags, due, agent-ready flag, agent history, PR link
- [x] Notes tab: markdown editor for `notes.md` with live preview
- [x] Pin note snippets to a task (as agent context)
- [x] Open folder (Explorer) and editor auto-detect (Cursor → VS Code → Explorer), overridable per project
- [x] Per-project settings editor (name, color, base branch, commands, editor, rules, commit tasks)

## M4 — Agent runner (local)

- [x] Preflight: `claude` on PATH and logged in, `gh` logged in, remote exists, test command set; fix-it messages
- [x] `git fetch` plus worktree on `agent/<task-id>-<slug>` from the latest base branch, under `<root>\.pill-worktrees\`
- [x] Prompt template rendered with all fields and saved to `runs/<id>/prompt.md`
- [x] Headless Claude Code: stream-json, `acceptEdits`, tool allowlist and deny list, max turns, timeout
- [x] `ANTHROPIC_API_KEY` stripped from the child environment
- [x] Windows Job Object so Stop kills the whole process tree
- [x] Stream parser: tool used, file edited, test run, finished; transcript saved to `transcript.jsonl`
- [x] `run.json` state machine: running → verifying → failed / pr_open / stopped / interrupted
- [x] Pill re-runs the test command itself after Claude exits
- [x] Runs survive closing the panel; marked interrupted on next launch after a quit
- [x] Agents tab: list, live log, files changed, tests, branch; Stop and Open worktree
- [x] Agent plan mode: plan, approve or edit, then build (per run or per-project default)

## M5 — PRs and review loop

- [x] Push only the agent branch; `gh pr create --draft` with the body from `pr.md`
- [x] Card moves to Review, PR link stored, green signal
- [x] Windows toast with an Open PR action
- [x] Retry with feedback: resume the same Claude session with the failure output
- [x] Worktree cleanup (button, plus prompt after merge or close)
- [x] Run queue enforcing the parallel cap, with a warning when the cap is exceeded
- [x] Agent-ready tasks from Inbox / To do are queued automatically (opt-in per task)
- [x] Scheduled runs: work agent-ready tasks inside a time window
- [x] GitHub issues as cards (assigned to me, per project), with PR review and CI status synced back

## M4b — Project chat and providers

- [x] Chat tab: per-project conversations backed by real Claude Code sessions (your login), resumable
- [x] Chat runs read-only in the repo by default; "Let it edit" runs it in a worktree like an agent
- [x] Chats can propose tasks ("find issues and open a PR for each"); approve to create them and start runs
- [x] Chat history saved per project under `.pill/chats/` (gitignored)
- [x] Provider abstraction: Claude Code and Codex CLI behind one contract, chosen per project *(Codex verified by unit tests; needs a real run after `codex login`)*
- [x] Per-provider install and login detection with fix-it messages

## M4c — Design, models and windows

- [x] Settings: two-pane rail (project, Projects, Agents, Accounts, General), shared kit, sticky save bar
- [x] Pop-out window: movable, resizable, maximize, pin on top, remembers bounds
- [x] Model and effort pickers, loaded live from both CLIs (Claude `initialize`, `codex debug models`)
- [x] Chat redesign: token streaming, model label, copy, code copy, retry, follow-latest, composer picker
- [x] Themes: black, graphite, glass (shared wallpaper glass on notch and pop-out), tint levels
- [x] Notch steps aside while windows are dragged (Windows Snap bar); hover or click to open
- [x] Notes tab folded into the chat sidebar; segmented controls redesigned; pointer cursors
- [x] Pop-out: no gap at the edges when maximized; verify Aero Snap to screen edges
- [x] Provider is a per-chat and per-run choice (the project only sets the default); switch mid-chat
- [x] Compare runs: start one task on Claude and Codex at once, compare diff, tests and cost, ship the winner
- [x] Command bar: the bottom input runs commands or asks AI and shows previews to approve or deny
      (create tasks, spin up review agents); visible chips replace the `# ! @ +` syntax
- [x] Autonomous tasks: hands-off tasks start from autopilot and move themselves To do → Doing →
      Review (Done when the PR merges); failures go back with the reason on the card; one automatic
      fix turn on failed checks (configurable); toasts only for plans, tool approvals, reviews,
      failures and comparisons, skipped while that run or task is on screen; clicking one opens Pill on it
- [x] Codex end-to-end run after `codex login` (app-server protocol, verified live)
- [x] Tool approvals from Pill for both CLIs: once, per chat/run, always (saved to the project's Claude settings or Codex rules), deny
- [x] Custom select, date, time and checkbox controls; no native form controls
- [x] Glass background picker: Windows wallpapers, any image, original gradients
- [x] Project repository status and git settings (finish mode, branch prefix, lint)
- [x] Switch a chat between read-only and editing
- [x] Notch "Keep on top" setting
- [x] Light theme (needs every surface and text color on theme tokens)
- [x] Screenshot pass over every page after these land

## M6 — Plugins and polish

- [x] Plugin system: built-in plugins with enable toggles; signals, live item, panel widget
- [x] External plugin protocol: local process speaking JSON lines over stdio (`docs/plugins.md`)
- [x] Now playing plugin (Windows media sessions: Spotify, browsers, any player), with controls and artwork
- [x] Windows notifications plugin feeding the notification signal
- [x] Voice quick-add (Windows speech recognition, offline)
- [x] Settings: monitor, launch at startup, accent, hover delays, agent limits, autopilot window, CLI status (claude / gh: found, logged in, version)
- [x] Launch at startup (autostart)
- [x] Installer (NSIS + MSI), signing config wired but off
- [x] Auto-update from GitHub Releases (signed `latest.json`)
- [x] Edge cases: no remote, dirty base branch, rate limits, missing CLIs, renamed or deleted repos
- [x] README with install, usage and plugin authoring docs

## M7 — Run lifecycle

- [x] Runs that change no code finish as Answered with the reply kept, not Failed
- [x] Test command optional: without one, changes ship marked as not verified
- [x] Delete a finished run (worktree, unshipped branch, history); clear all runs that didn't finish
- [x] Run transcript as a conversation: agent messages, folded tool calls, steps, outcome, the instructions sent
- [x] Run activity saved to disk, so transcripts survive restarts
- [x] Task page shows every run on the task as a conversation
- [x] Test command found from the project's files (npm/pnpm/yarn/bun, Cargo, pytest/uv/poetry, Go, .NET, Deno, Make)
- [x] No test command: ask once per project (use the found one, have an agent set tests up, or not now)
- [x] Agents set tests up when none exist; Pill runs what they added and offers to keep the command

## M8 — Security feed and plugin depth

- [x] CVE plugin, open source in the repo as the reference external plugin (`plugins/cve-watch`)
  - [x] Scan every project's dependencies: npm, pnpm, Yarn, Bun, Cargo, Poetry, uv, pip, Go, Composer and Bundler lockfiles, checked against OSV.dev
  - [x] Feed of new CVEs for packages the projects actually use, with severity and fixed version
  - [x] Findings land in the notch's notifications: dismiss one, clear all, or start an agent to fix it
  - [x] Plugin authoring walkthrough in `docs/plugins.md` built around it
  - [x] Plugin protocol: `projects` capability (with explicit approval) and `feed` items with links and suggested tasks
- [x] Windows notifications: dismiss one or clear all from Pill, mirrored to the Action Center
- [ ] Windows notifications: show images (screenshots, hero and inline images) and action buttons
  - [x] App icons, and the screenshot behind Snipping Tool notifications (opens on click)
  - [x] Cleaner list: newest first, relative times, two-line bodies, "Show all" scrolls the rest
  - [ ] Hero and inline images: the listener API only exposes a toast's text
  - [ ] Action buttons
- [x] Now playing: seek bar with position, shuffle/repeat, per-app sessions
- [x] Now playing: per-app volume and mute, through the Windows volume mixer

## M9 — Plugin pages

- [x] `tab` capability: an external plugin adds a header tab (title and icon from the manifest), shown in the enable confirmation; it grants no access, so the approval is unchanged
- [x] Pages pulled on demand: `view` / `view` closed while a page is on screen, `page` answers and updates, `error` instead of a page
- [x] Pages cached per plugin and project (least recently used first), so reopening is instant; honest loading, slow, error and stopped states
- [x] Typed blocks drawn by Pill's own components: header, stats, section, list, table, text, keyvalue, notice, empty
- [x] Strict validation in the core: sizes, counts, nesting, string lengths, vocabularies; bad blocks, rows and buttons dropped one by one and reported in Settings
- [x] Page buttons: round-trip actions (`page` on `action`), https links, and task buttons through the fenced feed task path with "Start agent"
- [x] Full-page tab with a project sidebar ("All projects", then every project); plugins rank projects and add badges with `nav`; the sidebar and the header picker share one selection
- [x] Rows that open a project's page (`open`), for overview pages
- [x] Project pages also under Project notes in each project's Chat sidebar, sharing the cache and view lifecycle
- [x] Works in the notch and the pop-out window; the plugin learns which pages are on screen across both
- [x] CVE watch Security tab
  - [x] Overview: totals, projects ranked by exposure, the most widespread advisories, last and next check, Check now
  - [x] Project page: upgrade plan (smallest upgrade per package fixing the most advisories) with a task per package and Fix everything; findings grouped by package with advisory details and links
  - [x] Ignore a package or an advisory per project, persisted, with an Ignored section to undo
  - [x] Scanned lockfiles; direct and transitive (npm, pnpm, Cargo, Go, Bundler) and dev-only (npm, pnpm, Composer) marked where the lockfile records it
  - [x] Version comparison per ecosystem (SemVer, PEP 440, Gem::Version, numeric Composer); "see advisory" instead of guessing
  - [x] Card, alerts, tab and badges built from one report so their counts agree; the Today card cut down to a glance
- [x] Protocol docs: tab capability, `view` / `page` / `nav`, block reference with limits, safety
- [x] Plugin specification in `plugins/README.md`: discovery, manifest, consent, process model, every message, blocks, limits, versioning, security, a hello world plugin; `docs/plugins.md` stays the CVE watch walkthrough and links to it
- [x] Doc tests check both plugin docs against the code: examples parse, and limits, vocabularies and reserved ids match

## M10 — Many plugins

- [x] User-defined order for external plugins in Settings → Plugins: drag handle, or Alt+Up/Down on a focused row; saved in the config (`plugins.order`), new plugins appended, removed ones dropped
- [x] Every plugin list follows that order: header tabs, Chat's project sidebar, Today cards, notch signals and the live item
- [x] Header shows at most three plugin tabs; the rest open from a `+N` menu with icon, title and badge; an open tab always takes a header slot
- [x] "Show in the top bar" per tab plugin (`plugins.menuTabs`), to keep a tab in the menu only
- [x] Chat sidebar folds plugin pages past four into an inline "+N more" row; the open page stays visible
- [x] Today folds third-party cards past three into one "+N more plugins" row that expands in place
