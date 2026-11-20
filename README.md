# Pill

A notch at the top of your Windows screen for the tasks in your git
repositories, with Claude Code and Codex agents that work on them in
isolated worktrees.

[Website](https://bankkroll.github.io/Windows-Pill/) ·
[Download](https://github.com/BankkRoll/Windows-Pill/releases/latest) ·
[Docs](https://bankkroll.github.io/Windows-Pill/docs/) ·
[Plugins](plugins/README.md)

## Install

Download `Pill_x.y.z_x64-setup.exe` from
[Releases](https://github.com/BankkRoll/Windows-Pill/releases/latest). It
installs per user and updates itself from signed releases. The installer
isn't code-signed yet, so SmartScreen may ask: **More info → Run anyway**.

You need Windows 11, [Git](https://git-scm.com/download/win), and
[Claude Code](https://code.claude.com/docs) or
[Codex](https://github.com/openai/codex) signed in. The
[GitHub CLI](https://cli.github.com/) is optional, for pull requests.

## What it does

- Finds every git repository under the folders you add and keeps their
  tasks in `.pill/`, with a Today view, a board and quick-add.
- Starts an agent on a task in its own worktree and branch, runs your tests,
  and ships a draft PR, a branch or a local commit. Your checkout is never
  touched.
- Asks you in the notch before an agent runs anything it isn't allowed to.
- Shows Now playing, Windows notifications and plugin cards on Today.
- Runs [plugins](plugins/README.md): any program that speaks JSON lines.
  [CVE watch](plugins/cve-watch/) is the reference.

## Development

Requires Node.js 22, pnpm 10, stable Rust and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/).

```sh
pnpm install
pnpm tauri dev
pnpm test && pnpm typecheck
cargo test --manifest-path src-tauri/Cargo.toml
```

`src/` is the React UI, `src-tauri/` the Rust core, `plugins/` the plugin
spec and CVE watch, and `site/` the website. Pushing a `v*` tag builds and
drafts a release.

## License

[Apache-2.0](LICENSE)
