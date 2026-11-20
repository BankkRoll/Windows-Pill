# AI providers

Pill runs coding agents through each vendor's own CLI, so every run uses the
login the user already has (Claude Pro/Max, ChatGPT plan). No API keys are
required, and none are stored by Pill. This matches how Vercel's deepsec
(`vercel-labs/deepsec`) drives the same tools through their SDKs, which spawn
the same CLIs underneath.

## Claude Code

| Step | How |
| --- | --- |
| Installed | `claude` on PATH (or `%USERPROFILE%\.local\bin\claude.exe`) |
| Logged in | `claude auth status` → JSON `{ "loggedIn": true, "authMethod": "claude.ai" }` |
| Run | `claude -p --output-format stream-json --verbose --permission-mode acceptEdits --max-turns N --allowedTools … --disallowedTools … --add-dir <run dir>`; prompt on stdin |
| Plan | `--permission-mode plan`; the plan comes from the `ExitPlanMode` tool input, or the final result text |
| Resume | `--resume <session_id>` (`session_id` from the `system/init` event) |
| Billing | Remove `ANTHROPIC_API_KEY` from the child environment, or Claude Code bills that key instead of the subscription |

Stream events: `system/init` (session id), `assistant` with `text` and
`tool_use` content, `result` with `subtype` (`success`, `error_max_turns`, …),
`is_error`, `num_turns`, `total_cost_usd`.

## Codex

| Step | How |
| --- | --- |
| Installed | `codex` on PATH |
| Logged in | `%CODEX_HOME%\auth.json` or `%USERPROFILE%\.codex\auth.json` exists (written by `codex login`). A logged-out `codex` still runs and then fails with 401, so checking PATH alone isn't enough. |
| Run | `codex exec --experimental-json --sandbox workspace-write --cd <worktree> --skip-git-repo-check --config approval_policy="never" --config sandbox_workspace_write.network_access=false`; prompt on stdin |
| Resume | `… resume <thread_id>` (`thread_id` from `thread.started`), with the same `CODEX_HOME` |
| Parallel runs | Give each run its own temporary `CODEX_HOME` with a copy of `auth.json`, because concurrent runs that share one session database silently do nothing |
| Errors | Capture stderr. Codex can exit 0 with empty turns on auth or quota errors. Match usage-limit text in `turn.failed`, `error` and stderr. |

Stream events: `thread.started`, `turn.started`, `item.started|updated|completed`
(`agent_message`, `reasoning`, `command_execution`, `file_change`,
`todo_list`, `error`), `turn.completed` (usage), `turn.failed`, `error`.

## Choosing a provider

Each project picks its provider in Settings → project → "AI for agents and
chats". Runs and chats record the provider they started with, so a resume,
retry or plan approval always goes back to the same CLI and session.

Settings → Tools shows install and login status for Claude Code, Codex and
the GitHub CLI. Agent preflight checks the project's provider, plus `gh`, a
remote and a test command.

The Codex flags come from OpenAI's TypeScript SDK (`sdk/typescript/src/exec.ts`
in `openai/codex`). Codex isn't installed on the development machine, so the
Codex path is covered by unit tests over the documented event format only.
Run a real Codex agent once after `codex login` to confirm it end to end.
