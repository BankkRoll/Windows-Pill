//! Per-project conversations with Claude Code.
//!
//! Each chat is a real Claude Code session (the user's own login) resumed
//! with `--resume` on every message, so context carries across turns.
//!
//! - Read mode runs in the project folder with read-only tools: Claude can
//!   explore, explain and plan, but can't change anything.
//! - Edit mode gets its own worktree on `chat/<id>` (never the user's
//!   checkout) with the same tool policy as agent runs, and can be shipped
//!   as a draft PR.
//!
//! Claude can propose tasks by ending a reply with a fenced `pill-tasks`
//! JSON block; the UI turns those into tasks (and optionally agent runs).
//! Chats are stored in `<repo>\.pill\chats\<id>.json` (gitignored).

use crate::approvals::{Approvals, Conversation, ToolAsk, Turn};
use crate::config::write_atomic;
use crate::model::is_valid_id;
use crate::process::{self, git};
use crate::state::AppState;
use crate::provider::{self, Phase, Provider, StreamParser};
use crate::stream::{Parsed, RunEvent};
use crate::workspace::{new_id, now_rfc3339, pill_dir, Project};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

/// Event carrying a [`ChatUpdate`] while a reply streams in.
pub const CHAT_EVENT: &str = "pill://chat";

/// What Claude may do in a chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatMode {
    /// Explore and answer only.
    Read,
    /// Change code in the chat's own worktree.
    Edit,
}

/// A task Claude suggested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedTask {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub priority: Option<u8>,
}

/// One message in a chat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    /// `user` or `assistant`.
    pub role: String,
    pub text: String,
    pub at: String,
    /// Tool calls made while producing this reply, e.g. `Read src/main.ts`.
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub proposals: Vec<ProposedTask>,
    /// Set when the reply failed.
    #[serde(default)]
    pub error: Option<String>,
    /// Model that wrote this reply, as the CLI reported it.
    #[serde(default)]
    pub model: Option<String>,
    /// CLI that wrote this reply.
    #[serde(default)]
    pub provider: Option<Provider>,
}

/// A provider's session within a chat and how much of the chat it has seen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMark {
    pub provider: Provider,
    pub id: String,
    /// Number of chat messages that existed when this provider last replied.
    pub seen: usize,
}

/// Messages the next provider hasn't seen, formatted as context for its prompt.
/// `resumed` says whether it saw the start of the chat.
fn catch_up_block(messages: &[ChatMessage], resumed: bool) -> String {
    // Keep the newest part when a long exchange happened elsewhere.
    const LIMIT: usize = 24_000;
    let mut lines: Vec<String> = Vec::new();
    for m in messages {
        let who = match (m.role.as_str(), m.provider) {
            ("user", _) => "User".to_string(),
            (_, Some(p)) => format!("Assistant ({})", p.label()),
            _ => "Assistant".to_string(),
        };
        if !m.text.trim().is_empty() {
            lines.push(format!("{who}: {}", m.text.trim()));
        }
    }
    let mut body = lines.join("\n\n");
    if body.len() > LIMIT {
        let cut = body.len() - LIMIT;
        let start = (cut..body.len()).find(|i| body.is_char_boundary(*i)).unwrap_or(body.len());
        body = format!("[earlier messages trimmed]\n{}", &body[start..]);
    }
    let intro = if resumed {
        "While you were away, the conversation continued with another assistant:"
    } else {
        "You're joining a conversation that started with another assistant. Here it is so far:"
    };
    format!("{intro}\n\n<conversation>\n{body}\n</conversation>\n\nThe user's new message:\n\n")
}

/// A conversation, as stored on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: ChatMode,
    /// CLI that answered last; the next reply uses `choice.provider` or this.
    #[serde(default)]
    pub provider: Provider,
    /// Legacy single session from before per-provider sessions.
    #[serde(default)]
    pub session_id: Option<String>,
    /// One session per provider that has answered, so switching back resumes
    /// its own memory.
    #[serde(default)]
    pub sessions: Vec<SessionMark>,
    /// Edit mode's worktree and branch.
    #[serde(default)]
    pub worktree: Option<PathBuf>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub pr_url: Option<String>,
    /// Ref the edit worktree started from, compared against when shipping.
    #[serde(default)]
    pub base_ref: Option<String>,
    /// Provider, model and effort picked for this chat; empty values use the project's.
    #[serde(default)]
    pub choice: crate::models::RunChoice,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<ChatMessage>,
    /// True while a reply is being generated (not persisted as true across restarts).
    #[serde(default)]
    pub busy: bool,
}

/// Incremental progress on a reply.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatUpdate {
    pub chat_id: String,
    /// The assistant message being built.
    pub message: ChatMessage,
    pub busy: bool,
}

/// System prompt addition that teaches Claude how to hand Pill tasks.
pub(crate) const TASK_PROTOCOL: &str = r#"You are chatting with a developer inside Pill, their task manager, about this repository.

Task protocol (required): whenever your reply proposes, suggests or recommends any concrete work — bugs to fix, improvements, a feature broken into steps — you MUST end the reply with a fenced code block tagged pill-tasks holding a JSON array, one object per independent change. Pill turns each object into a task that an autonomous agent implements as one pull request, so every task must be self-contained. Write the prose answer first, then the block. Example:

```pill-tasks
[{"title": "Fix null check in parseConfig", "notes": "src/config.ts:42 dereferences config.env before checking it. Guard it and add a unit test for a missing env.", "priority": 1}]
```

Fields: title (short, imperative), notes (what to change, where, and how to verify), priority (1 = high, 2, 3 = low). Omit the block only when you propose no work."#;

/// Extracts proposed tasks from a ```pill-tasks block, returning the reply
/// text without the block.
pub fn extract_proposals(text: &str) -> (String, Vec<ProposedTask>) {
    let Some(start) = text.find("```pill-tasks") else { return (text.to_string(), Vec::new()) };
    let body_start = start + "```pill-tasks".len();
    let Some(len) = text[body_start..].find("```") else { return (text.to_string(), Vec::new()) };
    let json = text[body_start..body_start + len].trim();
    let tasks: Vec<ProposedTask> = serde_json::from_str(json).unwrap_or_default();
    let tasks = tasks
        .into_iter()
        .filter(|t| !t.title.trim().is_empty())
        .map(|mut t| {
            t.priority = t.priority.filter(|p| (1..=3).contains(p));
            t
        })
        .collect();
    let rest = format!("{}{}", &text[..start], &text[body_start + len + 3..]);
    (rest.trim().to_string(), tasks)
}

fn chats_dir(project: &Path) -> PathBuf {
    pill_dir(project).join("chats")
}

/// Moves the Codex home an older Pill kept in `.pill\chats\<id>.state` out
/// of the repository and removes what's left of that folder.
fn retire_legacy_state(project: &Path, chat_id: &str) {
    let legacy = chats_dir(project).join(format!("{chat_id}.state"));
    if legacy.is_dir() {
        provider::adopt_legacy_codex_home(&legacy, chat_id);
        let _ = fs::remove_file(legacy.join("system-prompt.md"));
        let _ = fs::remove_dir(&legacy);
    }
}

/// The branch an edit-mode chat works on.
fn chat_branch(chat_id: &str) -> String {
    format!("chat/{chat_id}")
}

/// A chat read from `<file_id>.json`, with a worktree or branch that isn't
/// the chat's own dropped. `None` when the file holds another chat.
///
/// SECURITY: chat files can be committed to a repository. A hand-made one
/// must not point edit mode at a folder outside `.pill-worktrees`, or make
/// deleting the chat force-delete a branch such as `main`.
fn checked(project: &Path, file_id: &str, mut chat: Chat) -> Option<Chat> {
    if chat.id != file_id {
        log::warn!("skipping chat file {file_id}.json: it holds chat {:?}", chat.id);
        return None;
    }
    if chat.worktree.as_ref().is_some_and(|w| !crate::worktree::is_pill_worktree(w, project) || w.file_name() != Some(std::ffi::OsStr::new(&chat.id))) {
        log::warn!("chat {}: ignoring a worktree that isn't its own", chat.id);
        chat.worktree = None;
    }
    if chat.branch.as_ref().is_some_and(|b| *b != chat_branch(&chat.id)) {
        log::warn!("chat {}: ignoring a branch that isn't its own", chat.id);
        chat.branch = None;
    }
    Some(chat)
}

fn title_from(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("New chat").trim();
    if line.chars().count() > 60 { format!("{}…", line.chars().take(59).collect::<String>()) } else { line.to_string() }
}

struct Active {
    cancel: Arc<AtomicBool>,
    #[cfg(windows)]
    job: Option<Arc<process::job::Job>>,
}

/// All chats, with the processes of replies in flight.
#[derive(Default)]
pub struct Chats {
    active: Mutex<HashMap<String, Active>>,
    /// Serializes writes per chat file.
    io: Mutex<()>,
}

impl Chats {
    fn path(project: &Path, chat_id: &str) -> PathBuf {
        chats_dir(project).join(format!("{chat_id}.json"))
    }

    fn load(project: &Path, chat_id: &str) -> Result<Chat, String> {
        if !is_valid_id(chat_id) {
            return Err(format!("{chat_id:?} isn't a chat id"));
        }
        let text = fs::read_to_string(Self::path(project, chat_id)).map_err(|e| format!("chat {chat_id}: {e}"))?;
        let chat = serde_json::from_str(&text).map_err(|e| format!("chat {chat_id}: {e}"))?;
        checked(project, chat_id, chat).ok_or_else(|| format!("chat {chat_id}: the file holds another chat"))
    }

    fn save(&self, project: &Path, chat: &Chat) -> Result<(), String> {
        // A reply can finish after its repository was deleted or moved; saving
        // would recreate the folder just to hold the chat.
        if !project.is_dir() {
            return Err(crate::workspace::PROJECT_GONE.into());
        }
        let _guard = self.io.lock().unwrap();
        let bytes = serde_json::to_vec_pretty(chat).map_err(|e| e.to_string())?;
        write_atomic(&Self::path(project, &chat.id), &bytes).map_err(|e| e.to_string())
    }

    /// Chats for a project, most recently updated first.
    pub fn list(&self, project: &Project) -> Vec<Chat> {
        let busy: Vec<String> = self.active.lock().unwrap().keys().cloned().collect();
        let mut chats: Vec<Chat> = fs::read_dir(chats_dir(&project.path))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().strip_suffix(".json").filter(|id| is_valid_id(id))?.to_string();
                let chat = serde_json::from_str::<Chat>(&fs::read_to_string(e.path()).ok()?).ok()?;
                checked(&project.path, &name, chat)
            })
            .map(|mut c| {
                c.busy = busy.contains(&c.id);
                c
            })
            .collect();
        chats.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        chats
    }

    /// Creates an empty chat.
    pub fn create(&self, project: &Project, mode: ChatMode) -> Result<Chat, String> {
        let now = now_rfc3339();
        let chat = Chat {
            id: new_id("c"),
            project_id: project.id.clone(),
            title: "New chat".into(),
            mode,
            provider: project.settings.provider,
            session_id: None,
            sessions: Vec::new(),
            worktree: None,
            branch: None,
            pr_url: None,
            base_ref: None,
            choice: Default::default(),
            created_at: now.clone(),
            updated_at: now,
            messages: Vec::new(),
            busy: false,
        };
        self.save(&project.path, &chat)?;
        crate::workspace::ensure_gitignore(&project.path, project.settings.commit_tasks).map_err(|e| e.to_string())?;
        Ok(chat)
    }

    /// Deletes a chat and its worktree.
    pub fn delete(&self, project: &Project, chat_id: &str) -> Result<(), String> {
        self.stop(chat_id);
        let chat = Self::load(&project.path, chat_id)?;
        if let Some(wt) = &chat.worktree {
            crate::worktree::remove(&project.path, wt)?;
        }
        if chat.pr_url.is_none() {
            if let Some(b) = &chat.branch {
                let _ = git(&project.path, &["branch", "-D", "--end-of-options", b]);
            }
        }
        fs::remove_file(Self::path(&project.path, chat_id)).map_err(|e| e.to_string())?;
        retire_legacy_state(&project.path, chat_id);
        provider::remove_private_state(chat_id);
        Ok(())
    }

    /// Stops a reply in progress.
    pub fn stop(&self, chat_id: &str) {
        if let Some(a) = self.active.lock().unwrap().get(chat_id) {
            a.cancel.store(true, Ordering::SeqCst);
            #[cfg(windows)]
            if let Some(job) = &a.job {
                job.kill();
            }
        }
    }

    /// Switches a chat between read-only and editing.
    ///
    /// The first switch to editing moves the chat into a new worktree, where
    /// the CLIs can't resume sessions started in the project folder, so those
    /// sessions are dropped and the next reply is handed the transcript.
    pub fn set_mode(&self, project: &Project, chat_id: &str, mode: ChatMode) -> Result<Chat, String> {
        if self.active.lock().unwrap().contains_key(chat_id) {
            return Err("Wait for the reply to finish first".into());
        }
        let mut chat = Self::load(&project.path, chat_id)?;
        if chat.mode == mode {
            return Ok(chat);
        }
        let moving = mode == ChatMode::Edit && !chat.worktree.as_ref().is_some_and(|w| w.is_dir());
        if moving {
            chat.sessions.clear();
            chat.session_id = None;
        }
        chat.mode = mode;
        chat.updated_at = now_rfc3339();
        self.save(&project.path, &chat)?;
        Ok(chat)
    }

    /// Sends a message and streams Claude's reply on a background thread.
    pub fn send(
        self: &Arc<Self>,
        app: &AppHandle,
        project: Project,
        chat_id: &str,
        text: String,
        choice: Option<crate::models::RunChoice>,
    ) -> Result<Chat, String> {
        if text.trim().is_empty() {
            return Err("Type a message first".into());
        }
        if self.active.lock().unwrap().contains_key(chat_id) {
            return Err("Still answering the previous message".into());
        }
        let mut chat = Self::load(&project.path, chat_id)?;
        if let Some(c) = choice {
            if !c.is_valid() {
                return Err("That model or effort name isn't valid".into());
            }
            chat.choice = c;
        }
        if chat.messages.is_empty() {
            chat.title = title_from(&text);
        }
        chat.messages.push(ChatMessage {
            id: new_id("m"),
            role: "user".into(),
            text: text.clone(),
            at: now_rfc3339(),
            tools: Vec::new(),
            proposals: Vec::new(),
            error: None,
            model: None,
            provider: None,
        });
        chat.updated_at = now_rfc3339();
        chat.busy = true;
        self.save(&project.path, &chat)?;

        let cancel = Arc::new(AtomicBool::new(false));
        self.active.lock().unwrap().insert(
            chat_id.to_string(),
            Active {
                cancel: cancel.clone(),
                #[cfg(windows)]
                job: None,
            },
        );
        let (me, handle, id) = (self.clone(), app.clone(), chat_id.to_string());
        let returned = chat.clone();
        std::thread::Builder::new()
            .name(format!("chat-{id}"))
            .spawn(move || {
                let result = me.reply(&handle, &project, &id, &text, &cancel);
                if let Err(e) = result {
                    log::warn!("chat {id}: {e}");
                }
                me.active.lock().unwrap().remove(&id);
            })
            .map_err(|e| e.to_string())?;
        Ok(returned)
    }

    fn ensure_worktree(&self, project: &Project, chat: &mut Chat) -> Result<PathBuf, String> {
        if let Some(wt) = chat.worktree.clone().filter(|w| w.is_dir()) {
            return Ok(wt);
        }
        let wt = crate::worktree::path_for(&project.path, &chat.id);
        let branch = chat_branch(&chat.id);
        // The latest base branch, as for agent runs; a repository whose base
        // branch can't be found still gets a chat, from its current commit.
        let base = match crate::worktree::resolve_base(project) {
            Ok(b) => b.base_ref,
            Err(_) => git(&project.path, &["rev-parse", "HEAD"])?,
        };
        crate::worktree::add(&project.path, &branch, &wt, &base)?;
        chat.base_ref = Some(base);
        chat.worktree = Some(wt.clone());
        chat.branch = Some(branch);
        Ok(wt)
    }

    fn reply(&self, app: &AppHandle, project: &Project, chat_id: &str, text: &str, cancel: &AtomicBool) -> Result<(), String> {
        let mut chat = Self::load(&project.path, chat_id)?;
        let mut message = ChatMessage {
            id: new_id("m"),
            role: "assistant".into(),
            text: String::new(),
            at: now_rfc3339(),
            tools: Vec::new(),
            proposals: Vec::new(),
            error: None,
            model: None,
            provider: None,
        };
        let emit = |m: &ChatMessage, busy: bool| {
            let _ = app.emit(CHAT_EVENT, ChatUpdate { chat_id: chat_id.to_string(), message: m.clone(), busy });
        };
        emit(&message, true);

        let provider = chat.choice.provider.unwrap_or(chat.provider);
        message.provider = Some(provider);
        // Chats from before per-provider sessions had one session id.
        if chat.sessions.is_empty() {
            if let Some(id) = chat.session_id.clone() {
                chat.sessions.push(SessionMark { provider: chat.provider, id, seen: chat.messages.len().saturating_sub(1) });
            }
        }
        let mark = chat.sessions.iter().find(|m| m.provider == provider).cloned();
        // Everything before the new message that this provider hasn't seen.
        let earlier = &chat.messages[..chat.messages.len().saturating_sub(1)];
        let unseen = match &mark {
            Some(m) => &earlier[m.seen.min(earlier.len())..],
            None => earlier,
        };
        let prompt_text = if unseen.is_empty() { text.to_string() } else { format!("{}{text}", catch_up_block(unseen, mark.is_some())) };
        let mut new_session: Option<String> = None;

        let outcome = (|| -> Result<Option<String>, String> {
            // Once a chat has a worktree, read-only turns run there too: CLI
            // sessions are tied to their folder, and the worktree holds the
            // chat's edits that later questions are about.
            let cwd = match chat.mode {
                ChatMode::Read => chat.worktree.clone().filter(|w| w.is_dir()).unwrap_or_else(|| project.path.clone()),
                ChatMode::Edit => self.ensure_worktree(project, &mut chat)?,
            };
            retire_legacy_state(&project.path, chat_id);
            let state_dir = provider::private_state_dir(chat_id);
            let config = app.state::<crate::state::AppState>().config.read().unwrap().clone();
            let (model, effort) = crate::models::resolve(&config, project, provider, &chat.choice.model_choice());
            // Codex doesn't report its model, so show what was asked for.
            message.model = model.clone();
            let mut cmd = provider::command(
                provider,
                &provider::Spec {
                    project: Some(project),
                    cwd: &cwd,
                    extra_dirs: Vec::new(),
                    resume: mark.as_ref().map(|m| m.id.as_str()),
                    max_turns: 40,
                    phase: match chat.mode {
                        ChatMode::Read => Phase::ChatRead,
                        ChatMode::Edit => Phase::ChatEdit,
                    },
                    state_dir: &state_dir,
                    system: Some(TASK_PROTOCOL),
                    model: model.as_deref(),
                    effort: effort.as_deref(),
                    partial: true,
                    host_prompts: true,
                    untrusted: false,
                },
            )?;
            cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            let label = provider.label();
            let mut child = cmd.spawn().map_err(|e| format!("Couldn't start {label}: {e}"))?;
            #[cfg(windows)]
            {
                let job = Arc::new(process::job::Job::new().map_err(|e| e.to_string())?);
                job.assign(&child).map_err(|e| e.to_string())?;
                if let Some(a) = self.active.lock().unwrap().get_mut(chat_id) {
                    a.job = Some(job);
                }
            }
            let prompt = provider::with_system(provider, Some(TASK_PROTOCOL), &prompt_text);
            let resume = mark.as_ref().map(|m| m.id.as_str());
            let mut conversation = Conversation::start(provider, &mut child, &prompt, &Turn { cwd: &cwd, resume, host: true })?;
            let approvals = app.state::<Arc<Approvals>>().inner().clone();
            let origin = chat.title.clone();
            let mut ask = |t: ToolAsk| {
                let mut a = t.to_approval(provider, &project.id, &origin);
                a.chat_id = Some(chat_id.to_string());
                approvals.ask(app, a, &|| cancel.load(Ordering::SeqCst))
            };
            let stderr = child.stderr.take();
            let stderr_reader = std::thread::spawn(move || {
                let mut buf = String::new();
                if let Some(mut s) = stderr {
                    let _ = std::io::Read::read_to_string(&mut s, &mut buf);
                }
                buf
            });
            let mut parser = StreamParser::new(provider, &cwd);
            let mut session = None;
            let mut final_text: Option<String> = None;
            let mut failure: Option<String> = None;
            let mut limit: Option<crate::limits::UsageLimit> = None;
            let stdout = child.stdout.take().ok_or("no stdout")?;
            let mut last_emit = std::time::Instant::now();
            let lines = BufReader::new(stdout).lines().map_while(Result::ok).flat_map(|raw| conversation.on_line(&raw, &mut ask));
            for line in lines {
                // Claude streams text deltas (`--include-partial-messages`); the
                // complete answer still arrives in the final result.
                let mut delta_only = false;
                if provider == Provider::Claude {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                        if v["type"] == "stream_event" {
                            let ev = &v["event"];
                            match ev["type"].as_str() {
                                Some("content_block_start") if ev["content_block"]["type"] == "text" && !message.text.is_empty() => {
                                    message.text.push_str("\n\n");
                                }
                                Some("content_block_delta") if ev["delta"]["type"] == "text_delta" => {
                                    message.text.push_str(ev["delta"]["text"].as_str().unwrap_or_default());
                                    delta_only = true;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                for p in parser.feed(&line) {
                    match p {
                        Parsed::Session(id) => session = Some(id),
                        Parsed::Model(m) => message.model = Some(m),
                        Parsed::Event(RunEvent::Tool { name, detail }) => {
                            message.tools.push(format!("{name} {detail}").trim().to_string());
                        }
                        Parsed::Event(RunEvent::Text { text }) if provider == Provider::Codex => {
                            message.text = text;
                        }
                        Parsed::Event(RunEvent::Output { text }) => failure = Some(text),
                        Parsed::UsageLimit(l) => {
                            failure = Some(l.describe());
                            limit = Some(l);
                        }
                        Parsed::Finished(f) => {
                            if f.ok {
                                final_text = Some(f.text);
                            } else {
                                failure = Some(if f.text.is_empty() { format!("{label} stopped ({})", f.subtype) } else { f.text });
                            }
                        }
                        _ => {}
                    }
                }
                // PERF: deltas arrive every few milliseconds; ~30 updates a
                // second is smooth without flooding the webview with IPC.
                if !delta_only || last_emit.elapsed() >= std::time::Duration::from_millis(33) {
                    emit(&message, true);
                    last_emit = std::time::Instant::now();
                }
            }
            let status = child.wait().map_err(|e| e.to_string())?;
            let stderr_text = stderr_reader.join().unwrap_or_default();
            new_session = session;
            if cancel.load(Ordering::SeqCst) {
                return Err("Stopped".into());
            }
            if let Some(f) = failure {
                // A limit hit in a chat applies to agent runs on the same plan too.
                if let Some(l) = limit.or_else(|| crate::limits::detect(provider, &f)) {
                    app.state::<Arc<crate::agent::Agents>>().pause(app, &l);
                    return Err(l.describe());
                }
                return Err(f);
            }
            if final_text.is_none() && !status.success() {
                return Err(if stderr_text.trim().is_empty() {
                    format!("{label} exited ({status})")
                } else {
                    provider::explain_failure(provider, stderr_text.trim())
                });
            }
            Ok(final_text)
        })();

        match outcome {
            Ok(final_text) => {
                // Prefer the final result text: it's the complete last answer.
                let full = final_text.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| message.text.clone());
                let (clean, proposals) = extract_proposals(&full);
                message.text = clean;
                message.proposals = proposals;
            }
            Err(e) => message.error = Some(e),
        }
        chat.messages.push(message.clone());
        chat.provider = provider;
        let seen = chat.messages.len();
        match (new_session, chat.sessions.iter_mut().find(|m| m.provider == provider)) {
            (Some(id), Some(m)) => {
                m.id = id;
                m.seen = seen;
            }
            (Some(id), None) => chat.sessions.push(SessionMark { provider, id, seen }),
            // A failed reply without a session still saw nothing new.
            (None, _) => {}
        }
        chat.updated_at = now_rfc3339();
        chat.busy = false;
        self.save(&project.path, &chat)?;
        emit(&message, false);
        Ok(())
    }

    /// Pushes an edit-mode chat's branch and opens a draft PR.
    pub fn ship(&self, app: &AppHandle, project: &Project, chat_id: &str) -> Result<String, String> {
        let mut chat = Self::load(&project.path, chat_id)?;
        let (wt, branch) = match (&chat.worktree, &chat.branch) {
            (Some(w), Some(b)) if w.is_dir() => (w.clone(), b.clone()),
            _ => return Err("This chat hasn't changed any code".into()),
        };
        if !git(&wt, &["status", "--porcelain"])?.is_empty() {
            git(&wt, &["add", "-A"])?;
            git(&wt, &["commit", "-m", &chat.title])?;
        }
        let remote = project
            .remote
            .clone()
            .ok_or("This project has no git remote, so there's nowhere to push. The changes stay on the chat's local branch.")?;
        let base = chat.base_ref.clone().unwrap_or_else(|| format!("{remote}/{}", project.base_branch));
        let ahead: u32 = git(&wt, &["rev-list", "--count", "--end-of-options", &format!("{base}..HEAD")])
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if ahead == 0 {
            return Err("No commits to ship yet".into());
        }
        git(&wt, &["push", "-u", &remote, &format!("HEAD:refs/heads/{branch}")])?;
        let url = match chat.pr_url.clone() {
            Some(u) => u,
            None => {
                let gh = process::which("gh").ok_or("GitHub CLI (gh) isn't installed")?;
                let body: String = chat
                    .messages
                    .iter()
                    .filter(|m| m.role == "user")
                    .map(|m| format!("> {}", m.text.replace('\n', "\n> ")))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let body_file = chats_dir(&project.path).join(format!("{chat_id}.pr.md"));
                fs::write(&body_file, format!("Changes from a Pill chat.\n\n## Requests\n\n{body}\n")).map_err(|e| e.to_string())?;
                let mut cmd = process::tool(&gh);
                cmd.current_dir(&wt)
                    .args(["pr", "create", "--draft"])
                    .arg(format!("--base={}", project.base_branch))
                    .arg(format!("--head={branch}"))
                    .arg(format!("--title={}", chat.title))
                    .arg("--body-file")
                    .arg(&body_file);
                let out = process::run(cmd).map_err(|e| format!("gh pr create: {e}"))?;
                out.lines().rev().find(|l| l.starts_with("https://")).unwrap_or(out.trim()).to_string()
            }
        };
        chat.pr_url = Some(url.clone());
        self.save(&project.path, &chat)?;
        crate::toast::pr_ready(app, &chat.title, &project.name, &url);
        Ok(url)
    }
}

/// Looks up the project for a chat command.
pub fn project_for(app: &AppHandle, project_id: &str) -> Result<Project, String> {
    app.state::<AppState>()
        .workspace
        .lock()
        .unwrap()
        .project(project_id)
        .cloned()
        .ok_or_else(|| crate::workspace::PROJECT_GONE.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, text: &str, provider: Option<Provider>) -> ChatMessage {
        ChatMessage {
            id: "m".into(),
            role: role.into(),
            text: text.into(),
            at: String::new(),
            tools: Vec::new(),
            proposals: Vec::new(),
            error: None,
            model: None,
            provider,
        }
    }

    #[test]
    fn catch_up_names_who_said_what() {
        let block = catch_up_block(&[msg("user", "Add tests", None), msg("assistant", "Done", Some(Provider::Codex))], true);
        assert!(block.starts_with("While you were away"));
        assert!(block.contains("User: Add tests"));
        assert!(block.contains("Assistant (Codex): Done"));
        assert!(block.ends_with("The user's new message:\n\n"));
    }

    #[test]
    fn extracts_task_proposals_and_strips_the_block() {
        let text = "Found two issues.\n\n```pill-tasks\n[{\"title\":\"Fix null check\",\"notes\":\"in a.ts\",\"priority\":1},{\"title\":\"\",\"notes\":\"x\"},{\"title\":\"Add test\",\"priority\":9}]\n```\n";
        let (rest, tasks) = extract_proposals(text);
        assert_eq!(rest, "Found two issues.");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].priority, Some(1));
        assert_eq!(tasks[1].priority, None);
    }

    #[test]
    fn leaves_text_without_a_block_alone() {
        let (rest, tasks) = extract_proposals("Just an answer.");
        assert_eq!(rest, "Just an answer.");
        assert!(tasks.is_empty());
        let (rest, tasks) = extract_proposals("```pill-tasks\nnot json\n```");
        assert!(tasks.is_empty());
        assert_eq!(rest, "");
    }

    fn chat_json(id: &str, worktree: &str, branch: &str) -> Chat {
        let json = serde_json::json!({
            "id": id, "projectId": "p", "title": "t", "mode": "edit", "worktree": worktree, "branch": branch,
            "createdAt": "a", "updatedAt": "a", "messages": []
        });
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn committed_chat_files_cannot_point_outside_their_own_worktree_or_branch() {
        let repo = Path::new(r"C:\dev\app");
        let own = crate::worktree::path_for(repo, "c_1");
        let ok = checked(repo, "c_1", chat_json("c_1", &own.to_string_lossy(), "chat/c_1")).unwrap();
        assert_eq!((ok.worktree.as_deref(), ok.branch.as_deref()), (Some(own.as_path()), Some("chat/c_1")));

        let bad = checked(repo, "c_1", chat_json("c_1", r"C:\dev\.pill-worktrees\app\..\..", "main")).unwrap();
        assert_eq!((bad.worktree, bad.branch), (None, None));
        let other = checked(repo, "c_1", chat_json("c_1", &crate::worktree::path_for(repo, "c_2").to_string_lossy(), "chat/c_2")).unwrap();
        assert_eq!((other.worktree, other.branch), (None, None));
        assert!(checked(repo, "c_1", chat_json(r"..\..", "", "")).is_none());
        assert!(Chats::load(repo, r"..\..\x").unwrap_err().contains("isn't a chat id"));
    }

    #[test]
    fn titles_come_from_the_first_line() {
        assert_eq!(title_from("\n  Find bugs in the reel code\nmore"), "Find bugs in the reel code");
        assert_eq!(title_from(&"x".repeat(80)).chars().count(), 60);
    }

}
