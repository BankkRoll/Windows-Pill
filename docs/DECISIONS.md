# Decisions

Answers to the spec's open questions, recorded 2026-11-19.

| Question | Decision |
| --- | --- |
| Name | **Pill**. Identifier `dev.pill.app`, global config at `%APPDATA%\Pill\`. |
| Scope | All six milestones, production quality from the first commit. Milestones set build order only. |
| Project roots | Several roots, chosen in Settings with a native folder picker. No roots ship by default; first launch opens Settings. |
| Claude Code | Native Windows install (not WSL). |
| Title bars | Float over title bars. No reserve-space app bar in v1. |
| `tasks.json` in git | Per-project toggle (`commitTasks`), default **gitignored**. |
| Agent scope | Hand-started runs, plus Inbox/To do tasks flagged **agent-ready**, which queue automatically up to the parallel cap. |
| Agent permissions | `acceptEdits` permission mode with a tool allowlist and deny list. `git push`, `gh` and network tools are denied. |
| Default editor | Auto-detect: Cursor, then VS Code, then Explorer. Can be overridden per project. |
| License | Apache-2.0. |
| Updates | Tauri updater with a signed `latest.json` on GitHub Releases. |
| Code signing | Unsigned for now. The signing config is wired up but turned off. |

## Pulled forward from "Later" into v1

- Agent plan mode: Claude plans, you approve or edit, then it builds.
- GitHub issues as cards, with PR review and CI status synced back.
- Scheduled runs: work agent-ready tasks inside a time window.
- Now-playing media in the live slot (Windows media sessions).
- Voice quick-add (Windows speech recognition, offline).

## Not in v1

- Machine-to-machine sync through a private git repo.
- Reserve-space app bar mode.
- macOS.

## Platform findings (M6)

Tested on Windows 11 from an unpackaged build, which is how Pill ships.

| Area | Finding | Decision |
| --- | --- | --- |
| Voice | WinRT `SpeechRecognizer` with the dictation constraint compiles, but `RecognizeAsync` fails with `0x80045509` ("speech privacy policy was not accepted") unless Online speech recognition is on, which sends audio to Microsoft. | Dictate with the SAPI 5 desktop recognizer (`SpInprocRecognizer`), which runs on the device. It needs a speech pack with desktop recognition (English, French, German, Spanish, Japanese, Chinese). |
| Notifications | `UserNotificationListener` works without package identity: access status, the access request and reading Action Center all succeed. Subscribing to `NotificationChanged` fails with `0x80070490`. | Poll Action Center every four seconds while the plugin is on. |
| Now playing | `GlobalSystemMediaTransportControlsSessionManager` and its change events work unpackaged. Chromium tabs sometimes skip playback events. | Use events, plus a three-second poll that only republishes on change. |
