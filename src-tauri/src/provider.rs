//! AI coding CLIs Pill can drive, behind one interface.
//!
//! Every provider runs through the vendor's own CLI so the user's existing
//! login (Claude Pro/Max, ChatGPT plan) is used and Pill stores no keys.
//! See `docs/PROVIDERS.md` for the exact command lines and event formats.
//!
//! - **Claude Code**: `claude -p --output-format stream-json --verbose …`,
//!   resumed with `--resume <session>`.
//! - **Codex**: `codex exec --experimental-json …`, resumed with
//!   `resume <thread>`. Each run and chat gets its own `CODEX_HOME` (with a
//!   copy of the login) under `%LOCALAPPDATA%\Pill\codex`, outside the
//!   repository, because concurrent Codex processes sharing one session
//!   store silently do nothing, and a resume must reuse the same home.

use crate::agent::{allowed_tools, Check, DENIED_TOOLS};
use crate::process;
use crate::stream::{self, Finish, Parsed, RunEvent};
use crate::workspace::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Which CLI does the work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Claude,
    Codex,
}

impl Provider {
    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    /// Short lowercase name for branch and folder names.
    pub fn slug(self) -> &'static str {
        self.exe_name()
    }

    fn exe_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Resolved executable, if installed.
    pub fn exe(self) -> Option<PathBuf> {
        process::which(self.exe_name())
    }
}

/// What the CLI is allowed to do in this invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Implement a task in a worktree.
    Build,
    /// Explore and produce a plan; no edits.
    Plan,
    /// Answer questions in the repo; no edits.
    ChatRead,
    /// Converse and edit in the chat's worktree.
    ChatEdit,
}

/// Everything needed to build a command line.
pub struct Spec<'a> {
    /// Project being worked on; `None` for the cross-project assistant,
    /// which only ever runs read-only.
    pub project: Option<&'a Project>,
    /// Working directory (repo or worktree).
    pub cwd: &'a Path,
    /// Extra directories the CLI may use: the run folder for `pr.md`, or the
    /// project roots the assistant reads.
    pub extra_dirs: Vec<&'a Path>,
    /// Session (Claude) or thread (Codex) to resume.
    pub resume: Option<&'a str>,
    pub max_turns: u32,
    pub phase: Phase,
    /// Private state folder for the run or chat, outside the repository
    /// ([`private_state_dir`]): Codex keeps its `CODEX_HOME` here.
    pub state_dir: &'a Path,
    /// Appended to the system prompt (Claude) or prepended to the prompt (Codex).
    pub system: Option<&'a str>,
    /// Resolved model; `None` lets the CLI pick.
    pub model: Option<&'a str>,
    /// Resolved reasoning effort; `None` lets the CLI pick.
    pub effort: Option<&'a str>,
    /// Stream partial text so a chat reply appears as it's written (Claude).
    pub partial: bool,
    /// Tools outside the allowed set ask the user through
    /// [`crate::approvals`]. When false, they are denied without asking.
    pub host_prompts: bool,
    /// The task's text came from someone other than the user (an issue or a
    /// plugin): nothing that runs code is pre-approved.
    pub untrusted: bool,
}

/// Builds the command for `provider`. The prompt is written to stdin by the caller.
pub fn command(provider: Provider, spec: &Spec) -> Result<Command, String> {
    let exe = provider.exe().ok_or_else(|| format!("{} isn't installed", provider.label()))?;
    let mut cmd = process::tool(&exe);
    cmd.current_dir(spec.cwd)
        // SECURITY: an API key in the environment would bill that key instead
        // of the user's subscription login.
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        // Pill may itself be launched from a Claude Code session.
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT");
    match provider {
        Provider::Claude => claude_args(&mut cmd, spec)?,
        Provider::Codex => codex_args(&mut cmd, spec)?,
    }
    Ok(cmd)
}

/// Read-only tool set for Claude in plan and read-only chat phases.
const CLAUDE_READ_TOOLS: &[&str] = &[
    "Read", "Glob", "Grep", "LS", "TodoWrite", "Bash(git log:*)", "Bash(git show:*)", "Bash(git diff:*)",
    "Bash(git status:*)", "Bash(git blame:*)", "Bash(ls:*)", "Bash(dir:*)",
];

/// Tools Claude may never use in read-only phases.
const CLAUDE_WRITE_TOOLS: &[&str] = &[
    "Edit", "Write", "MultiEdit", "NotebookEdit", "Bash(git push:*)", "Bash(git commit:*)",
    "Bash(git checkout:*)", "Bash(git reset:*)", "Bash(rm:*)", "Bash(del:*)", "Bash(gh:*)",
];

fn claude_args(cmd: &mut Command, spec: &Spec) -> Result<(), String> {
    // stream-json input keeps stdin open so Pill can answer permission
    // prompts; see approvals::Conversation.
    cmd.args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
        .args(["--permission-prompts", if spec.host_prompts { "host" } else { "none" }]);
    if spec.host_prompts {
        cmd.args(["--permission-prompt-tool", "stdio"]);
    }
    cmd
        .args(["--max-turns", &spec.max_turns.to_string()]);
    for dir in &spec.extra_dirs {
        cmd.arg("--add-dir").arg(dir);
    }
    if let Some(system) = spec.system {
        // COMPAT: the text has line breaks, which can't be passed as an
        // argument to npm's `claude.cmd` shim; a file works for every install.
        std::fs::create_dir_all(spec.state_dir).map_err(|e| e.to_string())?;
        let file = spec.state_dir.join("system-prompt.md");
        std::fs::write(&file, system).map_err(|e| format!("writing the system prompt: {e}"))?;
        cmd.arg("--append-system-prompt-file").arg(file);
    }
    if let Some(model) = spec.model {
        cmd.args(["--model", model]);
    }
    if let Some(effort) = spec.effort {
        cmd.args(["--effort", effort]);
    }
    if spec.partial {
        cmd.arg("--include-partial-messages");
    }
    match spec.phase {
        Phase::Build | Phase::ChatEdit => {
            cmd.args(["--permission-mode", "acceptEdits", "--allowedTools"])
                .args(spec.project.map(|p| allowed_tools(p, spec.untrusted)).unwrap_or_default())
                .arg("--disallowedTools")
                .args(DENIED_TOOLS);
        }
        Phase::Plan => {
            cmd.args(["--permission-mode", "plan", "--allowedTools"]).args(CLAUDE_READ_TOOLS);
        }
        Phase::ChatRead => {
            cmd.args(["--permission-mode", "default", "--allowedTools"])
                .args(CLAUDE_READ_TOOLS)
                .arg("--disallowedTools")
                .args(CLAUDE_WRITE_TOOLS);
        }
    }
    if let Some(session) = spec.resume {
        cmd.args(["--resume", session]);
    }
    Ok(())
}

/// Where `codex login` stores the ChatGPT login.
fn codex_auth_file() -> Option<PathBuf> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".codex")))?;
    Some(home.join("auth.json")).filter(|p| p.is_file())
}

/// Pill's private state for one run or chat, outside every repository:
/// `%LOCALAPPDATA%\Pill\codex\<id>`. Codex keeps its per-run home (with a
/// copy of the login) here, and chats their system prompt file.
///
/// SECURITY: this used to live in the repository's `.pill` folder, where a
/// copied `auth.json` was one `git add -f` or backup away from leaking.
pub fn private_state_dir(id: &str) -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Pill").join("codex").join(id)
}

/// Deletes the private state of a run or chat that is gone for good.
pub fn remove_private_state(id: &str) {
    if !crate::model::is_valid_id(id) {
        return;
    }
    let dir = private_state_dir(id);
    // NOTE: `rules` may be a junction to the user's own Codex rules; remove
    // the link first so nothing could ever recurse into the real folder.
    let _ = std::fs::remove_dir(dir.join(CODEX_HOME_DIR).join("rules"));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("deleting {}: {e}", dir.display()),
    }
}

/// Moves a Codex home that an older Pill kept in `legacy_state_dir` (inside
/// the repository) to [`private_state_dir`], so a resumed thread is still
/// found. When it can't be moved (another drive), it is deleted, since the
/// login copy must not stay in the repository.
pub fn adopt_legacy_codex_home(legacy_state_dir: &Path, id: &str) {
    let old = legacy_state_dir.join(CODEX_HOME_DIR);
    if !old.is_dir() || !crate::model::is_valid_id(id) {
        return;
    }
    let new = private_state_dir(id).join(CODEX_HOME_DIR);
    if !new.exists() && new.parent().is_some_and(|p| std::fs::create_dir_all(p).is_ok()) && std::fs::rename(&old, &new).is_ok() {
        return;
    }
    let _ = std::fs::remove_dir(old.join("rules"));
    if let Err(e) = std::fs::remove_dir_all(&old) {
        log::warn!("deleting the old Codex home {}: {e}", old.display());
    }
}

/// Folder inside a state dir that Codex gets as its `CODEX_HOME`.
const CODEX_HOME_DIR: &str = "codex-home";

/// `s` as a TOML basic string, for `--config key=value` overrides.
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The `writable_roots` override for Codex's workspace-write sandbox.
fn writable_roots(dirs: &[&Path]) -> String {
    let roots: Vec<String> = dirs.iter().map(|d| toml_string(&d.to_string_lossy())).collect();
    format!("sandbox_workspace_write.writable_roots=[{}]", roots.join(","))
}

fn codex_args(cmd: &mut Command, spec: &Spec) -> Result<(), String> {
    // A private CODEX_HOME per run: concurrent runs sharing one session store
    // silently do nothing. The login is copied (not linked: symlinks need
    // admin rights on Windows) and refreshed before every invocation.
    let home = spec.state_dir.join(CODEX_HOME_DIR);
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let auth = codex_auth_file().ok_or("Codex isn't logged in. Run `codex login` in a terminal.")?;
    std::fs::copy(&auth, home.join("auth.json")).map_err(|e| format!("copying Codex login: {e}"))?;

    let sandbox = match spec.phase {
        Phase::Build | Phase::ChatEdit => "workspace-write",
        Phase::Plan | Phase::ChatRead => "read-only",
    };
    if spec.host_prompts {
        link_codex_rules(&home);
        // The app-server speaks JSON-RPC on stdio, which lets Pill answer its
        // approval requests (see approvals::Conversation). Sandbox, model and
        // limits come from config; the thread and turn are started over the
        // protocol.
        // SECURITY: `untrusted` asks before any command Codex doesn't know
        // to be read-only, the counterpart of Claude's tighter allowlist.
        let approval = if spec.untrusted { "untrusted" } else { "on-request" };
        cmd.env("CODEX_HOME", &home)
            .arg("app-server")
            .arg("--config")
            .arg(format!("sandbox_mode=\"{sandbox}\""))
            .arg("--config")
            .arg(format!("approval_policy=\"{approval}\""))
            .args(["--config", "sandbox_workspace_write.network_access=false"])
            .args(["--config", "web_search=\"disabled\""]);
        if let Some(model) = spec.model {
            cmd.arg("--config").arg(format!("model={}", toml_string(model)));
        }
        if let Some(effort) = spec.effort {
            cmd.arg("--config").arg(format!("model_reasoning_effort={}", toml_string(effort)));
        }
        if !spec.extra_dirs.is_empty() {
            cmd.arg("--config").arg(writable_roots(&spec.extra_dirs));
        }
        return Ok(());
    }
    cmd.env("CODEX_HOME", &home)
        .args(["exec", "--experimental-json", "--sandbox", sandbox, "--skip-git-repo-check"])
        .arg("--cd")
        .arg(spec.cwd)
        .args(["--config", "approval_policy=\"never\""])
        .args(["--config", "sandbox_workspace_write.network_access=false"])
        .args(["--config", "web_search=\"disabled\""]);
    if let Some(model) = spec.model {
        cmd.args(["--model", model]);
    }
    if let Some(effort) = spec.effort {
        cmd.arg("--config").arg(format!("model_reasoning_effort={}", toml_string(effort)));
    }
    for dir in &spec.extra_dirs {
        cmd.arg("--add-dir").arg(dir);
    }
    if let Some(thread) = spec.resume {
        cmd.args(["resume", thread]);
    }
    Ok(())
}

/// Points a private `CODEX_HOME`'s `rules` folder at the user's own, so
/// "Always allow" rules Codex saves during a run outlive it and apply in the
/// Codex CLI too. Best effort: without the link, rules last for the run only.
fn link_codex_rules(home: &Path) {
    let link = home.join("rules");
    let Some(user_home) = std::env::var_os("CODEX_HOME").map(PathBuf::from).or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".codex"))) else {
        return;
    };
    let target = user_home.join("rules");
    if link.exists() || std::fs::create_dir_all(&target).is_err() {
        return;
    }
    // NOTE: a directory junction, unlike a symlink, needs no admin rights or
    // developer mode on Windows.
    #[cfg(windows)]
    {
        let mut cmd = process::command("cmd");
        cmd.args(["/C", "mklink", "/J"]).arg(&link).arg(&target);
        if let Err(e) = process::run(cmd) {
            log::warn!("linking Codex rules: {e}");
        }
    }
}

/// Text that prefixes a Codex prompt, since Codex has no system-prompt flag.
pub fn with_system(provider: Provider, system: Option<&str>, prompt: &str) -> String {
    match (provider, system) {
        (Provider::Codex, Some(s)) => format!("{s}\n\n---\n\n{prompt}"),
        _ => prompt.to_string(),
    }
}

/// Stateful stream parser for either provider.
pub struct StreamParser {
    provider: Provider,
    cwd: String,
    /// Codex: the latest agent message, which is the final answer.
    last_message: String,
}

impl StreamParser {
    /// Creates a parser; `cwd` shortens file paths for display.
    pub fn new(provider: Provider, cwd: &Path) -> Self {
        Self { provider, cwd: cwd.to_string_lossy().into_owned(), last_message: String::new() }
    }

    /// Parses one output line.
    pub fn feed(&mut self, line: &str) -> Vec<Parsed> {
        match self.provider {
            Provider::Claude => stream::parse_line(line, &self.cwd),
            Provider::Codex => self.codex(line),
        }
    }

    fn codex(&mut self, line: &str) -> Vec<Parsed> {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        let mut out = Vec::new();
        match v["type"].as_str() {
            Some("thread.started") => {
                if let Some(id) = v["thread_id"].as_str() {
                    out.push(Parsed::Session(id.to_string()));
                }
            }
            Some(kind @ ("item.started" | "item.completed")) => {
                let item = &v["item"];
                let done = kind == "item.completed";
                match item["type"].as_str() {
                    Some("agent_message") if done => {
                        let text = item["text"].as_str().unwrap_or_default().trim().to_string();
                        if !text.is_empty() {
                            self.last_message = text.clone();
                            out.push(Parsed::Event(RunEvent::Text { text: crate::stream::message(&text) }));
                        }
                    }
                    Some("command_execution") if !done => {
                        let command = item["command"].as_str().unwrap_or_default();
                        out.push(Parsed::Event(RunEvent::Tool { name: "Shell".into(), detail: clip(command, 160) }));
                    }
                    Some("file_change") if done => {
                        for change in item["changes"].as_array().into_iter().flatten() {
                            let path = change["path"].as_str().unwrap_or_default();
                            let shown = rel(path, &self.cwd);
                            out.push(Parsed::Event(RunEvent::Tool { name: "Edit".into(), detail: shown.clone() }));
                            if crate::stream::is_inside(path, &self.cwd) {
                                out.push(Parsed::FileTouched(shown));
                            }
                        }
                    }
                    Some("error") => {
                        let msg = item["message"].as_str().unwrap_or("Codex error");
                        out.push(Parsed::Event(RunEvent::Output { text: msg.to_string() }));
                    }
                    _ => {}
                }
            }
            Some("turn.completed") => out.push(Parsed::Finished(Finish {
                ok: true,
                subtype: "success".into(),
                text: self.last_message.clone(),
                turns: None,
                cost_usd: None,
            })),
            Some("turn.failed") => {
                let message = v["error"]["message"].as_str().unwrap_or("Codex turn failed");
                if let Some(limit) = crate::limits::detect(Provider::Codex, message) {
                    out.push(Parsed::UsageLimit(limit));
                }
                out.push(Parsed::Finished(Finish {
                    ok: false,
                    subtype: "error".into(),
                    text: explain_failure(Provider::Codex, message),
                    turns: None,
                    cost_usd: None,
                }));
            }
            Some("error") => {
                let message = v["message"].as_str().unwrap_or("Codex error");
                if let Some(limit) = crate::limits::detect(Provider::Codex, message) {
                    out.push(Parsed::UsageLimit(limit));
                }
                out.push(Parsed::Event(RunEvent::Output { text: explain_failure(Provider::Codex, message) }));
            }
            _ => {}
        }
        out
    }
}

/// A CLI's failure message as shown to the user: usage limits become one
/// sentence with the reset time ([`crate::limits`]); anything else is kept.
pub fn explain_failure(provider: Provider, message: &str) -> String {
    match crate::limits::detect(provider, message) {
        Some(limit) => limit.describe(),
        None => message.trim().to_string(),
    }
}

fn clip(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max { one } else { format!("{}…", one.chars().take(max - 1).collect::<String>()) }
}

fn rel(path: &str, root: &str) -> String {
    let p = path.replace('/', "\\");
    let r = root.replace('/', "\\");
    p.strip_prefix(&r).map(|s| s.trim_start_matches('\\').to_string()).unwrap_or(p)
}

/// Install and login checks for a provider.
pub fn checks(provider: Provider) -> Vec<Check> {
    match provider {
        Provider::Claude => crate::agent::claude_checks(),
        Provider::Codex => {
            let exe = provider.exe();
            let mut v = vec![Check {
                name: "Codex".into(),
                ok: exe.is_some(),
                message: match &exe {
                    Some(path) => {
                        let mut cmd = process::tool(path);
                        cmd.arg("--version");
                        format!("Installed · {}", process::run(cmd).unwrap_or_default())
                    }
                    None => "Codex isn't installed. Install it with `npm install -g @openai/codex`, then run `codex login`.".into(),
                },
            }];
            if exe.is_some() {
                let logged_in = codex_auth_file().is_some();
                v.push(Check {
                    name: "Codex login".into(),
                    ok: logged_in,
                    message: if logged_in {
                        "Logged in".into()
                    } else {
                        "Not logged in. Run `codex login` in a terminal and sign in with ChatGPT.".into()
                    },
                });
            }
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WT: &str = r"C:\dev\wt";

    fn feed(lines: &[&str]) -> Vec<Parsed> {
        let mut p = StreamParser::new(Provider::Codex, Path::new(WT));
        lines.iter().flat_map(|l| p.feed(l)).collect()
    }

    #[test]
    fn codex_thread_commands_files_and_final_answer() {
        let out = feed(&[
            r#"{"type":"thread.started","thread_id":"th_1"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.started","item":{"id":"1","type":"command_execution","command":"pnpm test","status":"in_progress"}}"#,
            r#"{"type":"item.completed","item":{"id":"2","type":"file_change","changes":[{"path":"C:\\dev\\wt\\src\\a.ts","kind":"update"}]}}"#,
            r#"{"type":"item.completed","item":{"id":"3","type":"agent_message","text":"Fixed it."}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}"#,
        ]);
        assert_eq!(out[0], Parsed::Session("th_1".into()));
        assert!(out.contains(&Parsed::Event(RunEvent::Tool { name: "Shell".into(), detail: "pnpm test".into() })));
        assert!(out.contains(&Parsed::FileTouched(r"src\a.ts".into())));
        assert!(matches!(out.last(), Some(Parsed::Finished(f)) if f.ok && f.text == "Fixed it."));
    }

    #[test]
    fn codex_failures_and_quota() {
        let out = feed(&[r#"{"type":"turn.failed","error":{"message":"You've hit your usage limit. Try again in 3 hours."}}"#]);
        assert!(matches!(&out[0], Parsed::UsageLimit(l) if l.provider == Provider::Codex && l.resets_at.is_some()));
        assert!(matches!(&out[1], Parsed::Finished(f) if !f.ok && f.text.starts_with("Codex usage limit reached, resets at")));
        assert_eq!(explain_failure(Provider::Codex, " boom\n"), "boom");
    }

    #[test]
    fn read_only_phases_never_allow_writes() {
        for t in ["Edit", "Write", "MultiEdit"] {
            assert!(!CLAUDE_READ_TOOLS.contains(&t));
            assert!(CLAUDE_WRITE_TOOLS.contains(&t));
        }
    }

    #[test]
    fn codex_config_strings_survive_quotes_and_backslashes() {
        assert_eq!(toml_string(r"C:\Users\o'brien"), r#""C:\\Users\\o'brien""#);
        assert_eq!(toml_string(r#"a"b"#), r#""a\"b""#);
        assert_eq!(toml_string("tab\there"), r#""tab\u0009here""#);
        let a = Path::new(r"C:\Users\o'brien\app\.pill\runs\r_1");
        let b = Path::new(r#"D:\we"ird"#);
        assert_eq!(
            writable_roots(&[a, b]),
            r#"sandbox_workspace_write.writable_roots=["C:\\Users\\o'brien\\app\\.pill\\runs\\r_1","D:\\we\"ird"]"#
        );
    }

    #[test]
    fn private_state_lives_outside_the_repository_and_only_for_plain_ids() {
        let dir = private_state_dir("r_1");
        assert!(dir.ends_with(r"Pill\codex\r_1"));
        assert!(!dir.components().any(|c| c.as_os_str() == ".pill"));
        // A traversal id never deletes anything.
        remove_private_state(r"..\..");
        remove_private_state("");
    }

    #[test]
    fn legacy_codex_homes_leave_the_repository() {
        let root = std::env::temp_dir().join(format!("pill-legacy-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let legacy = root.join(".pill").join("runs").join("r_legacy");
        std::fs::create_dir_all(legacy.join(CODEX_HOME_DIR).join("sessions")).unwrap();
        std::fs::write(legacy.join(CODEX_HOME_DIR).join("auth.json"), "{}").unwrap();
        let id = format!("r_legacy_test_{}", std::process::id());
        adopt_legacy_codex_home(&legacy, &id);
        assert!(!legacy.join(CODEX_HOME_DIR).exists());
        assert!(private_state_dir(&id).join(CODEX_HOME_DIR).join("sessions").is_dir());
        remove_private_state(&id);
        assert!(!private_state_dir(&id).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn codex_gets_the_system_text_in_the_prompt() {
        assert_eq!(with_system(Provider::Codex, Some("SYS"), "hi"), "SYS\n\n---\n\nhi");
        assert_eq!(with_system(Provider::Claude, Some("SYS"), "hi"), "hi");
    }
}
