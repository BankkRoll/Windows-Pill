//! Parses Claude Code's `--output-format stream-json` lines into the events
//! Pill shows: steps, tools used, files edited, test runs and the result.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One human-readable line in a run's activity log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunEvent {
    /// A pipeline step Pill performed (worktree, verify, push…).
    Step { text: String },
    /// Claude's own commentary.
    Text { text: String },
    /// A tool call, e.g. `Edit src/bonus.ts` or `Bash pnpm test`.
    Tool { name: String, detail: String },
    /// Output worth surfacing (test failures, errors).
    Output { text: String },
    /// The run (or a phase of it) finished.
    Result { ok: bool, text: String },
    /// What the user told the agent mid-run: retry feedback, plan approval.
    User { text: String },
}

/// What a single stream line means for the run.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Session(String),
    /// Model the CLI actually started with (Claude reports its full id).
    Model(String),
    Event(RunEvent),
    FileTouched(String),
    /// A plan delivered through the `ExitPlanMode` tool.
    Plan(String),
    /// The plan's usage limit was hit; further requests will fail until it resets.
    UsageLimit(crate::limits::UsageLimit),
    Finished(Finish),
}

/// The final `result` message.
#[derive(Debug, Clone, PartialEq)]
pub struct Finish {
    pub ok: bool,
    /// `success`, `error_max_turns`, `error_during_execution`…
    pub subtype: String,
    pub text: String,
    pub turns: Option<u32>,
    pub cost_usd: Option<f64>,
}

const FILE_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// Longest agent message kept in a run's activity, in characters.
pub const MESSAGE_LIMIT: usize = 8000;

/// An agent message as written, trimmed and capped at [`MESSAGE_LIMIT`].
/// Unlike [`clip`] it keeps line breaks, so markdown still renders.
pub fn message(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= MESSAGE_LIMIT {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(MESSAGE_LIMIT - 1).collect::<String>())
    }
}

fn clip(s: &str, max: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let cut: String = one_line.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// Makes `path` relative to the worktree for display.
fn rel(path: &str, root: &str) -> String {
    let p = path.replace('/', "\\");
    let r = root.replace('/', "\\");
    p.strip_prefix(&r).map(|s| s.trim_start_matches('\\').to_string()).unwrap_or(p)
}

/// Whether `path` is relative or under `root` (case-insensitive, either slash).
pub fn is_inside(path: &str, root: &str) -> bool {
    let p = path.replace('/', "\\").to_lowercase();
    let r = root.replace('/', "\\").to_lowercase();
    let absolute = p.starts_with("\\") || p.as_bytes().get(1) == Some(&b':');
    !absolute || p.strip_prefix(r.trim_end_matches('\\')).is_some_and(|rest| rest.starts_with('\\'))
}

/// Parses one stream-json line. Unknown or malformed lines yield nothing.
///
/// `worktree` is used to shorten absolute file paths for display.
pub fn parse_line(line: &str, worktree: &str) -> Vec<Parsed> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return Vec::new() };
    let mut out = Vec::new();
    match v["type"].as_str() {
        Some("system") if v["subtype"] == "init" => {
            if let Some(id) = v["session_id"].as_str() {
                out.push(Parsed::Session(id.to_string()));
            }
            if let Some(model) = v["model"].as_str().filter(|m| !m.is_empty()) {
                out.push(Parsed::Model(model.to_string()));
            }
        }
        Some("assistant") => {
            for item in v["message"]["content"].as_array().into_iter().flatten() {
                match item["type"].as_str() {
                    Some("text") => {
                        let text = item["text"].as_str().unwrap_or_default().trim();
                        if !text.is_empty() {
                            out.push(Parsed::Event(RunEvent::Text { text: message(text) }));
                        }
                    }
                    Some("tool_use") => out.extend(parse_tool(item, worktree)),
                    _ => {}
                }
            }
        }
        Some("rate_limit_event") => {
            let info = &v["rate_limit_info"];
            let resets_at = info["resetsAt"].as_i64();
            match info["status"].as_str().unwrap_or("allowed") {
                "rejected" => out.push(Parsed::UsageLimit(crate::limits::UsageLimit {
                    provider: crate::provider::Provider::Claude,
                    resets_at,
                    hint: None,
                })),
                // A heads-up only: the request itself went through.
                "allowed_warning" => {
                    let when = resets_at
                        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                        .map(|t| format!("; resets at {}", t.with_timezone(&chrono::Local).format("%a %H:%M")))
                        .unwrap_or_default();
                    out.push(Parsed::Event(RunEvent::Step { text: format!("Close to the Claude usage limit{when}") }));
                }
                _ => {}
            }
        }
        Some("result") => {
            let subtype = v["subtype"].as_str().unwrap_or("unknown").to_string();
            let ok = subtype == "success" && !v["is_error"].as_bool().unwrap_or(false);
            if let Some(id) = v["session_id"].as_str() {
                out.push(Parsed::Session(id.to_string()));
            }
            out.push(Parsed::Finished(Finish {
                ok,
                text: v["result"].as_str().unwrap_or_default().to_string(),
                subtype,
                turns: v["num_turns"].as_u64().map(|n| n as u32),
                cost_usd: v["total_cost_usd"].as_f64(),
            }));
        }
        _ => {}
    }
    out
}

fn parse_tool(item: &Value, worktree: &str) -> Vec<Parsed> {
    let name = item["name"].as_str().unwrap_or("tool").to_string();
    let input = &item["input"];
    let mut out = Vec::new();
    let detail = if FILE_TOOLS.contains(&name.as_str()) {
        let path = input["file_path"].as_str().or(input["notebook_path"].as_str()).unwrap_or_default();
        let shown = rel(path, worktree);
        // NOTE: the PR description lives in Pill's run folder, outside the
        // worktree; it isn't one of the changed files.
        if !path.is_empty() && is_inside(path, worktree) {
            out.push(Parsed::FileTouched(shown.clone()));
        }
        shown
    } else if name == "Bash" {
        clip(input["command"].as_str().unwrap_or_default(), 160)
    } else if name == "ExitPlanMode" {
        if let Some(plan) = input["plan"].as_str() {
            out.push(Parsed::Plan(plan.to_string()));
        }
        "plan ready".into()
    } else {
        let target = input["file_path"]
            .as_str()
            .or(input["path"].as_str())
            .or(input["pattern"].as_str())
            .or(input["description"].as_str())
            .unwrap_or_default();
        clip(&rel(target, worktree), 120)
    };
    out.insert(0, Parsed::Event(RunEvent::Tool { name, detail }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const WT: &str = r"C:\dev\.pill-worktrees\outlaw\t_1";

    #[test]
    fn files_outside_the_worktree_are_not_changes() {
        assert!(is_inside(r"src\a.ts", WT));
        assert!(is_inside(r"C:\dev\.pill-worktrees\outlaw\t_1\src\a.ts", WT));
        assert!(is_inside("c:/dev/.pill-worktrees/outlaw/t_1/src/a.ts", WT));
        assert!(!is_inside(r"C:\dev\outlaw\.pill\runs\r_1\pr.md", WT));
        assert!(!is_inside(r"C:\dev\.pill-worktrees\outlaw\t_10\a.ts", WT));
    }

    #[test]
    fn captures_session_from_init() {
        let p = parse_line(r#"{"type":"system","subtype":"init","session_id":"abc","tools":[]}"#, WT);
        assert_eq!(p, vec![Parsed::Session("abc".into())]);
    }

    #[test]
    fn edits_become_relative_file_events() {
        let line = r#"{"type":"assistant","message":{"content":[
            {"type":"text","text":"Fixing the scatter count."},
            {"type":"tool_use","name":"Edit","input":{"file_path":"C:\\dev\\.pill-worktrees\\outlaw\\t_1\\src\\bonus.ts"}}
        ]}}"#;
        let p = parse_line(line, WT);
        assert_eq!(p[0], Parsed::Event(RunEvent::Text { text: "Fixing the scatter count.".into() }));
        assert_eq!(p[1], Parsed::Event(RunEvent::Tool { name: "Edit".into(), detail: r"src\bonus.ts".into() }));
        assert_eq!(p[2], Parsed::FileTouched(r"src\bonus.ts".into()));
    }

    #[test]
    fn bash_and_plan_tools() {
        let bash = parse_line(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"pnpm test"}}]}}"#, WT);
        assert_eq!(bash[0], Parsed::Event(RunEvent::Tool { name: "Bash".into(), detail: "pnpm test".into() }));
        let plan = parse_line(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"ExitPlanMode","input":{"plan":"1. Do it"}}]}}"#, WT);
        assert!(plan.contains(&Parsed::Plan("1. Do it".into())));
    }

    #[test]
    fn result_reports_success_and_failures() {
        let ok = parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"Done","session_id":"s","num_turns":12,"total_cost_usd":0.5}"#, WT);
        assert!(matches!(&ok[1], Parsed::Finished(f) if f.ok && f.turns == Some(12)));
        let max = parse_line(r#"{"type":"result","subtype":"error_max_turns","is_error":true,"session_id":"s"}"#, WT);
        assert!(matches!(&max[1], Parsed::Finished(f) if !f.ok && f.subtype == "error_max_turns"));
    }

    /// A real `claude -p --output-format stream-json --verbose` transcript
    /// (paths scrubbed), so format drift in Claude Code breaks a test.
    #[test]
    fn parses_a_real_transcript() {
        let text = include_str!("../tests/fixtures/claude-stream.jsonl");
        let parsed: Vec<Parsed> = text.lines().flat_map(|l| parse_line(l, r"C:\dev\demo")).collect();
        assert!(parsed.iter().any(|p| matches!(p, Parsed::Session(s) if !s.is_empty())));
        assert!(parsed.iter().any(|p| matches!(p, Parsed::Event(RunEvent::Tool { name, .. }) if name == "Bash")));
        assert!(parsed.iter().any(|p| matches!(p, Parsed::Event(RunEvent::Text { .. }))));
        let finish = parsed.iter().find_map(|p| match p {
            Parsed::Finished(f) => Some(f),
            _ => None,
        });
        assert!(finish.is_some_and(|f| f.ok && f.turns == Some(2)));
    }

    #[test]
    fn surfaces_rate_limits_only_when_blocked() {
        let ok = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1795492800}}"#;
        assert!(parse_line(ok, WT).is_empty());
        let blocked = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1795492800}}"#;
        assert!(matches!(&parse_line(blocked, WT)[0], Parsed::UsageLimit(l) if l.resets_at == Some(1795492800)));
        let warning = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","resetsAt":1795492800}}"#;
        assert!(matches!(&parse_line(warning, WT)[0], Parsed::Event(RunEvent::Step { text }) if text.starts_with("Close to")));
    }

    #[test]
    fn ignores_garbage() {
        assert!(parse_line("not json", WT).is_empty());
        assert!(parse_line(r#"{"type":"user"}"#, WT).is_empty());
    }
}
