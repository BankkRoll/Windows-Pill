//! On-disk data formats for tasks and per-project settings.
//!
//! Every struct keeps unknown JSON fields in `extra` and writes them back, so
//! hand edits, other tools and newer Pill versions never lose data when this
//! version rewrites a file.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// Schema version written to `tasks.json` and `inbox.json`.
pub const TASKS_VERSION: u32 = 1;

/// Longest task, run or chat id Pill accepts.
pub const MAX_ID_LEN: usize = 64;

/// Whether `id` is a usable task, run or chat id: 1 to [`MAX_ID_LEN`] ASCII
/// letters, digits, `_` or `-`.
///
/// SECURITY: ids are read from files a repository can commit
/// (`.pill/tasks.json`, run and chat records) and end up in folder names Pill
/// deletes and in branch names it force-deletes. Anything that could climb
/// out of a folder (`..`, separators, drive prefixes) must never get through.
pub fn is_valid_id(id: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Board column a task sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Inbox,
    Todo,
    Doing,
    Review,
    Done,
}

/// Links between a task and its most recent agent run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentLink {
    pub last_run_id: Option<String>,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    /// Why the latest run stopped short (failed, stopped or interrupted), in
    /// plain words for the card. Autopilot leaves the task alone while this
    /// is set; starting a run or turning hands-off back on clears it.
    pub blocked: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The GitHub issue a task was made from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueLink {
    pub number: u64,
    /// Web page of the issue.
    pub url: String,
    /// Set by the GitHub sync once the issue is closed on GitHub.
    #[serde(default)]
    pub closed: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One task, as stored in `tasks.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    pub status: TaskStatus,
    /// 1 (high) to 3 (low), or `None`.
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Local calendar date, `YYYY-MM-DD`.
    #[serde(default)]
    pub due: Option<String>,
    /// Sort key within a column; lower first.
    #[serde(default)]
    pub order: f64,
    /// Hands-off: autopilot starts an agent on the task from Inbox or To do
    /// and the agent owns it from there, moving the card and notifying only
    /// when the user is needed. Stored as `agentReady`, its name from before
    /// it meant more than "may start automatically", so old files still load.
    #[serde(default)]
    pub agent_ready: bool,
    /// Note snippets handed to the agent as extra context.
    #[serde(default)]
    pub pinned_notes: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub agent: AgentLink,
    /// Set when the task was made from a GitHub issue; `None` in older files.
    #[serde(default)]
    pub issue: Option<IssueLink>,
    /// Who wrote the title and notes; see [`Task::source`]. Left out of the
    /// file for the user's own tasks.
    #[serde(default, skip_serializing_if = "TaskOrigin::is_user")]
    pub origin: TaskOrigin,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Task {
    /// Where the task's text came from. A task linked to an issue counts as
    /// [`TaskOrigin::Issue`] even in files from before `origin` existed.
    pub fn source(&self) -> TaskOrigin {
        if self.origin == TaskOrigin::User && self.issue.is_some() { TaskOrigin::Issue } else { self.origin }
    }
}

/// Where a task came from, which decides how far an agent trusts its text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskOrigin {
    /// Written by the user, or proposed in their own chat.
    #[default]
    User,
    /// Made from a GitHub issue: anyone who can file issues wrote its text.
    Issue,
    /// Suggested by a plugin from what it reads, such as public advisories.
    Plugin,
    /// A value from a newer Pill; trusted no more than a plugin.
    #[serde(other)]
    Unknown,
}

impl TaskOrigin {
    /// Whether the user wrote the task themselves.
    pub fn is_user(&self) -> bool {
        *self == Self::User
    }

    /// Whether someone other than the user wrote the task's text, so agents
    /// get it fenced and run with a tighter tool allowlist.
    pub fn is_untrusted(self) -> bool {
        !self.is_user()
    }

    /// Who wrote the text, as said in the prompt.
    pub fn author(self) -> &'static str {
        match self {
            Self::User => "the user",
            Self::Issue => "whoever filed the GitHub issue",
            Self::Plugin => "a plugin and the sources it reads",
            Self::Unknown => "someone outside this project",
        }
    }
}

/// Contents of a `tasks.json` or `inbox.json` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TasksFile {
    #[serde(default = "tasks_version")]
    pub version: u32,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn tasks_version() -> u32 {
    TASKS_VERSION
}

impl Default for TasksFile {
    fn default() -> Self {
        Self { version: TASKS_VERSION, tasks: Vec::new(), extra: Map::new() }
    }
}

/// Which editor "Open in editor" launches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditorChoice {
    /// Cursor if installed, else VS Code, else Explorer.
    #[default]
    Auto,
    Code,
    Cursor,
    Explorer,
}

/// What an agent run does with its branch once the checks pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShipMode {
    /// Push the branch and open a draft PR with `gh`.
    #[default]
    Pr,
    /// Push the branch without opening a PR.
    Push,
    /// Keep the commits on the local agent branch; nothing leaves the machine.
    Local,
}

impl ShipMode {
    /// Whether this mode pushes to the remote.
    pub fn pushes(self) -> bool {
        !matches!(self, Self::Local)
    }
}

/// Branch prefix used when a project doesn't set one.
pub const DEFAULT_BRANCH_PREFIX: &str = "agent/";

/// Turns a user-typed branch prefix into one git accepts as the start of a
/// ref name. Disallowed characters become `-`, components can't start with
/// `.` or end in `.lock`, and `..` collapses. Empty input gives
/// [`DEFAULT_BRANCH_PREFIX`].
///
/// # Example
/// ```ignore
/// assert_eq!(normalize_branch_prefix(" my bots/ "), "my-bots/");
/// ```
pub fn normalize_branch_prefix(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/') { c } else { '-' })
        .collect();
    let trailing_slash = cleaned.ends_with('/');
    let parts: Vec<String> = cleaned
        .split('/')
        .map(|part| {
            let mut s = part.to_string();
            while s.contains("..") {
                s = s.replace("..", ".");
            }
            let s = s.trim_start_matches(['.', '-']);
            s.strip_suffix(".lock").unwrap_or(s).to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return DEFAULT_BRANCH_PREFIX.into();
    }
    let mut out = parts.join("/");
    if trailing_slash {
        out.push('/');
    }
    out
}

/// Per-project settings in `.pill/project.json`. Empty strings mean "use the
/// default" so the file stays minimal and readable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectSettings {
    /// Display name; empty uses the folder name.
    pub name: String,
    /// `#RRGGBB`; empty picks a stable color from the path.
    pub color: String,
    /// Empty auto-detects from the remote's `HEAD`, else a local `main` or `master`.
    pub base_branch: String,
    pub test_command: String,
    pub lint_command: String,
    pub editor: EditorChoice,
    /// Extra rules appended to every agent prompt for this project.
    pub agent_rules: String,
    /// Commit `tasks.json` to git instead of ignoring it.
    pub commit_tasks: bool,
    /// Start agent runs in plan mode by default.
    pub plan_mode: bool,
    /// Which AI CLI runs agents and chats for this project.
    pub provider: crate::provider::Provider,
    /// Model for this project's provider; empty uses the global default.
    pub model: String,
    /// Reasoning effort; empty uses the global default.
    pub effort: String,
    /// What a finished run does with its branch.
    pub ship: ShipMode,
    /// Start of agent branch names; empty uses [`DEFAULT_BRANCH_PREFIX`].
    pub branch_prefix: String,
    /// Show open GitHub issues assigned to the signed-in `gh` user on the
    /// board. Only has an effect for GitHub remotes.
    pub github_issues: bool,
    /// The user said not now to setting up a test command; stop asking.
    pub skip_test_setup: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            name: String::new(),
            color: String::new(),
            base_branch: String::new(),
            test_command: String::new(),
            lint_command: String::new(),
            editor: EditorChoice::default(),
            agent_rules: String::new(),
            commit_tasks: false,
            plan_mode: false,
            provider: Default::default(),
            model: String::new(),
            effort: String::new(),
            ship: ShipMode::default(),
            branch_prefix: String::new(),
            // NOTE: serde fills fields missing from older files from here, so
            // existing projects get issues switched on too.
            github_issues: true,
            skip_test_setup: false,
            extra: Map::new(),
        }
    }
}

impl ProjectSettings {
    /// The branch prefix agents use, normalized to a valid ref start.
    pub fn agent_branch_prefix(&self) -> String {
        normalize_branch_prefix(&self.branch_prefix)
    }
}

/// Fields for a new task. Missing fields get defaults.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewTask {
    pub title: String,
    pub notes: String,
    pub status: Option<TaskStatus>,
    pub priority: Option<u8>,
    pub tags: Vec<String>,
    pub due: Option<String>,
    pub agent_ready: bool,
    /// Set by the core for issue and plugin tasks; never read from the UI.
    #[serde(skip)]
    pub origin: TaskOrigin,
}

/// A partial update. Nullable fields use a double `Option`: absent leaves the
/// value alone, `null` clears it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub status: Option<TaskStatus>,
    #[serde(deserialize_with = "double_option")]
    pub priority: Option<Option<u8>>,
    pub tags: Option<Vec<String>>,
    #[serde(deserialize_with = "double_option")]
    pub due: Option<Option<String>>,
    pub order: Option<f64>,
    pub agent_ready: Option<bool>,
    pub pinned_notes: Option<Vec<String>>,
}

pub(crate) fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Rejects values a hand-edited or malicious payload could smuggle in.
pub fn validate_fields(priority: Option<u8>, due: Option<&str>) -> Result<(), String> {
    if let Some(p) = priority {
        if !(1..=3).contains(&p) {
            return Err(format!("priority must be 1–3, got {p}"));
        }
    }
    if let Some(d) = due {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| format!("due must be YYYY-MM-DD, got {d:?}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_prefixes_become_valid_ref_starts() {
        assert_eq!(normalize_branch_prefix(""), "agent/");
        assert_eq!(normalize_branch_prefix("   "), "agent/");
        assert_eq!(normalize_branch_prefix("pill/"), "pill/");
        assert_eq!(normalize_branch_prefix("bot-"), "bot-");
        assert_eq!(normalize_branch_prefix(" my bots/ "), "my-bots/");
        assert_eq!(normalize_branch_prefix("/a//b/"), "a/b/");
        assert_eq!(normalize_branch_prefix(".hidden/x..y/"), "hidden/x.y/");
        assert_eq!(normalize_branch_prefix("refs.lock/"), "refs/");
        assert_eq!(normalize_branch_prefix("~^:?*[\\"), "agent/");
    }

    #[test]
    fn ship_mode_defaults_to_pr_and_round_trips() {
        let s: ProjectSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.ship, ShipMode::Pr);
        assert!(ShipMode::Push.pushes());
        assert!(!ShipMode::Local.pushes());
        let s: ProjectSettings = serde_json::from_str(r#"{"ship":"local","branchPrefix":"bot/"}"#).unwrap();
        assert_eq!(s.ship, ShipMode::Local);
        assert_eq!(s.agent_branch_prefix(), "bot/");
        assert_eq!(serde_json::to_value(&s).unwrap()["ship"], "local");
    }

    #[test]
    fn parses_spec_example_and_keeps_unknown_fields() {
        let json = r#"{
          "version": 1,
          "custom": "kept",
          "tasks": [{
            "id": "t_8f2k1",
            "title": "Fix bonus round not triggering on 3 scatters",
            "notes": "Repro: seed 4411.",
            "status": "todo",
            "priority": 1,
            "tags": ["bug"],
            "due": "2026-11-21",
            "order": 3,
            "createdAt": "2026-11-19T21:04:00Z",
            "updatedAt": "2026-11-19T21:04:00Z",
            "agent": { "lastRunId": null, "branch": null, "prUrl": null },
            "estimate": 5
          }]
        }"#;
        let file: TasksFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.tasks[0].status, TaskStatus::Todo);
        assert!(!file.tasks[0].agent_ready);
        assert_eq!(file.tasks[0].agent.blocked, None);
        let out = serde_json::to_value(&file).unwrap();
        assert_eq!(out["custom"], "kept");
        assert_eq!(out["tasks"][0]["estimate"], 5);
    }

    #[test]
    fn old_files_get_issue_defaults() {
        let s: ProjectSettings = serde_json::from_str(r#"{"name":"x"}"#).unwrap();
        assert!(s.github_issues);
        let s: ProjectSettings = serde_json::from_str(r#"{"githubIssues":false}"#).unwrap();
        assert!(!s.github_issues);
        let t: Task = serde_json::from_str(r#"{"id":"t_1","title":"x","status":"todo","createdAt":"a","updatedAt":"a"}"#).unwrap();
        assert_eq!(t.issue, None);
        let t: Task = serde_json::from_str(
            r#"{"id":"t_1","title":"x","status":"todo","createdAt":"a","updatedAt":"a",
                "issue":{"number":12,"url":"https://github.com/o/r/issues/12","nodeId":"kept"}}"#,
        )
        .unwrap();
        let link = t.issue.as_ref().unwrap();
        assert_eq!((link.number, link.closed), (12, false));
        assert_eq!(serde_json::to_value(&t).unwrap()["issue"]["nodeId"], "kept");
    }

    #[test]
    fn origin_is_kept_and_issue_tasks_count_as_untrusted() {
        let base = r#""id":"t_1","title":"x","status":"todo","createdAt":"a","updatedAt":"a""#;
        let mine: Task = serde_json::from_str(&format!("{{{base}}}")).unwrap();
        assert_eq!(mine.source(), TaskOrigin::User);
        assert!(serde_json::to_value(&mine).unwrap().get("origin").is_none());
        let plugin: Task = serde_json::from_str(&format!(r#"{{{base},"origin":"plugin"}}"#)).unwrap();
        assert!(plugin.source().is_untrusted());
        assert_eq!(serde_json::to_value(&plugin).unwrap()["origin"], "plugin");
        let old_issue: Task = serde_json::from_str(&format!(r#"{{{base},"issue":{{"number":1,"url":"u"}}}}"#)).unwrap();
        assert_eq!(old_issue.source(), TaskOrigin::Issue);
        let newer: Task = serde_json::from_str(&format!(r#"{{{base},"origin":"martian"}}"#)).unwrap();
        assert!(newer.source().is_untrusted());
        let draft: NewTask = serde_json::from_str(r#"{"title":"x","origin":"user"}"#).unwrap();
        assert_eq!(draft.origin, TaskOrigin::User);
    }

    #[test]
    fn patch_distinguishes_absent_from_null() {
        let p: TaskPatch = serde_json::from_str(r#"{ "due": null }"#).unwrap();
        assert_eq!(p.due, Some(None));
        assert_eq!(p.priority, None);
    }

    #[test]
    fn ids_cannot_name_a_path_outside_their_folder() {
        for ok in ["t_8f2k1", "r_20261119_211000_8f2k1_ab12c", "c_x-1", &"a".repeat(MAX_ID_LEN)] {
            assert!(is_valid_id(ok), "{ok}");
        }
        for bad in ["", "..", r"..\..\..", "../x", "t_1/..", r"C:\Users", "t 1", "t_1\0", "ü", &"a".repeat(MAX_ID_LEN + 1)] {
            assert!(!is_valid_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn validation_rejects_bad_values() {
        assert!(validate_fields(Some(4), None).is_err());
        assert!(validate_fields(None, Some("2026-13-01")).is_err());
        assert!(validate_fields(Some(2), Some("2026-11-21")).is_ok());
    }
}
