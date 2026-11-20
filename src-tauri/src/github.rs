//! GitHub issues and pull request status, read through the user's own `gh`
//! login. Pill never stores a token.
//!
//! Responsibilities:
//! - Parse the JSON `gh issue list`, `gh issue view` and `gh pr view` print.
//! - Per GitHub project: list open issues assigned to `@me`, read the review
//!   decision and CI rollup of every PR Pill opened, and check whether issues
//!   linked to tasks were closed.
//! - Sync on a background thread: when the panel opens (at most once a
//!   minute), every 5 minutes while the panel has been used recently and every
//!   15 minutes otherwise. Failures back off per project; a rate limit pauses
//!   everything.
//! - Feed results back into tasks: a merged PR moves its task to Done, a
//!   closed issue is marked on its task.
//!
//! Projects without a GitHub remote, and machines without a signed-in `gh`,
//! get a plain status instead of repeated errors.

use crate::agent::Agents;
use crate::gitinfo;
use crate::model::{Task, TaskStatus};
use crate::process::{self, git};
use crate::state::AppState;
use crate::workspace::{now_rfc3339, TaskSync};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Event carrying every project's [`ProjectGithub`], keyed by project id.
pub const GITHUB_EVENT: &str = "pill://github";

/// Sync cadence while the panel was opened in the last [`RECENT_USE`].
const ACTIVE_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Sync cadence when nobody has looked at Pill for a while.
const IDLE_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// How long an opened panel counts as "in use".
const RECENT_USE: Duration = Duration::from_secs(30 * 60);
/// Opening the panel syncs projects older than this.
const FOCUS_MIN_AGE: Duration = Duration::from_secs(60);
/// First sync after launch, so startup isn't slowed by network calls.
const STARTUP_DELAY: Duration = Duration::from_secs(5);
/// Longest a single `gh` call may take.
const GH_TIMEOUT: Duration = Duration::from_secs(45);
/// Issues listed per project.
const ISSUE_LIMIT: u32 = 100;
/// Linked issues looked up one by one per sync; the rest wait for the next.
const ISSUE_LOOKUPS: usize = 15;
/// PRs of runs older than this are no longer polled.
const RUN_PR_WINDOW_DAYS: i64 = 30;
/// Projects synced at the same time.
const PARALLEL: usize = 4;

// -----------------------------------------------------------------------------
// gh JSON shapes
// -----------------------------------------------------------------------------

/// An issue label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub name: String,
    /// Hex color without `#`, as GitHub stores it.
    #[serde(default)]
    pub color: String,
}

/// An open issue assigned to the user. Deserializes from
/// `gh issue list --json number,title,url,body,labels,updatedAt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// Markdown written by the issue's author. Untrusted.
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub updated_at: String,
}

/// `gh issue view --json number,state` output.
#[derive(Debug, Clone, Deserialize)]
struct IssueStateJson {
    number: u64,
    state: String,
}

/// One entry of `statusCheckRollup`: an Actions/App check run or a legacy
/// commit status.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "__typename")]
enum CheckJson {
    CheckRun {
        #[serde(default)]
        status: String,
        #[serde(default)]
        conclusion: String,
    },
    StatusContext {
        #[serde(default)]
        state: String,
    },
    #[serde(other)]
    Other,
}

/// `gh pr view --json number,url,state,isDraft,reviewDecision,statusCheckRollup` output.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrJson {
    number: u64,
    url: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    status_check_rollup: Option<Vec<CheckJson>>,
}

/// Lifecycle of a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

/// The PR's review decision under the repository's branch protection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    Approved,
    ChangesRequested,
    /// Branch protection wants a review nobody has given yet.
    #[serde(rename = "review_required")]
    Required,
    /// No review rule applies and nobody has reviewed.
    None,
}

/// Combined CI result of the PR's head commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Checks {
    Passing,
    Failing,
    Pending,
    /// The commit has no checks.
    None,
}

/// What the UI shows about one pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrStatus {
    pub number: u64,
    pub url: String,
    pub state: PrState,
    pub draft: bool,
    pub review: Review,
    pub checks: Checks,
    /// Checks that succeeded (skipped and neutral ones count as passed).
    pub passed: u32,
    pub failed: u32,
    pub pending: u32,
}

/// Parses `gh issue list --json …` output.
pub fn parse_issues(json: &str) -> Result<Vec<Issue>, String> {
    serde_json::from_str(json).map_err(|e| format!("Unexpected output from gh issue list: {e}"))
}

/// Parses `gh issue view --json number,state` output into (number, closed).
pub fn parse_issue_state(json: &str) -> Result<(u64, bool), String> {
    let s: IssueStateJson = serde_json::from_str(json).map_err(|e| format!("Unexpected output from gh issue view: {e}"))?;
    // `gh issue view` also answers for PR numbers, which can be MERGED.
    Ok((s.number, s.state != "OPEN"))
}

/// Parses `gh pr view --json …` output.
pub fn parse_pr(json: &str) -> Result<PrStatus, String> {
    let pr: PrJson = serde_json::from_str(json).map_err(|e| format!("Unexpected output from gh pr view: {e}"))?;
    let state = match pr.state.as_str() {
        "MERGED" => PrState::Merged,
        "CLOSED" => PrState::Closed,
        _ => PrState::Open,
    };
    let review = match pr.review_decision.as_deref() {
        Some("APPROVED") => Review::Approved,
        Some("CHANGES_REQUESTED") => Review::ChangesRequested,
        Some("REVIEW_REQUIRED") => Review::Required,
        _ => Review::None,
    };
    let (mut passed, mut failed, mut pending) = (0, 0, 0);
    for check in pr.status_check_rollup.unwrap_or_default() {
        match check_outcome(&check) {
            Some(Outcome::Pass) => passed += 1,
            Some(Outcome::Fail) => failed += 1,
            Some(Outcome::Pending) => pending += 1,
            None => {}
        }
    }
    let checks = if failed > 0 {
        Checks::Failing
    } else if pending > 0 {
        Checks::Pending
    } else if passed > 0 {
        Checks::Passing
    } else {
        Checks::None
    };
    Ok(PrStatus { number: pr.number, url: pr.url, state, draft: pr.is_draft, review, checks, passed, failed, pending })
}

enum Outcome {
    Pass,
    Fail,
    Pending,
}

/// Classifies one rollup entry the way GitHub's own rollup does.
fn check_outcome(check: &CheckJson) -> Option<Outcome> {
    match check {
        CheckJson::CheckRun { status, conclusion } => {
            if status != "COMPLETED" {
                return Some(Outcome::Pending);
            }
            match conclusion.as_str() {
                "SUCCESS" | "NEUTRAL" | "SKIPPED" => Some(Outcome::Pass),
                "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => Some(Outcome::Fail),
                _ => None,
            }
        }
        CheckJson::StatusContext { state } => match state.as_str() {
            "SUCCESS" => Some(Outcome::Pass),
            "FAILURE" | "ERROR" => Some(Outcome::Fail),
            "PENDING" | "EXPECTED" => Some(Outcome::Pending),
            _ => None,
        },
        CheckJson::Other => None,
    }
}

/// The `[HOST/]OWNER/REPO` that `gh -R` takes, from a remote's display form
/// (`github.com/owner/repo`). `None` for anything that isn't exactly that.
pub fn repo_spec(display: &str) -> Option<String> {
    let parts: Vec<&str> = display.split('/').collect();
    (parts.len() == 3 && parts.iter().all(|p| !p.is_empty())).then(|| display.to_string())
}

/// Appends `Closes #N` to a PR body unless it already closes that issue, so
/// merging the PR closes the issue on GitHub.
///
/// # Example
/// ```ignore
/// assert_eq!(with_closing_line("Fix", 7), "Fix\n\nCloses #7\n");
/// ```
pub fn with_closing_line(body: &str, number: u64) -> String {
    const KEYWORDS: &[&str] = &["close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved"];
    let lower = body.to_lowercase();
    let target = format!("#{number}");
    let words: Vec<&str> = lower.split_whitespace().collect();
    let already = words.windows(2).any(|w| {
        let keyword = w[0].trim_end_matches(':');
        let reference = w[1].trim_end_matches(['.', ',', ';', ')']);
        KEYWORDS.contains(&keyword) && reference == target
    });
    if already {
        return body.to_string();
    }
    let trimmed = body.trim_end();
    if trimmed.is_empty() { format!("Closes #{number}\n") } else { format!("{trimmed}\n\nCloses #{number}\n") }
}

// -----------------------------------------------------------------------------
// Errors and status
// -----------------------------------------------------------------------------

/// Where a project's sync stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncKind {
    /// Not synced yet this session.
    Waiting,
    Ok,
    /// The remote isn't on GitHub, or the project has no remote.
    NotGithub,
    /// The GitHub CLI isn't installed.
    NoGh,
    /// `gh` has no valid login for the remote's host.
    SignedOut,
    /// GitHub's API rate limit was hit; everything waits until `retry_at`.
    RateLimited,
    Error,
}

/// A project's sync status, as the settings page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub kind: SyncKind,
    /// Short sentence for the UI; empty when the kind says it all.
    pub message: String,
    /// When the last successful sync finished (RFC 3339).
    pub synced_at: Option<String>,
    /// When a failed or paused sync is tried again (RFC 3339).
    pub retry_at: Option<String>,
}

/// Everything the UI knows about one project on GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGithub {
    /// `owner/repo`, when the remote is on GitHub.
    pub repo: Option<String>,
    pub status: SyncStatus,
    /// True while a sync for this project is running.
    pub syncing: bool,
    /// Open issues assigned to the user; empty when the project hides them.
    pub issues: Vec<Issue>,
    /// Status of every polled PR, keyed by its URL.
    pub prs: HashMap<String, PrStatus>,
}

impl Default for ProjectGithub {
    fn default() -> Self {
        Self {
            repo: None,
            status: SyncStatus { kind: SyncKind::Waiting, message: String::new(), synced_at: None, retry_at: None },
            syncing: false,
            issues: Vec::new(),
            prs: HashMap::new(),
        }
    }
}

/// Why a `gh` call failed, sorted into what Pill does about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhError {
    RateLimited,
    SignedOut,
    /// The repository doesn't exist or this login can't see it.
    NotFound,
    IssuesDisabled,
    Other(String),
}

/// Sorts `gh`'s error text into a [`GhError`].
pub fn classify_error(stderr: &str) -> GhError {
    let lower = stderr.to_lowercase();
    if lower.contains("rate limit") || lower.contains("abuse detection") {
        GhError::RateLimited
    } else if lower.contains("gh auth login") || lower.contains("http 401") || lower.contains("bad credentials") || lower.contains("not logged") {
        GhError::SignedOut
    } else if lower.contains("has disabled issues") {
        GhError::IssuesDisabled
    } else if lower.contains("could not resolve to a repository") || lower.contains("http 404") {
        GhError::NotFound
    } else {
        let line = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("gh failed");
        GhError::Other(line.chars().take(200).collect())
    }
}

/// Delay before retrying a project after `failures` failures in a row:
/// 2, 4, 8… minutes, capped at an hour.
pub fn backoff(failures: u32) -> Duration {
    let minutes = 2u64.saturating_pow(failures.clamp(1, 6)).min(60);
    Duration::from_secs(minutes * 60)
}

// -----------------------------------------------------------------------------
// Feeding results back into tasks
// -----------------------------------------------------------------------------

/// Decides which tasks the sync changes:
///
/// - a task in Doing or Review whose PR merged moves to Done, once per PR
///   (`applied` remembers merges already acted on, so moving the task back
///   by hand sticks);
/// - a linked issue's `closed` flag follows GitHub;
/// - when an issue closes and the task has no open PR and no agent working on
///   it, the task moves to Done, since there is nothing left to do.
///
/// `issue_closed` holds issue states learned this sync; `busy` holds task ids
/// with an active agent run.
pub fn plan_task_updates(
    tasks: &[Task],
    prs: &HashMap<String, PrStatus>,
    issue_closed: &HashMap<u64, bool>,
    busy: &HashSet<String>,
    applied: &HashSet<String>,
) -> Vec<TaskSync> {
    let mut out = Vec::new();
    for t in tasks {
        let pr_url = t.agent.pr_url.as_deref();
        let pr = pr_url.and_then(|u| prs.get(u));
        let mut status = None;
        if let (Some(pr), Some(url)) = (pr, pr_url) {
            if pr.state == PrState::Merged && matches!(t.status, TaskStatus::Doing | TaskStatus::Review) && !applied.contains(url) {
                status = Some(TaskStatus::Done);
            }
        }
        let mut closed = None;
        if let Some(link) = &t.issue {
            if let Some(&now_closed) = issue_closed.get(&link.number) {
                if now_closed != link.closed {
                    closed = Some(now_closed);
                    let pr_settled = pr_url.is_none() || pr.is_some_and(|p| p.state != PrState::Open);
                    if now_closed && status.is_none() && t.status != TaskStatus::Done && pr_settled && !busy.contains(&t.id) {
                        status = Some(TaskStatus::Done);
                    }
                }
            }
        }
        if status.is_some() || closed.is_some() {
            out.push(TaskSync { task_id: t.id.clone(), status, issue_closed: closed });
        }
    }
    out
}

// -----------------------------------------------------------------------------
// Running gh
// -----------------------------------------------------------------------------

/// Runs `gh` with a timeout and no prompts. Returns stdout, or the classified
/// error.
fn run_gh(exe: &Path, cwd: &Path, args: &[&str]) -> Result<String, GhError> {
    let mut cmd = process::tool(exe);
    cmd.current_dir(cwd)
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| GhError::Other(format!("Couldn't start gh: {e}")))?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });
    let deadline = Instant::now() + GH_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GhError::Other("gh took too long to answer".into()));
            }
            Err(e) => return Err(GhError::Other(e.to_string())),
        }
    };
    let out = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    if status.success() {
        Ok(out)
    } else {
        Err(classify_error(if err.trim().is_empty() { &out } else { &err }))
    }
}

/// Open issues in `repo` assigned to the signed-in user.
fn list_issues(exe: &Path, cwd: &Path, repo: &str) -> Result<Vec<Issue>, GhError> {
    let limit = ISSUE_LIMIT.to_string();
    let out = run_gh(
        exe,
        cwd,
        &["issue", "list", "-R", repo, "--assignee", "@me", "--state", "open", "--limit", &limit, "--json", "number,title,url,body,labels,updatedAt"],
    )?;
    parse_issues(&out).map_err(GhError::Other)
}

fn pr_status(exe: &Path, cwd: &Path, url: &str) -> Result<PrStatus, GhError> {
    let out = run_gh(exe, cwd, &["pr", "view", url, "--json", "number,url,state,isDraft,reviewDecision,statusCheckRollup"])?;
    parse_pr(&out).map_err(GhError::Other)
}

fn issue_closed(exe: &Path, cwd: &Path, repo: &str, number: u64) -> Result<bool, GhError> {
    let out = run_gh(exe, cwd, &["issue", "view", &number.to_string(), "-R", repo, "--json", "number,state"])?;
    parse_issue_state(&out).map(|(_, closed)| closed).map_err(GhError::Other)
}

/// Reads one issue fresh from GitHub, for an agent's prompt.
fn fetch_issue(exe: &Path, cwd: &Path, repo: &str, number: u64) -> Result<Issue, GhError> {
    let out = run_gh(exe, cwd, &["issue", "view", &number.to_string(), "-R", repo, "--json", "number,title,url,body,labels,updatedAt"])?;
    serde_json::from_str(&out).map_err(|e| GhError::Other(format!("Unexpected output from gh issue view: {e}")))
}

/// The project's preferred remote as `gh -R` wants it, when it's on GitHub.
fn github_repo(path: &Path) -> Option<String> {
    let out = git(path, &["config", "--get-regexp", r"^remote\..*\.url$"]).ok()?;
    let (name, url) = gitinfo::pick_remote(&out)?;
    let remote = gitinfo::describe_remote(&name, &url);
    if remote.github { repo_spec(&remote.display) } else { None }
}

fn host_of(spec: &str) -> &str {
    spec.split('/').next().unwrap_or("github.com")
}

/// `owner/repo` part of a `host/owner/repo` spec, for display.
fn short_repo(spec: &str) -> String {
    spec.split_once('/').map_or(spec, |(_, rest)| rest).to_string()
}

// -----------------------------------------------------------------------------
// Sync engine
// -----------------------------------------------------------------------------

/// What woke the sync thread.
enum Wake {
    /// The panel opened or the window got focus: sync whatever is a minute old.
    Focus,
    /// Sync one project now (settings changed, refresh pressed, PR opened).
    Project(String),
}

#[derive(Default)]
struct Entry {
    view: ProjectGithub,
    failures: u32,
    next_at: Option<Instant>,
    last_attempt: Option<Instant>,
    /// Merged PRs whose task was already moved to Done this session.
    applied_merges: HashSet<String>,
}

#[derive(Default)]
struct Inner {
    projects: HashMap<String, Entry>,
    last_focus: Option<Instant>,
    paused_until: Option<(Instant, String)>,
    rate_hits: u32,
}

/// GitHub sync state, shared by commands and the sync thread.
pub struct Github {
    inner: Mutex<Inner>,
    wake: Mutex<Option<Sender<Wake>>>,
}

impl Default for Github {
    fn default() -> Self {
        Self { inner: Mutex::new(Inner::default()), wake: Mutex::new(None) }
    }
}

/// Inputs for one project's sync, copied out of the workspace so no lock is
/// held while `gh` runs.
#[derive(Clone)]
struct Job {
    project_id: String,
    path: PathBuf,
    show_issues: bool,
    tasks: Vec<Task>,
    run_prs: Vec<String>,
}

/// What one sync read: assigned issues, PR status by URL, and which linked
/// issues are closed, by number.
type Synced = (Vec<Issue>, HashMap<String, PrStatus>, HashMap<u64, bool>);

/// Result of one project's sync.
struct Fetched {
    repo: Option<String>,
    outcome: Result<Synced, GhError>,
}

impl Github {
    /// Every project's GitHub view, keyed by project id.
    pub fn snapshot(&self) -> HashMap<String, ProjectGithub> {
        self.inner.lock().unwrap().projects.iter().map(|(id, e)| (id.clone(), e.view.clone())).collect()
    }

    /// A cached issue, for prompts and for turning it into a task.
    pub fn cached_issue(&self, project_id: &str, number: u64) -> Option<Issue> {
        let inner = self.inner.lock().unwrap();
        inner.projects.get(project_id)?.view.issues.iter().find(|i| i.number == number).cloned()
    }

    /// Reads an issue from GitHub, falling back to the cached copy.
    pub fn issue(&self, project_id: &str, path: &Path, number: u64) -> Result<Issue, String> {
        let fresh = process::which("gh").ok_or_else(|| "GitHub CLI (gh) isn't installed".to_string()).and_then(|exe| {
            let repo = github_repo(path).ok_or("This project's remote isn't on GitHub")?;
            fetch_issue(&exe, path, &repo, number).map_err(|e| describe(&e))
        });
        match fresh {
            Ok(issue) => Ok(issue),
            Err(e) => self.cached_issue(project_id, number).ok_or(e),
        }
    }

    /// Asks the sync thread to run. `project` syncs that project now;
    /// otherwise projects older than a minute sync, which is what opening
    /// the panel does.
    pub fn request(&self, project: Option<String>) {
        if project.is_none() {
            self.inner.lock().unwrap().last_focus = Some(Instant::now());
        }
        if let Some(tx) = self.wake.lock().unwrap().as_ref() {
            let _ = tx.send(project.map_or(Wake::Focus, Wake::Project));
        }
    }

    /// Starts the sync thread.
    pub fn spawn(app: AppHandle) {
        let (tx, rx) = mpsc::channel();
        *app.state::<Arc<Github>>().wake.lock().unwrap() = Some(tx);
        let spawned = std::thread::Builder::new().name("pill-github".into()).spawn(move || {
            std::thread::sleep(STARTUP_DELAY);
            let github = app.state::<Arc<Github>>().inner().clone();
            github.cycle(&app, None, false);
            github.run(&app, rx);
        });
        if let Err(e) = spawned {
            log::error!("couldn't start GitHub sync: {e}");
        }
    }

    fn run(&self, app: &AppHandle, rx: Receiver<Wake>) {
        loop {
            // A short tick that only checks what is due; the cadence itself
            // lives in `cycle`.
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(Wake::Focus) => self.cycle(app, None, true),
                Ok(Wake::Project(id)) => self.cycle(app, Some(id), false),
                Err(RecvTimeoutError::Timeout) => self.cycle(app, None, false),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Picks the projects due for a sync and syncs them.
    ///
    /// `only` forces one project; `focus` syncs anything older than
    /// [`FOCUS_MIN_AGE`]; otherwise projects sync on their cadence or
    /// backoff.
    fn cycle(&self, app: &AppHandle, only: Option<String>, focus: bool) {
        let state = app.state::<AppState>();
        let runs = app.state::<Arc<Agents>>().list();
        let cutoff = chrono::Utc::now() - chrono::Duration::days(RUN_PR_WINDOW_DAYS);
        let mut jobs: Vec<Job> = {
            let ws = state.workspace.lock().unwrap();
            ws.snapshot()
                .projects
                .into_iter()
                .map(|p| {
                    let run_prs = runs
                        .iter()
                        .filter(|r| r.project_id == p.id)
                        .filter(|r| chrono::DateTime::parse_from_rfc3339(&r.started_at).is_ok_and(|t| t > cutoff))
                        .filter_map(|r| r.pr_url.clone())
                        .collect();
                    Job { project_id: p.id, path: p.path, show_issues: p.settings.github_issues, tasks: p.tasks, run_prs }
                })
                .collect()
        };

        let now = Instant::now();
        {
            let mut inner = self.inner.lock().unwrap();
            let live: HashSet<&str> = jobs.iter().map(|j| j.project_id.as_str()).collect();
            inner.projects.retain(|id, _| live.contains(id.as_str()));
            if let Some((until, _)) = &inner.paused_until {
                if *until > now {
                    return;
                }
                inner.paused_until = None;
            }
            let recent = inner.last_focus.is_some_and(|t| t.elapsed() < RECENT_USE);
            let cadence = if recent { ACTIVE_INTERVAL } else { IDLE_INTERVAL };
            jobs.retain(|j| {
                let e = inner.projects.entry(j.project_id.clone()).or_default();
                if let Some(id) = &only {
                    return id == &j.project_id;
                }
                let age = e.last_attempt.map(|t| now.duration_since(t));
                if e.next_at.is_some_and(|t| t > now) {
                    return false;
                }
                match age {
                    None => true,
                    Some(age) if focus => age >= FOCUS_MIN_AGE,
                    Some(age) => age >= cadence,
                }
            });
            if jobs.is_empty() {
                return;
            }
            for j in &jobs {
                if let Some(e) = inner.projects.get_mut(&j.project_id) {
                    e.view.syncing = true;
                    e.last_attempt = Some(now);
                }
            }
        }
        self.emit(app);

        let Some(exe) = process::which("gh") else {
            self.settle_all(app, &jobs, SyncKind::NoGh, "Install the GitHub CLI and run gh auth login");
            return;
        };

        let mut signed_in: HashMap<String, bool> = HashMap::new();
        let mut fetched: Vec<(Job, Fetched)> = Vec::new();
        for chunk in jobs.chunks(PARALLEL) {
            let results: Vec<Fetched> = std::thread::scope(|s| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|job| {
                        let exe = exe.clone();
                        let known = self.known_prs(&job.project_id);
                        s.spawn(move || fetch_project(&exe, job, &known))
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap_or(Fetched { repo: None, outcome: Err(GhError::Other("sync thread panicked".into())) })).collect()
            });
            let rate_limited = results.iter().any(|f| matches!(f.outcome, Err(GhError::RateLimited)));
            fetched.extend(chunk.iter().cloned().zip(results));
            if rate_limited {
                break;
            }
        }
        // Projects never reached because of a rate limit keep their old data.
        let reached: HashSet<String> = fetched.iter().map(|(j, _)| j.project_id.clone()).collect();

        for (job, result) in fetched {
            // A signed-out host fails every repo on it; confirm once per host
            // so the status says "signed out" rather than a raw error.
            let result = match (&result.outcome, &result.repo) {
                (Err(GhError::NotFound | GhError::Other(_)), Some(repo)) => {
                    let host = host_of(repo).to_string();
                    let ok = *signed_in.entry(host.clone()).or_insert_with(|| auth_ok(&exe, &host));
                    if ok { result } else { Fetched { repo: result.repo, outcome: Err(GhError::SignedOut) } }
                }
                _ => result,
            };
            self.apply(app, &job, result);
        }
        {
            let mut inner = self.inner.lock().unwrap();
            for j in &jobs {
                if !reached.contains(&j.project_id) {
                    if let Some(e) = inner.projects.get_mut(&j.project_id) {
                        e.view.syncing = false;
                    }
                }
            }
        }
        self.emit(app);
    }

    fn known_prs(&self, project_id: &str) -> HashMap<String, PrStatus> {
        self.inner.lock().unwrap().projects.get(project_id).map(|e| e.view.prs.clone()).unwrap_or_default()
    }

    /// Marks every job with the same non-GitHub outcome (no `gh` at all).
    fn settle_all(&self, app: &AppHandle, jobs: &[Job], kind: SyncKind, message: &str) {
        {
            let mut inner = self.inner.lock().unwrap();
            for j in jobs {
                let e = inner.projects.entry(j.project_id.clone()).or_default();
                if e.view.status.kind != kind {
                    log::info!("github sync for {}: {message}", j.path.display());
                }
                e.view.syncing = false;
                e.view.status = SyncStatus { kind, message: message.into(), synced_at: e.view.status.synced_at.clone(), retry_at: None };
            }
        }
        self.emit(app);
    }

    /// Stores one project's result and feeds it back into its tasks.
    fn apply(&self, app: &AppHandle, job: &Job, result: Fetched) {
        let now = Instant::now();
        let busy: HashSet<String> = app
            .state::<Arc<Agents>>()
            .list()
            .into_iter()
            .filter(|r| r.state.is_active())
            .map(|r| r.task_id)
            .collect();
        let mut updates = Vec::new();
        let mut merged_titles = Vec::new();
        {
            let mut inner = self.inner.lock().unwrap();
            let rate_hits = inner.rate_hits;
            let e = inner.projects.entry(job.project_id.clone()).or_default();
            e.view.syncing = false;
            e.view.repo = result.repo.as_deref().map(short_repo);
            let previous = e.view.status.kind;
            let mut pause = None;
            match result.outcome {
                Ok((issues, prs, closed)) => {
                    e.failures = 0;
                    e.next_at = None;
                    updates = plan_task_updates(&job.tasks, &prs, &closed, &busy, &e.applied_merges);
                    for u in &updates {
                        if u.status == Some(TaskStatus::Done) {
                            if let Some(t) = job.tasks.iter().find(|t| t.id == u.task_id) {
                                if let Some(url) = t.agent.pr_url.as_ref().filter(|url| prs.get(*url).is_some_and(|p| p.state == PrState::Merged)) {
                                    e.applied_merges.insert(url.clone());
                                    merged_titles.push((t.id.clone(), t.title.clone()));
                                }
                            }
                        }
                    }
                    let message = if job.show_issues {
                        match issues.len() {
                            0 => "No open issues assigned to you".to_string(),
                            1 => "1 open issue assigned to you".to_string(),
                            n => format!("{n} open issues assigned to you"),
                        }
                    } else {
                        "Issues hidden; PR status still syncs".to_string()
                    };
                    e.view.issues = if job.show_issues { issues } else { Vec::new() };
                    e.view.prs = prs;
                    e.view.status = SyncStatus { kind: SyncKind::Ok, message, synced_at: Some(now_rfc3339()), retry_at: None };
                }
                Err(err) => {
                    let (kind, message, retry) = match &err {
                        _ if result.repo.is_none() => (SyncKind::NotGithub, "The remote isn't on GitHub".to_string(), None),
                        GhError::SignedOut => (SyncKind::SignedOut, "gh isn't signed in for this host. Run gh auth login.".into(), None),
                        GhError::RateLimited => {
                            let wait = backoff(rate_hits + 3);
                            pause = Some(wait);
                            (SyncKind::RateLimited, "GitHub's rate limit was reached".into(), Some(wait))
                        }
                        GhError::NotFound => {
                            e.failures += 1;
                            let wait = backoff(e.failures);
                            (SyncKind::Error, "This gh login can't see the repository".into(), Some(wait))
                        }
                        GhError::IssuesDisabled => (SyncKind::Ok, "Issues are turned off for this repository".into(), None),
                        GhError::Other(msg) => {
                            e.failures += 1;
                            let wait = backoff(e.failures);
                            (SyncKind::Error, msg.clone(), Some(wait))
                        }
                    };
                    if kind != previous {
                        log::warn!("github sync for {}: {message}", job.path.display());
                    }
                    e.next_at = retry.map(|w| now + w);
                    let retry_at = retry.map(|w| (chrono::Utc::now() + chrono::Duration::from_std(w).unwrap_or(chrono::Duration::zero())).to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
                    let synced_at = if kind == SyncKind::Ok { Some(now_rfc3339()) } else { e.view.status.synced_at.clone() };
                    if kind == SyncKind::NotGithub {
                        e.view.issues.clear();
                        e.view.prs.clear();
                    }
                    e.view.status = SyncStatus { kind, message, synced_at, retry_at };
                }
            }
            if let Some(wait) = pause {
                inner.rate_hits += 1;
                inner.paused_until = Some((now + wait, job.project_id.clone()));
            } else if matches!(inner.projects.get(&job.project_id).map(|e| e.view.status.kind), Some(SyncKind::Ok)) {
                inner.rate_hits = 0;
            }
        }
        if updates.is_empty() {
            return;
        }
        let state = app.state::<AppState>();
        let changed = state.workspace.lock().unwrap().apply_task_sync(&job.project_id, &updates);
        match changed {
            Ok(true) => state.emit_snapshot(app),
            Ok(false) => {}
            Err(e) => log::warn!("github sync couldn't update tasks in {}: {e}", job.path.display()),
        }
        let project = job.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for (task_id, title) in merged_titles {
            let target = crate::notify::Target { run_id: None, task_id: Some(task_id), project_id: Some(job.project_id.clone()) };
            crate::toast::need(app, &format!("Merged: {title}"), "Moved to Done", &project, &crate::notify::link(&target), None);
        }
    }

    fn emit(&self, app: &AppHandle) {
        let _ = app.emit(GITHUB_EVENT, self.snapshot());
    }
}

fn auth_ok(exe: &Path, host: &str) -> bool {
    let mut cmd = process::tool(exe);
    cmd.args(["auth", "status", "--hostname", host]).env("GH_PROMPT_DISABLED", "1");
    process::run(cmd).is_ok()
}

/// Fetches one project's issues, PR statuses and linked issue states.
///
/// PRs already known to be merged or closed aren't asked about again; their
/// last status is kept so Done cards still show it.
fn fetch_project(exe: &Path, job: &Job, known: &HashMap<String, PrStatus>) -> Fetched {
    let Some(repo) = github_repo(&job.path) else {
        return Fetched { repo: None, outcome: Err(GhError::NotFound) };
    };
    let outcome = (|| {
        let issues = match list_issues(exe, &job.path, &repo) {
            Ok(list) => list,
            Err(GhError::IssuesDisabled) => Vec::new(),
            Err(e) => return Err(e),
        };

        let mut urls: Vec<String> = job
            .tasks
            .iter()
            .filter_map(|t| t.agent.pr_url.clone())
            .chain(job.run_prs.iter().cloned())
            .filter(|u| u.starts_with("https://"))
            .collect();
        urls.sort();
        urls.dedup();
        let mut prs = HashMap::new();
        for url in urls {
            match known.get(&url) {
                Some(done) if done.state != PrState::Open => {
                    prs.insert(url, done.clone());
                }
                _ => match pr_status(exe, &job.path, &url) {
                    Ok(status) => {
                        prs.insert(url, status);
                    }
                    Err(e @ (GhError::RateLimited | GhError::SignedOut)) => return Err(e),
                    // A deleted PR or one on another repo shouldn't fail the
                    // whole project; it just shows no status.
                    Err(e) => log::debug!("pr view {url}: {e:?}"),
                },
            }
        }

        let open: HashSet<u64> = issues.iter().map(|i| i.number).collect();
        let mut closed = HashMap::new();
        let mut lookups = 0;
        for t in &job.tasks {
            let Some(link) = &t.issue else { continue };
            if open.contains(&link.number) {
                closed.insert(link.number, false);
            } else if !link.closed && lookups < ISSUE_LOOKUPS {
                lookups += 1;
                match issue_closed(exe, &job.path, &repo, link.number) {
                    Ok(c) => {
                        closed.insert(link.number, c);
                    }
                    Err(e @ (GhError::RateLimited | GhError::SignedOut)) => return Err(e),
                    Err(e) => log::debug!("issue view #{}: {e:?}", link.number),
                }
            }
        }
        Ok((issues, prs, closed))
    })();
    Fetched { repo: Some(repo), outcome }
}

/// A label name as a Pill tag: lowercase, with spaces turned into dashes,
/// since tags are single words.
pub fn label_tag(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join("-").to_lowercase()
}

/// A [`GhError`] in words, for command errors.
pub fn describe(e: &GhError) -> String {
    match e {
        GhError::RateLimited => "GitHub's rate limit was reached. Try again in a few minutes.".into(),
        GhError::SignedOut => "gh isn't signed in. Run gh auth login in a terminal.".into(),
        GhError::NotFound => "That issue or repository wasn't found with this gh login".into(),
        GhError::IssuesDisabled => "Issues are turned off for this repository".into(),
        GhError::Other(m) => m.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentLink, IssueLink};

    /// Recorded from `gh issue list --json number,title,url,body,labels,updatedAt` (gh 2.93).
    const ISSUE_LIST: &str = r####"[{"body":"### Describe the feature or problem you’d like to solve\n\n`gh event listen` refuses every host except github.com.\n\n```console\n$ gh event listen --resource https://ghes.example.com/OWNER/REPO/pull/12\nevent subscriptions are only available on github.com\n```\n","labels":[{"id":"MDU6TGFiZWwxNjk4MDc2MTcz","name":"enhancement","description":"a request to improve CLI","color":"0dd8ac"},{"id":"LA_kwDODKw3uc8AAAABfZ1x2Q","name":"needs-triage","description":"needs to be reviewed","color":"e4e669"}],"number":15104,"title":"`gh event`: let the host answer, instead of refusing every host but github.com","updatedAt":"2026-11-19T08:46:55Z","url":"https://github.com/cli/cli/issues/15104"},{"body":"","labels":[],"number":15102,"title":"gh pr checkout from another repo","updatedAt":"2026-11-18T22:10:03Z","url":"https://github.com/cli/cli/issues/15102"}]"####;

    /// Recorded from `gh pr view --json …` on an open PR whose checks all passed.
    const PR_PASSING: &str = r#"{"isDraft":false,"number":15108,"reviewDecision":"REVIEW_REQUIRED","state":"OPEN","statusCheckRollup":[{"__typename":"CheckRun","completedAt":"2026-11-19T17:50:13Z","conclusion":"SUCCESS","detailsUrl":"https://github.com/cli/cli/actions/runs/44580805822/job/135760595507","name":"CodeQL-Build (go, manual, ./.github/codeql/codeql-config.yml)","startedAt":"2026-11-19T17:46:46Z","status":"COMPLETED","workflowName":"Code Scanning"},{"__typename":"CheckRun","completedAt":"2026-11-19T17:49:39Z","conclusion":"SUCCESS","detailsUrl":"https://github.com/cli/cli/actions/runs/44580805818/job/135760597412","name":"lint","startedAt":"2026-11-19T17:46:46Z","status":"COMPLETED","workflowName":"Lint"},{"__typename":"CheckRun","completedAt":"2026-11-19T17:46:44Z","conclusion":"SKIPPED","detailsUrl":"https://github.com/cli/cli/actions/runs/44580806364/job/135760606671","name":"close-from-default-branch / close-from-default-branch","startedAt":"2026-11-19T17:46:45Z","status":"COMPLETED","workflowName":"PR Triaging"}],"url":"https://github.com/cli/cli/pull/15108"}"#;

    /// Recorded from a merged, approved PR.
    const PR_MERGED: &str = r#"{"isDraft":false,"mergedAt":"2026-11-09T22:42:53Z","number":15000,"reviewDecision":"APPROVED","state":"MERGED","statusCheckRollup":[{"__typename":"CheckRun","completedAt":"2026-11-09T20:08:12Z","conclusion":"SUCCESS","detailsUrl":"https://github.com/cli/cli/actions/runs/42689441934/job/129404973145","name":"build (ubuntu-latest)","startedAt":"2026-11-09T20:04:43Z","status":"COMPLETED","workflowName":"Unit and Integration Tests"}],"url":"https://github.com/cli/cli/pull/15000"}"#;

    /// A draft PR with a running Actions job, a failed one and a pending
    /// legacy commit status (shapes from microsoft/vscode and kubernetes).
    const PR_MIXED: &str = r#"{"isDraft":true,"number":349314,"reviewDecision":"","state":"OPEN","statusCheckRollup":[{"__typename":"CheckRun","completedAt":"0001-01-01T00:00:00Z","conclusion":"","detailsUrl":"https://github.com/microsoft/vscode/actions/runs/44600564269/job/135830633276","name":"Analyze (javascript-typescript)","startedAt":"2026-11-19T19:39:37Z","status":"IN_PROGRESS","workflowName":"CodeQL"},{"__typename":"CheckRun","completedAt":"2026-11-19T19:41:00Z","conclusion":"FAILURE","detailsUrl":"https://github.com/microsoft/vscode/actions/runs/44600564270/job/135830633277","name":"Monaco Editor checks","startedAt":"2026-11-19T19:39:37Z","status":"COMPLETED","workflowName":"Monaco Editor checks"},{"__typename":"StatusContext","context":"pull-kubernetes-e2e-gce","startedAt":"2026-11-19T19:11:28Z","state":"PENDING","targetUrl":"https://prow.k8s.io/view/gs/kubernetes-ci-logs/pr-logs/pull/144361/pull-kubernetes-e2e-gce/2123494378562392064"}],"url":"https://github.com/microsoft/vscode/pull/349314"}"#;

    #[test]
    fn parses_issue_list() {
        let issues = parse_issues(ISSUE_LIST).unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].number, 15104);
        assert_eq!(issues[0].labels, vec![
            Label { name: "enhancement".into(), color: "0dd8ac".into() },
            Label { name: "needs-triage".into(), color: "e4e669".into() },
        ]);
        assert!(issues[0].body.contains("```console"));
        assert_eq!(issues[1].body, "");
        assert_eq!(issues[1].updated_at, "2026-11-18T22:10:03Z");
        assert_eq!(parse_issues("[]").unwrap(), Vec::new());
        assert!(parse_issues("not json").is_err());
    }

    #[test]
    fn parses_issue_state_including_pr_numbers() {
        assert_eq!(parse_issue_state(r#"{"number":15106,"state":"CLOSED"}"#).unwrap(), (15106, true));
        assert_eq!(parse_issue_state(r#"{"number":1,"state":"OPEN"}"#).unwrap(), (1, false));
        assert_eq!(parse_issue_state(r#"{"closedAt":"2026-11-09T22:42:53Z","number":15000,"state":"MERGED","stateReason":""}"#).unwrap(), (15000, true));
    }

    #[test]
    fn parses_pr_review_and_checks() {
        let p = parse_pr(PR_PASSING).unwrap();
        assert_eq!((p.number, p.state, p.draft), (15108, PrState::Open, false));
        assert_eq!(p.review, Review::Required);
        assert_eq!(serde_json::to_value(Review::Required).unwrap(), "review_required");
        assert_eq!((p.checks, p.passed, p.failed, p.pending), (Checks::Passing, 3, 0, 0));

        let m = parse_pr(PR_MERGED).unwrap();
        assert_eq!((m.state, m.review, m.checks), (PrState::Merged, Review::Approved, Checks::Passing));

        let x = parse_pr(PR_MIXED).unwrap();
        assert!(x.draft);
        assert_eq!(x.review, Review::None);
        assert_eq!((x.checks, x.passed, x.failed, x.pending), (Checks::Failing, 0, 1, 2));
    }

    #[test]
    fn pr_without_checks_or_with_null_rollup() {
        let none = parse_pr(r#"{"number":3,"url":"https://github.com/o/r/pull/3","state":"CLOSED","isDraft":false,"reviewDecision":"CHANGES_REQUESTED","statusCheckRollup":[]}"#).unwrap();
        assert_eq!((none.state, none.review, none.checks), (PrState::Closed, Review::ChangesRequested, Checks::None));
        let null = parse_pr(r#"{"number":3,"url":"u","state":"OPEN","statusCheckRollup":null,"reviewDecision":null}"#).unwrap();
        assert_eq!((null.checks, null.review), (Checks::None, Review::None));
        let pending = parse_pr(r#"{"number":4,"url":"u","state":"OPEN","statusCheckRollup":[{"__typename":"StatusContext","context":"ci","state":"EXPECTED"},{"__typename":"CheckRun","status":"COMPLETED","conclusion":"NEUTRAL","name":"x"}]}"#).unwrap();
        assert_eq!((pending.checks, pending.passed, pending.pending), (Checks::Pending, 1, 1));
    }

    #[test]
    fn repo_specs_need_host_owner_and_repo() {
        assert_eq!(repo_spec("github.com/cli/cli").as_deref(), Some("github.com/cli/cli"));
        assert_eq!(repo_spec("github.example.com/team/app").as_deref(), Some("github.example.com/team/app"));
        assert_eq!(repo_spec("github.com/cli"), None);
        assert_eq!(repo_spec("github.com/a/b/c"), None);
        assert_eq!(short_repo("github.com/cli/cli"), "cli/cli");
        assert_eq!(host_of("github.example.com/team/app"), "github.example.com");
    }

    #[test]
    fn closing_line_is_added_once() {
        assert_eq!(with_closing_line("## Summary\nFix it\n", 7), "## Summary\nFix it\n\nCloses #7\n");
        assert_eq!(with_closing_line("", 7), "Closes #7\n");
        let has = "Fixes #7.\n";
        assert_eq!(with_closing_line(has, 7), has);
        assert_eq!(with_closing_line("Resolves: #7", 7), "Resolves: #7");
        assert!(with_closing_line("Closes #70", 7).ends_with("Closes #7\n"));
        assert!(with_closing_line("See #7", 7).ends_with("Closes #7\n"));
    }

    #[test]
    fn classifies_gh_errors() {
        assert_eq!(classify_error("GraphQL: API rate limit exceeded for user ID 1."), GhError::RateLimited);
        assert_eq!(classify_error("You have exceeded a secondary rate limit"), GhError::RateLimited);
        assert_eq!(classify_error("To get started with GitHub CLI, please run:  gh auth login"), GhError::SignedOut);
        assert_eq!(classify_error("HTTP 401: Bad credentials (https://api.github.com/graphql)"), GhError::SignedOut);
        assert_eq!(classify_error("GraphQL: Could not resolve to a Repository with the name 'a/b'. (repository)"), GhError::NotFound);
        assert_eq!(classify_error("the 'a/b' repository has disabled issues"), GhError::IssuesDisabled);
        assert_eq!(classify_error("\n  error connecting to api.github.com\ncheck your internet connection"), GhError::Other("error connecting to api.github.com".into()));
    }

    #[test]
    fn labels_become_single_word_tags() {
        assert_eq!(label_tag("good first issue"), "good-first-issue");
        assert_eq!(label_tag("Bug"), "bug");
        assert_eq!(label_tag("  "), "");
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff(0), Duration::from_secs(120));
        assert_eq!(backoff(1), Duration::from_secs(120));
        assert_eq!(backoff(2), Duration::from_secs(240));
        assert_eq!(backoff(5), Duration::from_secs(32 * 60));
        assert_eq!(backoff(50), Duration::from_secs(60 * 60));
    }

    fn task(id: &str, status: TaskStatus, pr: Option<&str>, issue: Option<(u64, bool)>) -> Task {
        Task {
            id: id.into(),
            title: id.into(),
            notes: String::new(),
            status,
            priority: None,
            tags: Vec::new(),
            due: None,
            order: 1.0,
            agent_ready: false,
            pinned_notes: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
            agent: AgentLink { pr_url: pr.map(str::to_string), ..Default::default() },
            issue: issue.map(|(number, closed)| IssueLink { number, url: format!("https://github.com/o/r/issues/{number}"), closed, extra: Default::default() }),
            origin: Default::default(),
            extra: Default::default(),
        }
    }

    fn pr(url: &str, state: PrState) -> PrStatus {
        PrStatus { number: 1, url: url.into(), state, draft: false, review: Review::None, checks: Checks::None, passed: 0, failed: 0, pending: 0 }
    }

    #[test]
    fn merged_prs_move_tasks_to_done_once() {
        let url = "https://github.com/o/r/pull/1";
        let prs = HashMap::from([(url.to_string(), pr(url, PrState::Merged))]);
        let none = HashSet::new();
        let tasks = vec![task("a", TaskStatus::Review, Some(url), None), task("b", TaskStatus::Todo, Some(url), None)];
        let plan = plan_task_updates(&tasks, &prs, &HashMap::new(), &none, &none);
        assert_eq!(plan, vec![TaskSync { task_id: "a".into(), status: Some(TaskStatus::Done), issue_closed: None }]);
        let applied = HashSet::from([url.to_string()]);
        assert!(plan_task_updates(&tasks, &prs, &HashMap::new(), &none, &applied).is_empty());
        let open = HashMap::from([(url.to_string(), pr(url, PrState::Open))]);
        assert!(plan_task_updates(&tasks, &open, &HashMap::new(), &none, &none).is_empty());
    }

    #[test]
    fn closed_issues_are_marked_and_finish_idle_tasks() {
        let url = "https://github.com/o/r/pull/2";
        let open_pr = HashMap::from([(url.to_string(), pr(url, PrState::Open))]);
        let closed = HashMap::from([(5, true), (6, true), (7, true), (8, false)]);
        let busy = HashSet::from(["working".to_string()]);
        let none = HashSet::new();
        let tasks = vec![
            task("idle", TaskStatus::Todo, None, Some((5, false))),
            task("in-review", TaskStatus::Review, Some(url), Some((6, false))),
            task("working", TaskStatus::Doing, None, Some((7, false))),
            task("reopened", TaskStatus::Todo, None, Some((8, true))),
        ];
        let plan = plan_task_updates(&tasks, &open_pr, &closed, &busy, &none);
        assert_eq!(
            plan,
            vec![
                TaskSync { task_id: "idle".into(), status: Some(TaskStatus::Done), issue_closed: Some(true) },
                TaskSync { task_id: "in-review".into(), status: None, issue_closed: Some(true) },
                TaskSync { task_id: "working".into(), status: None, issue_closed: Some(true) },
                TaskSync { task_id: "reopened".into(), status: None, issue_closed: Some(false) },
            ]
        );
        // Already marked closed: nothing to do, so a task moved back by hand stays.
        let again = vec![task("idle", TaskStatus::Todo, None, Some((5, true)))];
        assert!(plan_task_updates(&again, &open_pr, &closed, &busy, &none).is_empty());
    }
}
