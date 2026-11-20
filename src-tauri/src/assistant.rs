//! The command bar's AI: one read-only request that can see every project
//! and answers with a short reply plus concrete actions (create tasks,
//! start or compare agents, open a chat) for the user to approve or deny.
//!
//! Nothing runs from here. The actions travel back to the UI as a preview;
//! only an explicit approval there turns them into tasks and runs.

use crate::approvals::{Answer, Conversation, Turn};
use crate::models::RunChoice;
use crate::process;
use crate::provider::{self, Phase, Provider, StreamParser};
use crate::state::AppState;
use crate::stream::{Parsed, RunEvent};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// Progress while the assistant works: `{ activity }`.
pub const ASSISTANT_EVENT: &str = "pill://assistant";

/// What happens to a created task once it exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartMode {
    #[default]
    None,
    Agent,
    Compare,
}

/// One proposed action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Action {
    CreateTask {
        /// `None` puts it in the Inbox.
        #[serde(default)]
        project_id: Option<String>,
        title: String,
        #[serde(default)]
        notes: String,
        #[serde(default)]
        priority: Option<u8>,
        /// `YYYY-MM-DD`.
        #[serde(default)]
        due: Option<String>,
        #[serde(default)]
        start: StartMode,
    },
    StartAgent {
        project_id: String,
        task_id: String,
        #[serde(default)]
        compare: bool,
    },
    Chat {
        project_id: String,
        message: String,
    },
}

/// The assistant's answer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub reply: String,
    pub actions: Vec<Action>,
    pub provider: Provider,
    pub model: Option<String>,
}

const PROTOCOL: &str = r#"You are the assistant inside Pill, a task and coding-agent app for the user's git projects on Windows.
You can read files in the user's projects (read-only). You cannot change anything yourself.
Answer in at most a few short sentences, then propose concrete actions for the user to approve.

End EVERY reply with a fenced block tagged pill-actions holding a JSON array (use [] when nothing fits):

```pill-actions
[
  {"type": "create_task", "projectId": "p_123", "title": "Upgrade lodash to 4.17.21 (CVE-2021-23337)", "notes": "Found in package.json", "priority": 1, "due": null, "start": "agent"},
  {"type": "start_agent", "projectId": "p_123", "taskId": "t_abc", "compare": false},
  {"type": "chat", "projectId": "p_123", "message": "Walk me through the auth flow"}
]
```

Rules:
- projectId must be an id from the project list, or null for the Inbox. taskId must be an existing task id.
- "start": "none" just files the task, "agent" hands it to a coding agent, "compare" runs Claude Code and Codex side by side.
- Only propose agent work the user asked for or clearly implied. One task per distinct change, per project.
- When asked to check many projects, look at their files first and only propose tasks where something actually applies.
- priority is 1 (urgent) to 3 (low) or null. due is YYYY-MM-DD or null."#;

/// Cancels the request in progress, if any.
static RUNNING: Mutex<Option<u32>> = Mutex::new(None);

/// Builds the context block: today's date and every project with its open tasks.
fn context(app: &AppHandle) -> String {
    let snapshot = app.state::<AppState>().snapshot();
    let mut out = format!("Today is {}.\n\nProjects:\n", chrono::Local::now().format("%A %Y-%m-%d"));
    for p in &snapshot.projects {
        let open: Vec<_> = p.tasks.iter().filter(|t| !matches!(t.status, crate::model::TaskStatus::Done)).collect();
        out.push_str(&format!(
            "- {} (id {}, {}, {} open task{}{})\n",
            p.name,
            p.id,
            p.path.display(),
            open.len(),
            if open.len() == 1 { "" } else { "s" },
            if p.has_remote { "" } else { ", no git remote" }
        ));
        for t in open.iter().take(8) {
            out.push_str(&format!("    - [{}] {} (task id {})\n", serde_json::to_string(&t.status).unwrap_or_default().trim_matches('"'), t.title, t.id));
        }
    }
    out.push_str(&format!("\nInbox: {} task(s).\n", snapshot.inbox.len()));
    out
}

/// Splits the reply from its `pill-actions` block and keeps only actions that
/// point at real projects and tasks.
pub fn extract_actions(text: &str, valid_project: impl Fn(&str) -> bool, valid_task: impl Fn(&str, &str) -> bool) -> (String, Vec<Action>) {
    let Some(start) = text.find("```pill-actions") else { return (text.trim().to_string(), Vec::new()) };
    let body_start = start + "```pill-actions".len();
    let Some(len) = text[body_start..].find("```") else { return (text[..start].trim().to_string(), Vec::new()) };
    let raw: Vec<serde_json::Value> = serde_json::from_str(text[body_start..body_start + len].trim()).unwrap_or_default();
    let actions = raw
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Action>(v).ok())
        .filter_map(|a| match a {
            Action::CreateTask { project_id, title, notes, priority, due, start } => {
                let title = title.trim().to_string();
                if title.is_empty() || project_id.as_deref().is_some_and(|id| !valid_project(id)) {
                    return None;
                }
                let due = due.filter(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok());
                let start = if project_id.is_none() { StartMode::None } else { start };
                Some(Action::CreateTask { project_id, title, notes, priority: priority.filter(|p| (1..=3).contains(p)), due, start })
            }
            Action::StartAgent { project_id, task_id, compare } => {
                valid_task(&project_id, &task_id).then_some(Action::StartAgent { project_id, task_id, compare })
            }
            Action::Chat { project_id, message } => {
                (valid_project(&project_id) && !message.trim().is_empty()).then_some(Action::Chat { project_id, message })
            }
        })
        .collect();
    let reply = format!("{}{}", &text[..start], &text[body_start + len + 3..]);
    (reply.trim().to_string(), actions)
}

/// Picks the provider: the explicit one, else Claude Code if installed, else Codex.
fn pick_provider(choice: &RunChoice) -> Result<Provider, String> {
    if let Some(p) = choice.provider {
        return Ok(p);
    }
    if process::which("claude").is_some() {
        Ok(Provider::Claude)
    } else if process::which("codex").is_some() {
        Ok(Provider::Codex)
    } else {
        Err("Install Claude Code or Codex to ask the assistant (see Settings → Accounts)".into())
    }
}

/// Runs one assistant request to completion. Blocking; call off the main thread.
pub fn ask(app: &AppHandle, text: &str, choice: RunChoice) -> Result<Plan, String> {
    if text.trim().is_empty() {
        return Err("Type a question first".into());
    }
    if !choice.is_valid() {
        return Err("That model or effort name isn't valid".into());
    }
    let provider = pick_provider(&choice)?;
    let state = app.state::<AppState>();
    let config = state.config.read().unwrap().clone();
    let global = config.models.get(provider);
    let model = Some(if choice.model.is_empty() { global.model.clone() } else { choice.model.clone() }).filter(|m| !m.is_empty());
    let effort = Some(if choice.effort.is_empty() { global.effort.clone() } else { choice.effort.clone() }).filter(|e| !e.is_empty());

    let home = state.config_path.parent().map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let state_dir = home.join("assistant");
    std::fs::create_dir_all(&state_dir).map_err(|e| e.to_string())?;
    let dirs: Vec<PathBuf> = config.roots.iter().chain(config.projects.iter()).filter(|p| p.is_dir()).cloned().collect();

    let mut cmd = provider::command(
        provider,
        &provider::Spec {
            project: None,
            cwd: &state_dir,
            extra_dirs: dirs.iter().map(PathBuf::as_path).collect(),
            resume: None,
            max_turns: 30,
            phase: Phase::ChatRead,
            state_dir: &state_dir,
            system: Some(PROTOCOL),
            model: model.as_deref(),
            effort: effort.as_deref(),
            partial: false,
            // The assistant only reads; it proposes actions instead of taking them.
            host_prompts: false,
            untrusted: false,
        },
    )?;
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let label = provider.label();
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start {label}: {e}"))?;
    #[cfg(windows)]
    let job = {
        let job = process::job::Job::new().map_err(|e| e.to_string())?;
        job.assign(&child).map_err(|e| e.to_string())?;
        job
    };
    *RUNNING.lock().unwrap() = Some(child.id());
    let prompt = format!("{}\n\nThe user asks:\n{text}", context(app));
    let mut conversation = Conversation::start(provider, &mut child, &provider::with_system(provider, Some(PROTOCOL), &prompt), &Turn { cwd: &state_dir, resume: None, host: false })?;
    let mut deny = |_| Answer::Deny { message: Some("The assistant only reads. Propose an action instead.".into()) };
    let stderr = child.stderr.take();
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut s) = stderr {
            let _ = std::io::Read::read_to_string(&mut s, &mut buf);
        }
        buf
    });

    let mut parser = StreamParser::new(provider, &state_dir);
    let mut final_text: Option<String> = None;
    let mut failure: Option<String> = None;
    let mut reported_model = None;
    let emit = |activity: String| {
        let _ = app.emit(ASSISTANT_EVENT, serde_json::json!({ "activity": activity }));
    };
    emit(format!("Asking {label}…"));
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let lines = BufReader::new(stdout).lines().map_while(Result::ok).flat_map(|raw| conversation.on_line(&raw, &mut deny));
    for line in lines {
        for p in parser.feed(&line) {
            match p {
                Parsed::Model(m) => reported_model = Some(m),
                Parsed::Event(RunEvent::Tool { name, detail }) => emit(format!("{name} {detail}").trim().to_string()),
                Parsed::Finished(f) if f.ok => final_text = Some(f.text),
                Parsed::Finished(f) => failure = Some(if f.text.is_empty() { format!("{label} stopped ({})", f.subtype) } else { f.text }),
                Parsed::UsageLimit(l) => failure = Some(l.describe()),
                _ => {}
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    drop(job);
    let cancelled = RUNNING.lock().unwrap().take().is_none();
    if cancelled {
        return Err("Stopped".into());
    }
    let stderr_text = stderr_reader.join().unwrap_or_default();
    if let Some(f) = failure {
        return Err(provider::explain_failure(provider, &f));
    }
    let Some(text) = final_text else {
        return Err(if stderr_text.trim().is_empty() {
            format!("{label} exited ({status}) without an answer")
        } else {
            provider::explain_failure(provider, stderr_text.trim())
        });
    };

    let snapshot = state.snapshot();
    let (reply, actions) = extract_actions(
        &text,
        |id| snapshot.projects.iter().any(|p| p.id == id),
        |pid, tid| snapshot.projects.iter().any(|p| p.id == pid && p.tasks.iter().any(|t| t.id == tid)),
    );
    Ok(Plan { reply, actions, provider, model: reported_model.or(model) })
}

/// Stops the request in progress. Its process tree dies with its job object.
pub fn stop() {
    if let Some(pid) = RUNNING.lock().unwrap().take() {
        let mut cmd = process::command("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
        let _ = process::run(cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_actions_that_point_at_real_things() {
        let text = r#"Two projects use lodash 4.17.20.

```pill-actions
[
  {"type": "create_task", "projectId": "p_1", "title": "Upgrade lodash", "priority": 1, "start": "agent"},
  {"type": "create_task", "projectId": "p_missing", "title": "Nope"},
  {"type": "create_task", "projectId": null, "title": "Inbox idea", "start": "agent", "due": "not a date"},
  {"type": "start_agent", "projectId": "p_1", "taskId": "t_1"},
  {"type": "start_agent", "projectId": "p_1", "taskId": "t_gone"},
  {"type": "chat", "projectId": "p_1", "message": "Explain"},
  {"type": "unknown"}
]
```"#;
        let (reply, actions) = extract_actions(text, |id| id == "p_1", |p, t| p == "p_1" && t == "t_1");
        assert_eq!(reply, "Two projects use lodash 4.17.20.");
        assert_eq!(actions.len(), 4);
        assert!(matches!(&actions[0], Action::CreateTask { start: StartMode::Agent, priority: Some(1), .. }));
        // Inbox tasks can't start agents, and bad dates are dropped.
        assert!(matches!(&actions[1], Action::CreateTask { project_id: None, start: StartMode::None, due: None, .. }));
        assert!(matches!(&actions[2], Action::StartAgent { .. }));
        assert!(matches!(&actions[3], Action::Chat { .. }));
    }

    #[test]
    fn reply_without_block_has_no_actions() {
        let (reply, actions) = extract_actions("Just an answer.", |_| true, |_, _| true);
        assert_eq!(reply, "Just an answer.");
        assert!(actions.is_empty());
    }
}
