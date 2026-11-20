//! Tool approvals: the CLI asks before using a tool its permissions don't
//! already allow, and the user answers from Pill.
//!
//! - [`Approvals`] holds pending questions, tells the UI about them and blocks
//!   the asking thread until someone answers (or the chat/run is stopped).
//! - [`Conversation`] owns a CLI's stdin. For Claude Code it speaks the
//!   stream-json control protocol (`--permission-prompts host`): it sends the
//!   prompt as a user message, answers `can_use_tool` requests and closes
//!   stdin once the result arrives. For Codex it writes the prompt and closes.
//!
//! "Always allow" sends Claude's own suggested rule back with a settings
//! destination, and the CLI writes it into the project's
//! `.claude/settings.local.json` (just me) or `.claude/settings.json`
//! (shared), so the rule also applies in the terminal and editor.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Child, ChildStdin};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::provider::Provider;
use crate::workspace::{new_id, now_rfc3339};

/// Event carrying the full list of pending approvals whenever it changes.
pub const EVENT: &str = "pill://approvals";

/// One question waiting for the user.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    pub project_id: String,
    /// Set when a chat is asking.
    pub chat_id: Option<String>,
    /// Set when an agent run is asking.
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    /// Chat or task title, for notifications and the Agents list.
    pub origin: String,
    pub provider: Provider,
    /// Tool name as the CLI reports it, e.g. `Bash`, `Edit`, `WebFetch`.
    pub tool: String,
    /// One line, e.g. "Run `npm install`" or "Edit src/main.ts".
    pub summary: String,
    /// Extra context: the CLI's description, or the text being written.
    pub detail: Option<String>,
    /// The rule "Always allow" would save, e.g. `Bash(npm install *)`;
    /// `None` when the CLI offers no lasting rule for this request.
    pub rule: Option<String>,
    /// Whether "for this chat/run" is on offer.
    pub session: bool,
    pub created_at: String,
}

/// The user's answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Answer {
    /// Just this once.
    Once,
    /// For the rest of this chat or run.
    Session,
    /// Save the rule to the project's Claude settings. `shared` writes the
    /// committed `.claude/settings.json` instead of the personal local file.
    Always { shared: bool },
    Deny { message: Option<String> },
}

impl Answer {
    fn deny(message: &str) -> Self {
        Self::Deny { message: Some(message.into()) }
    }
}

/// Pending approvals across all chats and runs.
#[derive(Default)]
pub struct Approvals {
    pending: Mutex<Vec<(Approval, Sender<Answer>)>>,
}

impl Approvals {
    /// Every pending approval, oldest first.
    pub fn list(&self) -> Vec<Approval> {
        self.pending.lock().unwrap().iter().map(|(a, _)| a.clone()).collect()
    }

    /// Whether anything from this run is waiting on the user.
    pub fn waiting_on_run(&self, run_id: &str) -> bool {
        self.pending.lock().unwrap().iter().any(|(a, _)| a.run_id.as_deref() == Some(run_id))
    }

    fn emit(&self, app: &AppHandle) {
        let _ = app.emit(EVENT, self.list());
    }

    /// Shows `approval` and blocks until it is answered. `stopped` is polled
    /// so stopping the chat or run releases the thread with a denial.
    pub fn ask(&self, app: &AppHandle, approval: Approval, stopped: &dyn Fn() -> bool) -> Answer {
        let (tx, rx) = mpsc::channel();
        let id = approval.id.clone();
        if approval.run_id.is_some() {
            // Runs work unattended, so the user may be anywhere.
            crate::notify::send(app, &tool_notice(app, &approval));
        }
        self.pending.lock().unwrap().push((approval, tx));
        self.emit(app);
        let answer = loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(a) => break a,
                Err(RecvTimeoutError::Timeout) if stopped() => break Answer::deny("Stopped by the user"),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break Answer::deny("No answer"),
            }
        };
        self.pending.lock().unwrap().retain(|(a, _)| a.id != id);
        self.emit(app);
        answer
    }

    /// Answers a pending approval.
    pub fn answer(&self, id: &str, answer: Answer) -> Result<(), String> {
        let pending = self.pending.lock().unwrap();
        let (_, tx) = pending.iter().find(|(a, _)| a.id == id).ok_or("That request was already answered")?;
        tx.send(answer).map_err(|_| "That request is no longer waiting".to_string())
    }
}

/// The notification for an agent run's tool request.
fn tool_notice(app: &AppHandle, approval: &Approval) -> crate::notify::Notice {
    use tauri::Manager;
    let project = app
        .state::<crate::state::AppState>()
        .workspace
        .lock()
        .unwrap()
        .project(&approval.project_id)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    crate::notify::Notice {
        need: crate::notify::Need::Tool,
        title: format!("Agent needs approval: {}", approval.origin),
        body: approval.summary.clone(),
        project,
        project_id: Some(approval.project_id.clone()),
        run_id: approval.run_id.clone(),
        task_id: approval.task_id.clone(),
        pr_url: None,
    }
}

/// A tool use the CLI wants approved, as parsed from its request.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolAsk {
    pub tool: String,
    pub input: Value,
    pub description: Option<String>,
    /// Claude's `permission_suggestions`, passed back on "for this session"
    /// and "always".
    pub suggestions: Vec<Value>,
}

impl ToolAsk {
    /// Builds the user-facing [`Approval`], minus where it came from.
    pub fn to_approval(&self, provider: Provider, project_id: &str, origin: &str) -> Approval {
        let (summary, detail) = summarize(&self.tool, &self.input, self.description.as_deref());
        Approval {
            id: new_id("ap"),
            project_id: project_id.into(),
            chat_id: None,
            run_id: None,
            task_id: None,
            origin: origin.into(),
            provider,
            tool: self.tool.clone(),
            summary,
            detail,
            rule: match provider {
                Provider::Claude => always_rules(self).iter().map(rule_label).next(),
                Provider::Codex => self.suggestions.iter().find_map(|d| d["acceptWithExecpolicyAmendment"]["execpolicy_amendment"].as_array()).map(|parts| {
                    let words: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
                    format!("{} …", words.join(" "))
                }),
            },
            session: match provider {
                Provider::Claude => true,
                Provider::Codex => self.suggestions.is_empty() || self.suggestions.iter().any(|d| d == "acceptForSession"),
            },
            created_at: now_rfc3339(),
        }
    }
}

/// How a CLI run starts, beyond its prompt.
pub struct Turn<'a> {
    pub cwd: &'a Path,
    /// Session (Claude) or thread (Codex) to continue.
    pub resume: Option<&'a str>,
    /// Whether tool approvals come to Pill. Codex then runs as an app-server;
    /// without it, `codex exec` runs and never asks.
    pub host: bool,
}

/// The stdin side of a running CLI.
pub struct Conversation {
    stdin: Option<ChildStdin>,
    provider: Provider,
    codex: Option<CodexSession>,
}

/// Codex app-server state: the JSON-RPC handshake, then one turn.
struct CodexSession {
    prompt: String,
    next_id: u64,
    thread_request: u64,
    /// Paths per file-change item, since its approval request names only the item.
    file_items: HashMap<String, Vec<String>>,
}

const CODEX_INIT: u64 = 1;
const CODEX_THREAD: u64 = 2;

impl Conversation {
    /// Sends the prompt. The CLI's stdin stays open for approval answers
    /// until the turn ends, except for `codex exec`, which reads the prompt
    /// and needs stdin closed.
    pub fn start(provider: Provider, child: &mut Child, prompt: &str, turn: &Turn) -> Result<Self, String> {
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        match provider {
            Provider::Claude => {
                let line = json!({ "type": "user", "message": { "role": "user", "content": prompt } });
                writeln!(stdin, "{line}").map_err(|e| e.to_string())?;
                Ok(Self { stdin: Some(stdin), provider, codex: None })
            }
            Provider::Codex if !turn.host => {
                stdin.write_all(prompt.as_bytes()).map_err(|e| e.to_string())?;
                Ok(Self { stdin: None, provider, codex: None })
            }
            Provider::Codex => {
                let cwd = turn.cwd.to_string_lossy();
                let thread = match turn.resume {
                    Some(id) => json!({ "id": CODEX_THREAD, "method": "thread/resume", "params": { "threadId": id, "cwd": cwd } }),
                    None => json!({ "id": CODEX_THREAD, "method": "thread/start", "params": { "cwd": cwd } }),
                };
                let client = json!({ "name": "pill", "title": "Pill", "version": env!("CARGO_PKG_VERSION") });
                let init = json!({ "id": CODEX_INIT, "method": "initialize", "params": { "clientInfo": client } });
                for msg in [init, json!({ "method": "initialized" }), thread] {
                    writeln!(stdin, "{msg}").map_err(|e| e.to_string())?;
                }
                let session = CodexSession { prompt: prompt.to_string(), next_id: 3, thread_request: CODEX_THREAD, file_items: HashMap::new() };
                Ok(Self { stdin: Some(stdin), provider, codex: Some(session) })
            }
        }
    }

    fn send(&mut self, msg: &Value) {
        if let Some(stdin) = self.stdin.as_mut() {
            if writeln!(stdin, "{msg}").and_then(|_| stdin.flush()).is_err() {
                self.stdin = None;
            }
        }
    }

    /// Handles one line of CLI output: answers protocol requests (calling
    /// `ask` for approvals, which blocks until the user answers) and returns
    /// the output lines to parse. Codex app-server messages come back
    /// translated to the `codex exec --experimental-json` events the parser
    /// understands.
    pub fn on_line(&mut self, line: &str, ask: &mut dyn FnMut(ToolAsk) -> Answer) -> Vec<String> {
        if self.codex.is_some() {
            return self.codex_line(line, ask);
        }
        if self.provider != Provider::Claude || self.stdin.is_none() {
            return vec![line.to_string()];
        }
        // PERF: most lines are output; skip the JSON parse for them.
        if !line.contains("\"control_request\"") && !line.contains("\"result\"") {
            return vec![line.to_string()];
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![line.to_string()] };
        match v["type"].as_str() {
            Some("control_request") => {
                let request_id = v["request_id"].as_str().unwrap_or_default().to_string();
                let request = &v["request"];
                let response = if request["subtype"] == "can_use_tool" {
                    let tool_ask = parse_can_use_tool(request);
                    let answer = ask(tool_ask.clone());
                    claude_response(&tool_ask, &answer)
                } else {
                    // Nothing else is expected from the CLI; acknowledge so it never hangs.
                    json!({})
                };
                self.send(&json!({
                    "type": "control_response",
                    "response": { "subtype": "success", "request_id": request_id, "response": response },
                }));
                Vec::new()
            }
            Some("result") => {
                // In stream-json input mode the CLI waits for more messages
                // until stdin closes.
                self.stdin = None;
                vec![line.to_string()]
            }
            _ => vec![line.to_string()],
        }
    }

    fn codex_line(&mut self, line: &str, ask: &mut dyn FnMut(ToolAsk) -> Answer) -> Vec<String> {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        let method = v["method"].as_str().map(str::to_string);
        let id = v.get("id").filter(|i| !i.is_null()).cloned();
        let params = &v["params"];

        let Some(method) = method else {
            // A response to one of our requests.
            if let Some(message) = v["error"]["message"].as_str() {
                self.stdin = None;
                return vec![json!({ "type": "turn.failed", "error": { "message": message } }).to_string()];
            }
            let session = self.codex.as_mut().expect("codex session");
            if id.as_ref().and_then(Value::as_u64) != Some(session.thread_request) {
                return Vec::new();
            }
            let thread = v["result"]["thread"]["id"].as_str().unwrap_or_default().to_string();
            let turn_request = session.next_id;
            session.next_id += 1;
            let input = json!([{ "type": "text", "text": session.prompt }]);
            self.send(&json!({ "id": turn_request, "method": "turn/start", "params": { "threadId": thread, "input": input } }));
            return vec![json!({ "type": "thread.started", "thread_id": thread }).to_string()];
        };

        if let Some(id) = id {
            // A request from the server; every one needs an answer or the turn stalls.
            let approval = matches!(
                method.as_str(),
                "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" | "item/permissions/requestApproval" | "execCommandApproval" | "applyPatchApproval"
            );
            if approval {
                let tool_ask = codex_ask(&method, params, &self.codex.as_ref().expect("codex session").file_items);
                let answer = ask(tool_ask.clone());
                self.send(&json!({ "id": id, "result": { "decision": codex_decision(&tool_ask, &answer) } }));
            } else {
                self.send(&json!({ "id": id, "error": { "code": -32601, "message": format!("Pill doesn't handle {method}") } }));
            }
            return Vec::new();
        }

        let item = &params["item"];
        let out = match method.as_str() {
            "item/started" | "item/completed" => {
                if item["type"] == "fileChange" {
                    let paths = item["changes"].as_array().into_iter().flatten().filter_map(|c| c["path"].as_str().map(str::to_string)).collect();
                    let key = item["id"].as_str().unwrap_or_default().to_string();
                    self.codex.as_mut().expect("codex session").file_items.insert(key, paths);
                }
                let kind = if method == "item/started" { "item.started" } else { "item.completed" };
                codex_exec_item(item).map(|i| json!({ "type": kind, "item": i })).into_iter().collect()
            }
            "turn/completed" => {
                self.stdin = None;
                let turn = &params["turn"];
                vec![match turn["status"].as_str() {
                    Some("completed") => json!({ "type": "turn.completed" }),
                    Some("interrupted") => json!({ "type": "turn.failed", "error": { "message": "Stopped" } }),
                    _ => json!({ "type": "turn.failed", "error": { "message": turn["error"]["message"].as_str().unwrap_or("Codex turn failed") } }),
                }]
            }
            "error" if params["willRetry"] != true => {
                vec![json!({ "type": "error", "message": params["error"]["message"].as_str().unwrap_or("Codex error") })]
            }
            _ => Vec::new(),
        };
        out.into_iter().map(|l| l.to_string()).collect()
    }
}

/// An app-server thread item in `codex exec --experimental-json` form.
fn codex_exec_item(item: &Value) -> Option<Value> {
    match item["type"].as_str()? {
        "agentMessage" => Some(json!({ "type": "agent_message", "text": item["text"] })),
        "commandExecution" => Some(json!({ "type": "command_execution", "command": item["command"], "status": item["status"] })),
        "fileChange" => Some(json!({ "type": "file_change", "changes": item["changes"] })),
        _ => None,
    }
}

fn codex_ask(method: &str, params: &Value, file_items: &HashMap<String, Vec<String>>) -> ToolAsk {
    let reason = params["reason"].as_str().map(str::to_string).filter(|s| !s.is_empty());
    let decisions = params["availableDecisions"].as_array().cloned().unwrap_or_default();
    if method.contains("ommand") || method == "execCommandApproval" {
        // Codex wraps commands in its shell launcher; `commandActions` has
        // what the user would recognise.
        let actions: Vec<&str> = params["commandActions"].as_array().into_iter().flatten().filter_map(|a| a["command"].as_str()).collect();
        let command = if !actions.is_empty() {
            actions.join(" && ")
        } else {
            match &params["command"] {
                Value::Array(parts) => parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
                other => other.as_str().unwrap_or_default().to_string(),
            }
        };
        return ToolAsk { tool: "Shell".into(), input: json!({ "command": command }), description: reason, suggestions: decisions };
    }
    if method.contains("ileChange") || method == "applyPatchApproval" {
        let paths = file_items.get(params["itemId"].as_str().unwrap_or_default()).cloned().unwrap_or_default();
        let file = if paths.len() == 1 { paths[0].clone() } else { format!("{} files", paths.len()) };
        return ToolAsk { tool: "Edit".into(), input: json!({ "file_path": file, "paths": paths }), description: reason, suggestions: decisions };
    }
    ToolAsk { tool: "Permissions".into(), input: params["permissions"].clone(), description: reason, suggestions: decisions }
}

/// The decision Codex expects. Only decisions the request offers are sent.
fn codex_decision(ask: &ToolAsk, answer: &Answer) -> Value {
    let offered = |name: &str| ask.suggestions.is_empty() || ask.suggestions.iter().any(|d| d == name || d.get(name).is_some());
    match answer {
        Answer::Once => json!("accept"),
        Answer::Session if offered("acceptForSession") => json!("acceptForSession"),
        Answer::Session => json!("accept"),
        Answer::Always { .. } => ask.suggestions.iter().find(|d| d.get("acceptWithExecpolicyAmendment").is_some()).cloned().unwrap_or_else(|| json!("accept")),
        Answer::Deny { .. } => json!("decline"),
    }
}

fn parse_can_use_tool(request: &Value) -> ToolAsk {
    ToolAsk {
        tool: request["tool_name"].as_str().unwrap_or("Tool").to_string(),
        input: request["input"].clone(),
        description: request["description"].as_str().map(str::to_string).filter(|s| !s.is_empty()),
        suggestions: request["permission_suggestions"].as_array().cloned().unwrap_or_default(),
    }
}

/// Rules "Always allow" saves: Claude's suggested `addRules`, or one built
/// from the request when it suggested none.
fn always_rules(ask: &ToolAsk) -> Vec<Value> {
    let suggested: Vec<Value> = ask.suggestions.iter().filter(|s| s["type"] == "addRules").flat_map(|s| s["rules"].as_array().cloned().unwrap_or_default()).collect();
    if !suggested.is_empty() {
        return suggested;
    }
    // SECURITY: never widen a shell approval to the whole Bash tool; without a
    // suggestion, the rule covers exactly this command.
    match (ask.tool.as_str(), ask.input["command"].as_str()) {
        ("Bash", Some(cmd)) => vec![json!({ "toolName": "Bash", "ruleContent": cmd })],
        ("Bash", None) => Vec::new(),
        (tool, _) => vec![json!({ "toolName": tool })],
    }
}

fn rule_label(rule: &Value) -> String {
    let tool = rule["toolName"].as_str().unwrap_or("Tool");
    match rule["ruleContent"].as_str() {
        Some(content) => format!("{tool}({content})"),
        None => tool.to_string(),
    }
}

/// The `can_use_tool` response for an answer.
fn claude_response(ask: &ToolAsk, answer: &Answer) -> Value {
    let allow = |updates: Vec<Value>| {
        let mut r = json!({ "behavior": "allow", "updatedInput": ask.input });
        if !updates.is_empty() {
            r["updatedPermissions"] = Value::Array(updates);
        }
        r
    };
    match answer {
        Answer::Once => allow(Vec::new()),
        Answer::Session => {
            let mut updates: Vec<Value> = ask.suggestions.iter().cloned().map(|mut s| {
                s["destination"] = json!("session");
                s
            }).collect();
            if updates.is_empty() {
                let rules = always_rules(ask);
                if !rules.is_empty() {
                    updates.push(json!({ "type": "addRules", "rules": rules, "behavior": "allow", "destination": "session" }));
                }
            }
            allow(updates)
        }
        Answer::Always { shared } => {
            let rules = always_rules(ask);
            if rules.is_empty() {
                return allow(Vec::new());
            }
            let destination = if *shared { "projectSettings" } else { "localSettings" };
            allow(vec![json!({ "type": "addRules", "rules": rules, "behavior": "allow", "destination": destination })])
        }
        Answer::Deny { message } => json!({
            "behavior": "deny",
            "message": message.clone().unwrap_or_else(|| "The user declined this. Ask before trying another way.".into()),
        }),
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// File name relative to nothing in particular: the last two path parts read
/// best in a one-line summary.
fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    parts[parts.len().saturating_sub(2)..].join("/")
}

/// A one-line summary and optional detail for a tool use.
fn summarize(tool: &str, input: &Value, description: Option<&str>) -> (String, Option<String>) {
    let s = |k: &str| input[k].as_str().unwrap_or_default();
    let desc = description.map(str::to_string);
    match tool {
        "Bash" | "PowerShell" | "Shell" => (format!("Run `{}`", clip(s("command"), 160)), desc),
        "Write" => (format!("Write {}", short_path(s("file_path"))), Some(clip(s("content"), 600))),
        "Edit" | "MultiEdit" => (format!("Edit {}", short_path(s("file_path"))), Some(clip(s("new_string"), 600)).filter(|d| !d.is_empty()).or(desc)),
        "NotebookEdit" => (format!("Edit notebook {}", short_path(s("notebook_path"))), desc),
        "WebFetch" => (format!("Fetch {}", clip(s("url"), 160)), desc),
        "WebSearch" => (format!("Search the web for “{}”", clip(s("query"), 120)), None),
        _ => {
            let compact = serde_json::to_string(input).unwrap_or_default();
            (format!("Use {tool}"), Some(clip(&compact, 400)).filter(|c| c != "null" && c != "{}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bash_ask(suggestions: Vec<Value>) -> ToolAsk {
        ToolAsk { tool: "Bash".into(), input: json!({ "command": "npm init -y" }), description: None, suggestions }
    }

    #[test]
    fn parses_the_cli_request() {
        let req = json!({
            "subtype": "can_use_tool", "tool_name": "Bash",
            "input": { "command": "npm init -y", "description": "Init" },
            "description": "Initialize a package.json",
            "permission_suggestions": [{ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": "npm init *" }], "behavior": "allow", "destination": "localSettings" }],
        });
        let ask = parse_can_use_tool(&req);
        assert_eq!(ask.tool, "Bash");
        assert_eq!(ask.description.as_deref(), Some("Initialize a package.json"));
        let a = ask.to_approval(Provider::Claude, "p1", "Chat");
        assert_eq!(a.summary, "Run `npm init -y`");
        assert_eq!(a.rule.as_deref(), Some("Bash(npm init *)"));
    }

    #[test]
    fn always_saves_the_suggested_rule_where_asked() {
        let ask = bash_ask(vec![json!({ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": "npm init *" }], "behavior": "allow", "destination": "localSettings" })]);
        let shared = claude_response(&ask, &Answer::Always { shared: true });
        assert_eq!(shared["behavior"], "allow");
        assert_eq!(shared["updatedPermissions"][0]["destination"], "projectSettings");
        assert_eq!(shared["updatedPermissions"][0]["rules"][0]["ruleContent"], "npm init *");
        let local = claude_response(&ask, &Answer::Always { shared: false });
        assert_eq!(local["updatedPermissions"][0]["destination"], "localSettings");
    }

    #[test]
    fn bash_without_a_suggestion_is_limited_to_the_exact_command() {
        let ask = bash_ask(Vec::new());
        let r = claude_response(&ask, &Answer::Always { shared: false });
        assert_eq!(r["updatedPermissions"][0]["rules"][0], json!({ "toolName": "Bash", "ruleContent": "npm init -y" }));
    }

    #[test]
    fn session_keeps_suggestions_but_scopes_them_to_the_session() {
        let ask = ToolAsk {
            tool: "Write".into(),
            input: json!({ "file_path": "C:\\r\\src\\a.ts", "content": "x" }),
            description: None,
            suggestions: vec![json!({ "type": "setMode", "mode": "acceptEdits", "destination": "session" })],
        };
        let r = claude_response(&ask, &Answer::Session);
        assert_eq!(r["updatedPermissions"], json!([{ "type": "setMode", "mode": "acceptEdits", "destination": "session" }]));
        assert_eq!(ask.to_approval(Provider::Claude, "p", "o").summary, "Write src/a.ts");
    }

    #[test]
    fn codex_requests_become_asks_and_answers_become_decisions() {
        let params = json!({
            "reason": "May I write hello.txt?",
            "command": "powershell -Command \"Set-Content hello.txt hi\"",
            "availableDecisions": ["accept", { "acceptWithExecpolicyAmendment": { "execpolicy_amendment": ["Set-Content", "hello.txt"] } }, "cancel"],
        });
        let ask = codex_ask("item/commandExecution/requestApproval", &params, &HashMap::new());
        let a = ask.to_approval(Provider::Codex, "p", "Task");
        assert!(a.summary.starts_with("Run `powershell"));
        let wrapped = json!({ "command": "powershell.exe -Command \"ls\"", "commandActions": [{ "type": "unknown", "command": "Set-Content hello.txt hi" }] });
        let unwrapped = codex_ask("item/commandExecution/requestApproval", &wrapped, &HashMap::new());
        assert_eq!(unwrapped.to_approval(Provider::Codex, "p", "o").summary, "Run `Set-Content hello.txt hi`");
        assert_eq!(a.rule.as_deref(), Some("Set-Content hello.txt …"));
        assert!(!a.session, "acceptForSession wasn't offered");
        assert_eq!(codex_decision(&ask, &Answer::Once), json!("accept"));
        assert_eq!(codex_decision(&ask, &Answer::Session), json!("accept"));
        assert_eq!(codex_decision(&ask, &Answer::Deny { message: None }), json!("decline"));
        assert_eq!(codex_decision(&ask, &Answer::Always { shared: false })["acceptWithExecpolicyAmendment"]["execpolicy_amendment"][0], "Set-Content");
    }

    #[test]
    fn codex_items_translate_to_exec_events() {
        assert_eq!(codex_exec_item(&json!({ "type": "agentMessage", "text": "Done" })), Some(json!({ "type": "agent_message", "text": "Done" })));
        assert_eq!(codex_exec_item(&json!({ "type": "reasoning" })), None);
        let mut items = HashMap::new();
        items.insert("fc1".to_string(), vec![r"C:\r\a.ts".to_string()]);
        let ask = codex_ask("item/fileChange/requestApproval", &json!({ "itemId": "fc1" }), &items);
        assert_eq!(ask.to_approval(Provider::Codex, "p", "o").summary, "Edit r/a.ts");
    }

    /// Drives a real `codex app-server` through [`Conversation`]. Needs Codex
    /// installed and logged in, so it only runs on request:
    /// `PILL_LIVE_DIR=<empty dir> cargo test live_codex -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_codex_asks_and_respects_a_denial() {
        use crate::provider::{self, Phase, Spec};
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        let dir = std::path::PathBuf::from(std::env::var("PILL_LIVE_DIR").expect("PILL_LIVE_DIR"));
        let state = dir.join(".state");
        std::fs::create_dir_all(&state).unwrap();
        let spec = Spec {
            project: None, cwd: &dir, extra_dirs: Vec::new(), resume: None, max_turns: 10, phase: Phase::ChatRead,
            state_dir: &state, system: None, model: None, effort: None, partial: false, host_prompts: true, untrusted: false,
        };
        let mut cmd = provider::command(Provider::Codex, &spec).unwrap();
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let prompt = "Create a file hello.txt containing hi in this folder using a shell command. Then stop.";
        let mut conv = Conversation::start(Provider::Codex, &mut child, prompt, &Turn { cwd: &dir, resume: None, host: true }).unwrap();
        let mut asked = Vec::new();
        let mut ask = |t: ToolAsk| {
            asked.push(t.to_approval(Provider::Codex, "p", "o").summary);
            Answer::Deny { message: None }
        };
        let stdout = child.stdout.take().unwrap();
        let lines: Vec<String> = BufReader::new(stdout).lines().map_while(Result::ok).flat_map(|raw| conv.on_line(&raw, &mut ask)).collect();
        let _ = child.wait();
        for l in &lines {
            println!("{l}");
        }
        println!("asked: {asked:?}");
        assert!(lines.iter().any(|l| l.contains("thread.started")));
        assert!(lines.iter().any(|l| l.contains("turn.completed") || l.contains("turn.failed")));
        assert!(!asked.is_empty(), "a write in a read-only sandbox should ask");
        assert!(!dir.join("hello.txt").exists(), "denied write must not happen");
    }

    /// The Claude counterpart of [`live_codex_asks_and_respects_a_denial`]:
    /// `PILL_LIVE_DIR=<empty dir> cargo test live_claude -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_claude_asks_and_respects_a_denial() {
        use crate::provider::{self, Phase, Spec};
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        let dir = std::path::PathBuf::from(std::env::var("PILL_LIVE_DIR").expect("PILL_LIVE_DIR"));
        let spec = Spec {
            project: None, cwd: &dir, extra_dirs: Vec::new(), resume: None, max_turns: 6, phase: Phase::ChatEdit,
            state_dir: &dir, system: None, model: Some("haiku"), effort: None, partial: false, host_prompts: true, untrusted: false,
        };
        let mut cmd = provider::command(Provider::Claude, &spec).unwrap();
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let prompt = "Run the shell command `npm init -y` with the Bash tool, once. Then stop.";
        let mut conv = Conversation::start(Provider::Claude, &mut child, prompt, &Turn { cwd: &dir, resume: None, host: true }).unwrap();
        let mut asked = Vec::new();
        let mut ask = |t: ToolAsk| {
            asked.push(t.to_approval(Provider::Claude, "p", "o"));
            Answer::Deny { message: Some("Not now".into()) }
        };
        let stdout = child.stdout.take().unwrap();
        let lines: Vec<String> = BufReader::new(stdout).lines().map_while(Result::ok).flat_map(|raw| conv.on_line(&raw, &mut ask)).collect();
        let _ = child.wait();
        println!("asked: {:?}", asked.iter().map(|a| (&a.summary, &a.rule)).collect::<Vec<_>>());
        assert!(lines.iter().any(|l| l.contains("\"type\":\"result\"")), "the run should finish once stdin closes");
        assert!(!asked.is_empty());
        assert!(!dir.join("package.json").exists(), "denied command must not run");
    }

    #[test]
    fn once_and_deny_carry_no_rules() {
        let ask = bash_ask(Vec::new());
        assert!(claude_response(&ask, &Answer::Once).get("updatedPermissions").is_none());
        let d = claude_response(&ask, &Answer::Deny { message: Some("no".into()) });
        assert_eq!(d, json!({ "behavior": "deny", "message": "no" }));
    }
}
