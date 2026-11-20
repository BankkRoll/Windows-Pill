//! Runs headless Claude Code on a task and turns the result into a draft PR.
//!
//! Pipeline per run (each step is logged to the run's activity stream):
//! 1. Preflight: the CLI installed and logged in, a git repo, a test command,
//!    plus a remote and `gh` when the project's ship mode needs them. Any
//!    failure stops before touching git.
//! 2. Isolate: `git fetch` (when there is a remote), then a fresh worktree on
//!    `<prefix><task>-<slug>` from the latest base branch. The user's working
//!    folder is never touched.
//! 3. Prompt: render the template and save `prompt.md`.
//! 4. Run Claude Code headless (stream-json, accept-edits, tool allow/deny
//!    lists, max turns) inside a Job Object, with `ANTHROPIC_API_KEY` removed
//!    so the subscription login is used. Tools outside the allowlist go to
//!    Pill's approval prompt.
//! 5. Verify: Pill re-runs the test command (and lint command, if set) itself.
//!    A failure goes back to the same session for a fix turn, as many times
//!    as the agent limits allow, before the run fails.
//! 6. Ship, per the project's ship mode: push only the agent branch and open
//!    a draft PR with `gh`, push the branch only, or keep it local.
//!
//! Plan mode inserts an approval stop between exploring and building.
//! Every state change moves the task's card ([`board_move`]) and, when the
//! user is needed, sends a notification ([`crate::notify`]). Hands-off tasks
//! are started by autopilot and merged PRs move their task to Done.
//! Run records live in `<repo>\.pill\runs\<run-id>\run.json` and survive
//! restarts; runs active when Pill quit are marked interrupted on launch.
//!
//! SECURITY: Pill puts no OS sandbox around Claude Code, and Claude Code's
//! own sandbox doesn't run natively on Windows. What a Claude run can do
//! unasked is exactly its tool allowlist ([`allowed_tools`]); the deny list
//! only blocks pushing, GitHub and the obvious download commands, not the
//! network. Tasks made from issues or suggested by plugins get an allowlist
//! without interpreters, package managers or the project's test commands,
//! and Codex runs them with its `untrusted` approval policy. Pill's own
//! verification still runs the test command on whatever the agent wrote.

use crate::approvals::{Approvals, Conversation, ToolAsk, Turn};
use crate::config::write_atomic;
use crate::model::{ShipMode, TaskStatus};
use crate::scan;
use crate::process::{self, git};
use crate::prompt::{self, PromptContext};
use crate::provider::{self, Phase, Provider, StreamParser};
use crate::state::AppState;
use crate::limits::UsageLimit;
use crate::stream::{self, Parsed, RunEvent};
use crate::workspace::{now_rfc3339, pill_dir, AgentUpdate, Project};
use crate::worktree;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Event with the full list of [`RunRecord`]s after any state change.
pub const RUNS_EVENT: &str = "pill://runs";
/// Event with one [`LogLine`] as it happens.
pub const RUN_LOG_EVENT: &str = "pill://run-log";

/// Most recent log lines kept in memory per run (the transcript keeps all).
const LOG_CAP: usize = 400;

// -----------------------------------------------------------------------------
// Records
// -----------------------------------------------------------------------------

/// Lifecycle of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued,
    Preparing,
    Running,
    /// Plan mode: waiting for the user to approve the plan.
    AwaitingApproval,
    Verifying,
    Shipping,
    Failed,
    /// A compared run finished with passing tests and waits to be picked.
    Ready,
    PrOpen,
    /// Shipped without a PR: the branch was pushed ([`ShipMode::Push`]).
    Pushed,
    /// Shipped locally: commits stay on the agent branch ([`ShipMode::Local`]).
    Committed,
    Stopped,
    Interrupted,
    /// Finished without changing code (research, a review, an answer). The
    /// agent's reply is the result, kept in [`RunRecord::answer`].
    Answered,
}

impl RunState {
    /// Whether a worker thread owns the run right now.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Preparing | Self::Running | Self::Verifying | Self::Shipping)
    }

    /// Whether the run finished its work and shipped it, by any [`ShipMode`].
    pub fn is_shipped(self) -> bool {
        matches!(self, Self::PrOpen | Self::Pushed | Self::Committed)
    }
}

/// Lines and files changed against the base branch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiffStat {
    /// Files touched.
    pub files: u32,
    /// Lines added.
    pub insertions: u32,
    /// Lines removed.
    pub deletions: u32,
}

/// Parses `git diff --shortstat` output such as
/// ` 3 files changed, 40 insertions(+), 2 deletions(-)`.
fn parse_shortstat(s: &str) -> DiffStat {
    let mut d = DiffStat::default();
    for part in s.split(',') {
        let n: u32 = part.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        if part.contains("file") {
            d.files = n;
        } else if part.contains("insertion") {
            d.insertions = n;
        } else if part.contains("deletion") {
            d.deletions = n;
        }
    }
    d
}

/// Test outcome from Pill's own verification.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TestsResult {
    /// `None` until the tests ran.
    pub passed: Option<bool>,
    /// Last lines of the test output.
    pub summary: Option<String>,
}

/// Contents of `run.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    /// Run id, also the name of its folder under `.pill\runs`.
    pub id: String,
    /// The task the run works on.
    pub task_id: String,
    /// Owning project; follows the project when its folder moves.
    pub project_id: String,
    /// The task's title when the run started, for notifications and the PR.
    #[serde(default)]
    pub task_title: String,
    pub state: RunState,
    /// Agent branch the run commits to, under the project's branch prefix.
    pub branch: String,
    /// Worktree folder the agent works in, under `.pill-worktrees`.
    pub worktree: PathBuf,
    /// Session (Claude) or thread (Codex) id, used to resume the run.
    #[serde(default)]
    pub claude_session_id: Option<String>,
    /// RFC 3339 time the run was created.
    pub started_at: String,
    /// RFC 3339 time the run last stopped working, if it has.
    #[serde(default)]
    pub ended_at: Option<String>,
    /// Pill's own test verdict on the latest attempt.
    #[serde(default)]
    pub tests: TestsResult,
    /// The draft pull request, once opened.
    #[serde(default)]
    pub pr_url: Option<String>,
    /// Why the run failed or stopped, in plain words.
    #[serde(default)]
    pub error: Option<String>,
    /// CLI doing the work; resumes always use the same one.
    #[serde(default)]
    pub provider: Provider,
    /// Started in plan mode: explore and plan, then wait for approval.
    #[serde(default)]
    pub plan_mode: bool,
    /// The plan awaiting (or having received) approval.
    #[serde(default)]
    pub plan: Option<String>,
    /// The agent's final reply when it finished without changing code.
    #[serde(default)]
    pub answer: Option<String>,
    /// Test command Pill found and ran because the project has none set.
    #[serde(default)]
    pub tests_detected: Option<String>,
    /// When the latest attempt began: a retry or an approved plan restarts
    /// the clock, so the duration shown is that attempt's, not idle time.
    #[serde(default)]
    pub resumed_at: Option<String>,
    /// Whether the worktree is on disk; refreshed whenever runs are listed.
    #[serde(default, skip_deserializing)]
    pub worktree_present: bool,
    /// Files the agent edited, relative to the worktree.
    #[serde(default)]
    pub files_changed: Vec<String>,
    /// Latest step or tool, for the live slot.
    #[serde(default)]
    pub activity: Option<String>,
    /// Agent turns used, as the CLI reported.
    #[serde(default)]
    pub turns: Option<u32>,
    /// Spend the CLI reported across all attempts, in US dollars.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// Model the CLI reported (Claude) or was given (Codex); `None` before it starts.
    #[serde(default)]
    pub model: Option<String>,
    /// Reasoning effort passed to the CLI, if any.
    #[serde(default)]
    pub effort: Option<String>,
    /// Model and effort picked when the run was started; empty inherits.
    #[serde(default)]
    pub choice: crate::models::ModelChoice,
    /// Shared by runs started together to compare providers on one task.
    #[serde(default)]
    pub compare_group: Option<String>,
    /// Size of the change against the base branch, once there is one.
    #[serde(default)]
    pub diff: Option<DiffStat>,
    /// Started on a hands-off task: the agent owns the task, moving its card
    /// and notifying only when the user is needed.
    #[serde(default)]
    pub hands_off: bool,
    /// Automatic fix turns used after failed checks, see
    /// [`crate::config::AgentLimits::fix_attempts`].
    #[serde(default)]
    pub fix_attempts: u32,
    /// Ref the branch started from (`origin/main`, `main`…), so verifying
    /// compares against the same commit even if the remote changes.
    #[serde(default)]
    pub base_ref: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One activity line, as emitted to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub run_id: String,
    pub at: String,
    #[serde(flatten)]
    pub event: RunEvent,
}

/// A preflight check and how to fix it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

// -----------------------------------------------------------------------------
// Manager
// -----------------------------------------------------------------------------

struct Live {
    record: RunRecord,
    project_path: PathBuf,
    log: VecDeque<LogLine>,
    cancel: Arc<AtomicBool>,
    #[cfg(windows)]
    job: Option<Arc<process::job::Job>>,
    /// Whether the worktree was on disk, and when that was checked.
    presence: Option<(bool, Instant)>,
}

/// How long a worktree's presence on disk is trusted before it's checked
/// again. Pill drops the cache itself whenever it adds or removes one; this
/// only bounds how late a folder the user deleted shows up.
const PRESENCE_TTL: Duration = Duration::from_secs(5);

/// Shortest gap between two runs lists sent for activity-only changes.
const ACTIVITY_EMIT_GAP: Duration = Duration::from_millis(250);

/// When the runs list was last sent, and whether a send is already due.
#[derive(Default)]
struct EmitClock {
    last: Option<Instant>,
    scheduled: bool,
}

/// What a worker should do when it starts.
#[derive(Debug, Clone)]
enum Work {
    Fresh,
    ApprovePlan(String),
    Retry(String),
}

/// All runs (live and historical) plus the queue enforcing the parallel cap.
#[derive(Default)]
pub struct Agents {
    runs: Mutex<HashMap<String, Live>>,
    queue: Mutex<VecDeque<(String, Work)>>,
    active: Mutex<usize>,
    /// Compared runs picked to ship; from then on they finish like single runs.
    picked: Mutex<HashSet<String>>,
    /// Providers whose usage limit was hit, with when queued work may resume.
    paused: Mutex<HashMap<Provider, Pause>>,
    emits: Mutex<EmitClock>,
}

/// A provider held back by its usage limit.
#[derive(Debug, Clone)]
struct Pause {
    /// Unix seconds when queued runs may start again.
    until: i64,
    limit: UsageLimit,
}

/// Whether `after` differs from `before` in its activity line and nothing
/// else.
fn only_activity_changed(before: RunRecord, after: &RunRecord) -> bool {
    before.activity != after.activity && RunRecord { activity: after.activity.clone(), ..before } == *after
}

/// One taken parallel-run slot, given back when the worker ends.
///
/// NOTE: a drop guard, so a worker that panics (dev builds unwind) still
/// frees its slot instead of shrinking the cap for the rest of the session.
struct Slot<'a>(&'a Mutex<usize>);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        let mut active = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *active = active.saturating_sub(1);
    }
}

/// Index of the first queued run whose provider isn't paused.
fn next_runnable(queued: &[Provider], paused: &[Provider]) -> Option<usize> {
    queued.iter().position(|p| !paused.contains(p))
}

fn run_dir(project_path: &Path, run_id: &str) -> PathBuf {
    pill_dir(project_path).join("runs").join(run_id)
}

/// Whether a run record read from `.pill\runs\<folder>\run.json` names
/// itself and its task with valid ids, and lives in its own folder.
///
/// SECURITY: run records can be committed to a repository; their ids are
/// joined into paths that [`Agents::delete`] removes.
fn record_ids_ok(record: &RunRecord, folder: &str) -> bool {
    crate::model::is_valid_id(&record.id) && record.id == folder && crate::model::is_valid_id(&record.task_id)
}

/// Whether `branch` is one Pill made for a run of `task_id`, so deleting or
/// pushing it can't touch the user's own branches.
///
/// SECURITY: the branch name comes from `run.json`, which a repository can
/// commit; without this a record naming `main` would delete it.
pub(crate) fn is_agent_branch(project: &Project, task_id: &str, branch: &str) -> bool {
    let own = format!("{}{task_id}-", project.settings.agent_branch_prefix());
    crate::model::is_valid_id(task_id) && branch.starts_with(&own) && branch != project.base_branch && !branch.contains("..")
}

fn worktree_path(project: &Project, task_id: &str) -> PathBuf {
    worktree::path_for(&project.path, task_id)
}

/// A fresh run id such as `r_20261119_211000_8f2k1_x3k9a`, with `_<variant>`
/// appended for a compared run. The random part keeps two runs started in
/// the same second apart; the task part is capped so the id stays valid.
fn new_run_id(task_id: &str, variant: Option<&str>) -> String {
    let task: String = task_id.trim_start_matches("t_").chars().take(20).collect();
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let id = format!("r_{stamp}_{task}_{}", crate::workspace::random_token());
    match variant {
        Some(v) => format!("{id}_{v}"),
        None => id,
    }
}

/// A fresh comparison group id, unique to the task and the moment.
fn new_group_id(task_id: &str) -> String {
    let task: String = task_id.trim_start_matches("t_").chars().take(20).collect();
    format!("g_{task}_{}_{}", chrono::Local::now().format("%Y%m%d_%H%M%S"), crate::workspace::random_token())
}

impl Agents {
    /// Loads run history for every project and marks runs that were active
    /// when Pill last quit as interrupted. Returns the ids of those runs, so
    /// [`Agents::settle_interrupted`] can put their tasks back.
    pub fn load(&self, projects: &[(String, PathBuf)]) -> Vec<String> {
        self.sync_projects(projects).1
    }

    /// Brings run history in line with the projects Pill can see now.
    ///
    /// Called at launch and after every rescan. A project folder that was
    /// renamed or moved shows up under a new path (and id); its runs follow
    /// it instead of pointing at the old location. A folder seen for the
    /// first time brings its run history with it. Runs on disk that claim
    /// to be active but have no worker are marked interrupted.
    ///
    /// Returns the repositories whose worktrees need their links to the moved
    /// repository repaired, and the ids of runs newly marked interrupted.
    pub fn sync_projects(&self, projects: &[(String, PathBuf)]) -> (Vec<PathBuf>, Vec<String>) {
        let mut moved = Vec::new();
        let mut interrupted = Vec::new();
        let mut runs = self.runs.lock().unwrap();
        for (project_id, path) in projects {
            let Ok(entries) = fs::read_dir(pill_dir(path).join("runs")) else { continue };
            for entry in entries.flatten() {
                let file = entry.path().join("run.json");
                let Ok(text) = fs::read_to_string(&file) else { continue };
                let Ok(mut record) = serde_json::from_str::<RunRecord>(&text) else { continue };
                if !record_ids_ok(&record, &entry.file_name().to_string_lossy()) {
                    log::warn!("skipping {}: its run or task id isn't valid", file.display());
                    continue;
                }
                provider::adopt_legacy_codex_home(&entry.path(), &record.id);
                if let Some(live) = runs.get_mut(&record.id) {
                    if !worktree::same_path(&live.project_path, path) && !live.project_path.is_dir() {
                        log::info!("run {} follows its project to {}", record.id, path.display());
                        live.project_path = path.clone();
                        live.record.project_id = project_id.clone();
                        if !moved.iter().any(|m: &PathBuf| worktree::same_path(m, path)) {
                            moved.push(path.clone());
                        }
                    }
                    continue;
                }
                record.project_id = project_id.clone();
                if record.state.is_active() {
                    record.state = RunState::Interrupted;
                    record.error = Some("Pill quit while this run was in progress".into());
                    record.ended_at.get_or_insert_with(now_rfc3339);
                    if let Ok(bytes) = serde_json::to_vec_pretty(&record) {
                        let _ = write_atomic(&file, &bytes);
                    }
                    interrupted.push(record.id.clone());
                }
                runs.insert(
                    record.id.clone(),
                    Live {
                        record,
                        project_path: path.clone(),
                        log: VecDeque::new(),
                        cancel: Arc::new(AtomicBool::new(false)),
                        #[cfg(windows)]
                        job: None,
                        presence: None,
                    },
                );
            }
        }
        (moved, interrupted)
    }

    /// Puts the tasks of runs that were cut off by a quit back in To do, with
    /// the reason on the card. No toast: the user just started Pill and the
    /// notch shows the failure.
    pub fn settle_interrupted(&self, app: &AppHandle, run_ids: &[String]) {
        for id in run_ids {
            if let Some(record) = self.get(id) {
                self.place_task(app, &record);
            }
        }
    }

    /// [`Self::sync_projects`] after a rescan: settles runs found cut off,
    /// repairs worktree links of moved repositories in the background and
    /// tells the UI.
    pub fn after_rescan(&self, app: &AppHandle, projects: &[(String, PathBuf)]) {
        let (moved, interrupted) = self.sync_projects(projects);
        self.settle_interrupted(app, &interrupted);
        if !moved.is_empty() {
            std::thread::spawn(move || {
                for repo in moved {
                    if let Err(e) = git(&repo, &["worktree", "repair"]) {
                        log::warn!("repairing worktrees of {}: {e}", repo.display());
                    }
                }
            });
        }
        self.emit_runs(app);
    }

    /// All runs, newest first.
    pub fn list(&self) -> Vec<RunRecord> {
        let mut v: Vec<RunRecord> = {
            let mut runs = self.runs.lock().unwrap();
            runs.values_mut()
                .map(|l| {
                    // PERF: every run update sends the whole list; stat each
                    // worktree at most once per PRESENCE_TTL, not per update.
                    let present = match l.presence {
                        Some((present, at)) if at.elapsed() < PRESENCE_TTL => present,
                        _ => {
                            let present = l.record.worktree.is_dir();
                            l.presence = Some((present, Instant::now()));
                            present
                        }
                    };
                    RunRecord { worktree_present: present, ..l.record.clone() }
                })
                .collect()
        };
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        v
    }

    /// Drops the cached worktree presence of a run whose worktree Pill just
    /// added or removed, so the next list reports it right away.
    fn recheck_worktree(&self, run_id: &str) {
        if let Some(live) = self.runs.lock().unwrap().get_mut(run_id) {
            live.presence = None;
        }
    }

    /// Sends the runs list to the UI now.
    fn emit_runs(&self, app: &AppHandle) {
        self.emits.lock().unwrap().last = Some(Instant::now());
        let _ = app.emit(RUNS_EVENT, self.list());
    }

    /// Sends the runs list for an activity-only change: right away when the
    /// last send was a while ago, else once at the end of the gap, so a
    /// stream of tool events costs a few sends a second, not one each.
    fn emit_runs_soon(&self, app: &AppHandle) {
        let wait = {
            let mut clock = self.emits.lock().unwrap();
            if clock.scheduled {
                return;
            }
            let wait = clock.last.map_or(Duration::ZERO, |last| ACTIVITY_EMIT_GAP.saturating_sub(last.elapsed()));
            clock.scheduled = !wait.is_zero();
            wait
        };
        if wait.is_zero() {
            self.emit_runs(app);
            return;
        }
        let handle = app.clone();
        let spawned = std::thread::Builder::new().name("pill-runs-emit".into()).spawn(move || {
            std::thread::sleep(wait);
            let agents = handle.state::<Arc<Agents>>();
            agents.emits.lock().unwrap().scheduled = false;
            agents.emit_runs(&handle);
        });
        if spawned.is_err() {
            self.emits.lock().unwrap().scheduled = false;
            self.emit_runs(app);
        }
    }

    /// Activity lines for a run: in memory while live, else rebuilt from the transcript.
    pub fn log(&self, run_id: &str) -> Vec<LogLine> {
        let runs = self.runs.lock().unwrap();
        let Some(live) = runs.get(run_id) else { return Vec::new() };
        if !live.log.is_empty() {
            return live.log.iter().cloned().collect();
        }
        let dir = run_dir(&live.project_path, run_id);
        if let Ok(file) = File::open(dir.join("activity.jsonl")) {
            return BufReader::new(file).lines().map_while(Result::ok).filter_map(|l| serde_json::from_str(&l).ok()).collect();
        }
        // NOTE: runs from before activity.jsonl existed only have the raw
        // CLI stream; Pill's own steps for them are gone.
        let Ok(file) = File::open(dir.join("transcript.jsonl")) else { return Vec::new() };
        let mut parser = StreamParser::new(live.record.provider, &live.record.worktree);
        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .flat_map(|l| parser.feed(&l))
            .filter_map(|p| match p {
                Parsed::Event(event) => Some(LogLine { run_id: run_id.to_string(), at: String::new(), event }),
                _ => None,
            })
            .collect()
    }

    /// Everything the agent was told: the first prompt and, after a
    /// `---` rule, each follow-up (fix turns, retries, plan approvals).
    pub fn prompt(&self, run_id: &str) -> Option<String> {
        let path = {
            let runs = self.runs.lock().unwrap();
            let live = runs.get(run_id)?;
            run_dir(&live.project_path, run_id).join("prompt.md")
        };
        fs::read_to_string(path).ok()
    }

    /// Record of one run.
    pub fn get(&self, run_id: &str) -> Option<RunRecord> {
        self.runs.lock().unwrap().get(run_id).map(|l| l.record.clone())
    }

    fn update(&self, app: &AppHandle, run_id: &str, edit: impl FnOnce(&mut RunRecord)) {
        let (file, record, repo_present, activity_only) = {
            let mut runs = self.runs.lock().unwrap();
            let Some(live) = runs.get_mut(run_id) else { return };
            let before = live.record.clone();
            edit(&mut live.record);
            let activity_only = only_activity_changed(before, &live.record);
            (run_dir(&live.project_path, run_id).join("run.json"), live.record.clone(), live.project_path.is_dir(), activity_only)
        };
        // PERF: while a run works, its activity changes with every tool call.
        // That line only matters live: finishing rewrites it, and a run cut
        // off by a quit is marked interrupted on the next launch anyway.
        if activity_only && record.state.is_active() {
            self.emit_runs_soon(app);
            return;
        }
        // A run whose repository vanished keeps going in memory only;
        // writing would recreate the deleted folder just to hold run.json.
        if let Some(bytes) = repo_present.then(|| serde_json::to_vec_pretty(&record).ok()).flatten() {
            if let Err(e) = write_atomic(&file, &bytes) {
                log::warn!("saving {}: {e}", file.display());
            }
        }
        self.emit_runs(app);
    }

    fn log_event(&self, app: &AppHandle, run_id: &str, event: RunEvent) {
        let line = LogLine { run_id: run_id.to_string(), at: now_rfc3339(), event };
        let dir = {
            let mut runs = self.runs.lock().unwrap();
            runs.get_mut(run_id).map(|live| {
                if live.log.len() >= LOG_CAP {
                    live.log.pop_front();
                }
                live.log.push_back(line.clone());
                run_dir(&live.project_path, run_id)
            })
        };
        if let (Some(dir), Ok(json)) = (dir, serde_json::to_string(&line)) {
            let _ = fs::create_dir_all(&dir)
                .and_then(|_| OpenOptions::new().create(true).append(true).open(dir.join("activity.jsonl")))
                .and_then(|mut f| writeln!(f, "{json}"));
        }
        let _ = app.emit(RUN_LOG_EVENT, line);
    }

    fn step(&self, app: &AppHandle, run_id: &str, text: impl Into<String>) {
        let text = text.into();
        log::info!("run {run_id}: {text}");
        self.update(app, run_id, |r| r.activity = Some(text.clone()));
        self.log_event(app, run_id, RunEvent::Step { text });
    }

    /// Creates a run for a task and queues it.
    pub fn start(
        &self,
        app: &AppHandle,
        project_id: &str,
        task_id: &str,
        plan: Option<bool>,
        choice: crate::models::RunChoice,
    ) -> Result<RunRecord, String> {
        let run = self.new_run(app, project_id, task_id, plan, choice, None)?;
        Ok(self.launch(app, vec![run])?.remove(0))
    }

    /// Starts the same task once per choice, each on its own branch and
    /// worktree, to compare providers or models. The runs stop at
    /// [`RunState::Ready`] instead of opening PRs; [`Agents::pick`] ships one.
    pub fn compare(
        &self,
        app: &AppHandle,
        project_id: &str,
        task_id: &str,
        choices: Vec<crate::models::RunChoice>,
    ) -> Result<Vec<RunRecord>, String> {
        if choices.len() < 2 {
            return Err("Pick at least two setups to compare".into());
        }
        let group = new_group_id(task_id);
        let runs = choices
            .into_iter()
            .enumerate()
            .map(|(i, choice)| self.new_run(app, project_id, task_id, Some(false), choice, Some((group.clone(), i))))
            .collect::<Result<Vec<_>, String>>()?;
        self.launch(app, runs)
    }

    /// Registers new runs of one task and queues them.
    ///
    /// The check that no run of the task is active and the insert happen
    /// under one lock, so a double click, or a click racing autopilot, can't
    /// start the task twice.
    fn launch(&self, app: &AppHandle, new: Vec<(RunRecord, PathBuf)>) -> Result<Vec<RunRecord>, String> {
        {
            let mut runs = self.runs.lock().unwrap();
            if new.iter().any(|(r, _)| runs.values().any(|l| l.record.task_id == r.task_id && l.record.state.is_active())) {
                return Err("An agent is already working on this task".into());
            }
            for (record, project_path) in &new {
                runs.insert(
                    record.id.clone(),
                    Live {
                        record: record.clone(),
                        project_path: project_path.clone(),
                        log: VecDeque::new(),
                        cancel: Arc::new(AtomicBool::new(false)),
                        #[cfg(windows)]
                        job: None,
                        presence: None,
                    },
                );
            }
        }
        if let Some(e) = new.iter().find_map(|(r, path)| fs::create_dir_all(run_dir(path, &r.id)).err()) {
            let mut runs = self.runs.lock().unwrap();
            for (r, _) in &new {
                runs.remove(&r.id);
            }
            return Err(format!("Couldn't create the run folder: {e}"));
        }
        let mut started = Vec::with_capacity(new.len());
        for (record, _) in new {
            self.update(app, &record.id, |_| {});
            self.claim_task(app, &record);
            self.enqueue(app, record.id.clone(), Work::Fresh);
            started.push(record);
        }
        Ok(started)
    }

    /// The record of a new run of `task_id`, not yet registered.
    fn new_run(
        &self,
        app: &AppHandle,
        project_id: &str,
        task_id: &str,
        plan: Option<bool>,
        choice: crate::models::RunChoice,
        compare: Option<(String, usize)>,
    ) -> Result<(RunRecord, PathBuf), String> {
        if !choice.is_valid() {
            return Err("That model or effort name isn't valid".into());
        }
        let state = app.state::<AppState>();
        let (project, task) = state.workspace.lock().unwrap().agent_context(project_id, task_id)?;
        let provider = choice.provider.unwrap_or(project.settings.provider);
        // Compared runs get their own branch and worktree, named after the
        // setup, so they never touch each other's files.
        let variant = compare.as_ref().map(|(_, i)| format!("{}-{}", provider.slug(), i + 1));
        let id = new_run_id(task_id, variant.as_deref());
        let base_name = prompt::branch_name(&project.settings.agent_branch_prefix(), &task);
        let branch = match &variant {
            Some(v) => format!("{base_name}-{v}"),
            None => base_name,
        };
        let worktree = match &variant {
            Some(v) => worktree_path(&project, &format!("{}-{v}", task.id)),
            None => worktree_path(&project, &task.id),
        };
        let hands_off = task.agent_ready && compare.is_none();
        let record = RunRecord {
            id: id.clone(),
            task_id: task.id.clone(),
            project_id: project.id.clone(),
            task_title: task.title.clone(),
            state: RunState::Queued,
            branch,
            worktree,
            claude_session_id: None,
            started_at: now_rfc3339(),
            ended_at: None,
            tests: TestsResult::default(),
            pr_url: None,
            error: None,
            plan_mode: plan.unwrap_or(project.settings.plan_mode),
            provider,
            plan: None,
            answer: None,
            tests_detected: None,
            resumed_at: None,
            worktree_present: false,
            files_changed: Vec::new(),
            activity: Some("Queued".into()),
            turns: None,
            cost_usd: None,
            model: None,
            effort: None,
            choice: choice.model_choice(),
            compare_group: compare.map(|(g, _)| g),
            diff: None,
            hands_off,
            fix_attempts: 0,
            base_ref: None,
            extra: Map::new(),
        };
        Ok((record, project.path))
    }

    /// Approves a plan (optionally edited) and continues the run.
    pub fn approve(&self, app: &AppHandle, run_id: &str, plan: String) -> Result<(), String> {
        let r = self.get(run_id).ok_or("Unknown run")?;
        if r.state != RunState::AwaitingApproval {
            return Err("This run isn't waiting for approval".into());
        }
        self.update(app, run_id, |r| {
            r.plan = Some(plan.clone());
            r.state = RunState::Queued;
            r.error = None;
            r.resumed_at = Some(now_rfc3339());
        });
        self.claim_task(app, &r);
        let said = if r.plan.as_deref().map(str::trim) == Some(plan.trim()) {
            "Approved the plan".to_string()
        } else {
            format!("Approved the plan with edits:\n\n{}", plan.trim())
        };
        self.log_event(app, run_id, RunEvent::User { text: said });
        self.enqueue(app, run_id.to_string(), Work::ApprovePlan(plan));
        Ok(())
    }

    /// Resumes the same Claude session with feedback and verifies again.
    pub fn retry(&self, app: &AppHandle, run_id: &str, feedback: String) -> Result<(), String> {
        let r = self.get(run_id).ok_or("Unknown run")?;
        if r.state.is_active() {
            return Err("This run is still in progress".into());
        }
        if r.claude_session_id.is_none() || !r.worktree.is_dir() {
            return Err("Nothing to resume: the session or worktree is gone. Start a new run instead.".into());
        }
        self.update(app, run_id, |r| {
            r.state = RunState::Queued;
            r.error = None;
            r.ended_at = None;
            r.resumed_at = Some(now_rfc3339());
            r.tests = TestsResult::default();
            r.fix_attempts = 0;
        });
        self.claim_task(app, &r);
        let said = if feedback.trim().is_empty() { "Try again".to_string() } else { feedback.trim().to_string() };
        self.log_event(app, run_id, RunEvent::User { text: said });
        self.enqueue(app, run_id.to_string(), Work::Retry(feedback));
        Ok(())
    }

    /// Stops a run, killing Claude and every process it started.
    pub fn stop(&self, app: &AppHandle, run_id: &str) -> Result<(), String> {
        let mut was_queued = false;
        {
            let mut runs = self.runs.lock().unwrap();
            let live = runs.get_mut(run_id).ok_or("Unknown run")?;
            live.cancel.store(true, Ordering::SeqCst);
            #[cfg(windows)]
            if let Some(job) = &live.job {
                job.kill();
            }
            if live.record.state == RunState::Queued {
                was_queued = true;
            }
        }
        self.queue.lock().unwrap().retain(|(id, _)| id != run_id);
        let finished = was_queued || self.get(run_id).is_some_and(|r| r.state == RunState::AwaitingApproval);
        if finished {
            self.finish(app, run_id, RunState::Stopped, Some("Stopped".into()));
        }
        Ok(())
    }

    /// Removes a run's worktree (and its local branch when no PR uses it).
    pub fn cleanup(&self, app: &AppHandle, run_id: &str) -> Result<(), String> {
        let (record, project_path) = {
            let runs = self.runs.lock().unwrap();
            let live = runs.get(run_id).ok_or("Unknown run")?;
            (live.record.clone(), live.project_path.clone())
        };
        if record.state.is_active() {
            return Err("Stop the run before cleaning up".into());
        }
        worktree::remove(&project_path, &record.worktree)?;
        self.recheck_worktree(run_id);
        self.drop_branch(app, &record, &project_path);
        provider::remove_private_state(run_id);
        self.step(app, run_id, "Worktree cleaned up");
        Ok(())
    }

    /// Deletes a run's local branch unless it is the shipped result or not
    /// one Pill made ([`is_agent_branch`]).
    fn drop_branch(&self, app: &AppHandle, record: &RunRecord, repo: &Path) {
        // NOTE: Pushed and Committed runs have no PR but their branch *is* the
        // result; deleting it would throw the work away.
        if record.pr_url.is_some() || record.state.is_shipped() || !repo.is_dir() {
            return;
        }
        let project = app.state::<AppState>().workspace.lock().unwrap().project(&record.project_id).cloned();
        match project {
            Some(p) if is_agent_branch(&p, &record.task_id, &record.branch) => {
                let _ = git(repo, &["branch", "-D", "--end-of-options", &record.branch]);
            }
            _ => log::warn!("run {}: leaving branch {:?}, it isn't an agent branch of this project", record.id, record.branch),
        }
    }

    /// Deletes a finished run: its worktree, its branch unless the branch is
    /// the shipped result, and its history in `.pill/runs`. The task stays.
    pub fn delete(&self, app: &AppHandle, run_id: &str) -> Result<(), String> {
        let (record, project_path) = {
            let runs = self.runs.lock().unwrap();
            let live = runs.get(run_id).ok_or("Unknown run")?;
            (live.record.clone(), live.project_path.clone())
        };
        if record.state.is_active() {
            return Err("Stop the run before deleting it".into());
        }
        if record.worktree.exists() {
            worktree::remove(&project_path, &record.worktree)?;
        }
        self.drop_branch(app, &record, &project_path);
        provider::remove_private_state(run_id);
        let dir = run_dir(&project_path, run_id);
        if dir.is_dir() {
            fs::remove_dir_all(&dir).map_err(|e| format!("Couldn't delete the run's history: {e}"))?;
        }
        self.runs.lock().unwrap().remove(run_id);
        self.emit_runs(app);
        Ok(())
    }

    /// Deletes every failed, stopped or interrupted run, optionally only in
    /// one project. Returns how many went.
    pub fn clear_failed(&self, app: &AppHandle, project_id: Option<&str>) -> usize {
        let ids: Vec<String> = self
            .list()
            .into_iter()
            .filter(|r| matches!(r.state, RunState::Failed | RunState::Stopped | RunState::Interrupted))
            .filter(|r| project_id.is_none_or(|p| r.project_id == p))
            .map(|r| r.id)
            .collect();
        ids.iter().filter(|id| self.delete(app, id).map_err(|e| log::warn!("deleting run {id}: {e}")).is_ok()).count()
    }

    /// Queues work for a run. A Stop from before this point no longer
    /// applies; one that lands after it, even before the worker starts,
    /// cancels this work.
    fn enqueue(&self, app: &AppHandle, run_id: String, work: Work) {
        if let Some(live) = self.runs.lock().unwrap().get(&run_id) {
            live.cancel.store(false, Ordering::SeqCst);
        }
        self.queue.lock().unwrap().push_back((run_id, work));
        self.pump(app);
    }

    /// Holds back queued runs and autopilot for a provider until its usage
    /// limit resets. Says so once per limit, not once per failed run.
    pub fn pause(&self, app: &AppHandle, limit: &UsageLimit) {
        let now = chrono::Local::now();
        let until = limit.wait_until(now);
        let fresh = {
            let mut paused = self.paused.lock().unwrap();
            let fresh = paused.get(&limit.provider).is_none_or(|p| p.until <= now.timestamp());
            paused.insert(limit.provider, Pause { until, limit: limit.clone() });
            fresh
        };
        log::warn!("{}; holding queued {} runs", limit.describe(), limit.provider.label());
        if fresh {
            let when = chrono::DateTime::from_timestamp(until, 0)
                .map(|t| t.with_timezone(&chrono::Local).format("%a %H:%M").to_string())
                .unwrap_or_default();
            crate::toast::message(app, &limit.describe(), &format!("Queued {} runs wait until {when}.", limit.provider.label()));
        }
        self.pump(app);
    }

    /// Providers still under a usage limit; expired pauses are dropped.
    fn paused_providers(&self) -> Vec<(Provider, Pause)> {
        let now = chrono::Utc::now().timestamp();
        let mut paused = self.paused.lock().unwrap();
        paused.retain(|_, p| p.until > now);
        paused.iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    /// Starts queued runs while under the parallel cap, skipping providers
    /// held back by a usage limit.
    fn pump(&self, app: &AppHandle) {
        let cap = app.state::<AppState>().config.read().unwrap().agent.max_parallel_runs as usize;
        let paused = self.paused_providers();
        let paused_kinds: Vec<Provider> = paused.iter().map(|(p, _)| *p).collect();
        loop {
            let next = {
                let mut active = self.active.lock().unwrap();
                if *active >= cap {
                    break;
                }
                let mut queue = self.queue.lock().unwrap();
                let providers: Vec<Provider> = {
                    let runs = self.runs.lock().unwrap();
                    queue.iter().map(|(id, _)| runs.get(id).map(|l| l.record.provider).unwrap_or_default()).collect()
                };
                let Some(i) = next_runnable(&providers, &paused_kinds) else { break };
                let Some(item) = queue.remove(i) else { break };
                *active += 1;
                item
            };
            let (run_id, work) = next;
            let handle = app.clone();
            let spawned = std::thread::Builder::new().name(format!("agent-{run_id}")).spawn(move || {
                let agents = handle.state::<Arc<Agents>>().inner().clone();
                {
                    let _slot = Slot(&agents.active);
                    let worked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| agents.work(&handle, &run_id, work)));
                    if worked.is_err() {
                        log::error!("run {run_id} panicked");
                        agents.finish(&handle, &run_id, RunState::Failed, Some("Pill hit an internal error during this run".into()));
                    }
                }
                agents.pump(&handle);
            });
            if let Err(e) = spawned {
                *self.active.lock().unwrap() -= 1;
                log::error!("couldn't start agent thread: {e}");
            }
        }
        let queued: Vec<String> = self.queue.lock().unwrap().iter().map(|(id, _)| id.clone()).collect();
        for (i, id) in queued.iter().enumerate() {
            let provider = self.get(id).map(|r| r.provider).unwrap_or_default();
            let text = match paused.iter().find(|(p, _)| *p == provider) {
                Some((_, pause)) => format!("Waiting: {}", pause.limit.describe()),
                None => format!("Queued · {i} ahead (limit {cap} at once)"),
            };
            self.update(app, id, |r| r.activity = Some(text));
        }
    }

    /// Number of runs running or waiting in the queue.
    pub fn busy(&self) -> usize {
        self.runs.lock().unwrap().values().filter(|l| l.record.state.is_active()).count()
    }

    /// Starts hands-off tasks while the autopilot window allows and the
    /// parallel cap has room. A task is skipped while a run of it is working,
    /// waits on a plan or waits to be picked from a comparison, and while a
    /// failed, stopped or interrupted run holds it back (`agent.blocked`):
    /// a failure never loops and burns usage, the user decides what's next.
    pub fn autopilot_tick(&self, app: &AppHandle) {
        let state = app.state::<AppState>();
        let (autopilot, cap) = {
            let c = state.config.read().unwrap();
            (c.autopilot.clone(), c.agent.max_parallel_runs as usize)
        };
        let now = chrono::Local::now();
        let minutes = now.format("%H").to_string().parse::<u32>().unwrap_or(0) * 60
            + now.format("%M").to_string().parse::<u32>().unwrap_or(0);
        // Queued runs held by a usage limit start once it has reset.
        self.pump(app);
        if !autopilot.allows(minutes) {
            return;
        }
        let paused: Vec<Provider> = self.paused_providers().into_iter().map(|(p, _)| p).collect();
        let candidates: Vec<(String, String)> = {
            let ws = state.workspace.lock().unwrap();
            ws.agent_ready_tasks()
                .into_iter()
                .filter(|(pid, tid)| ws.agent_context(pid, tid).is_ok_and(|(p, _)| !paused.contains(&p.settings.provider)))
                .collect()
        };
        for (project_id, task_id) in candidates {
            if self.busy() >= cap {
                break;
            }
            if self.task_occupied(&task_id) {
                continue;
            }
            match self.start(app, &project_id, &task_id, None, Default::default()) {
                Ok(run) => log::info!("autopilot started {} for {task_id}", run.id),
                Err(e) => log::warn!("autopilot couldn't start {task_id}: {e}"),
            }
        }
    }

    /// Whether a run of `task_id` is working, waiting on its plan, or waiting
    /// to be picked from a comparison.
    fn task_occupied(&self, task_id: &str) -> bool {
        self.runs.lock().unwrap().values().any(|l| {
            l.record.task_id == task_id
                && (l.record.state.is_active() || matches!(l.record.state, RunState::AwaitingApproval | RunState::Ready))
        })
    }

    /// Runs [`Self::autopilot_tick`] every 30 seconds on a background thread.
    /// Merged pull requests are picked up by the GitHub sync (`github.rs`).
    pub fn spawn_autopilot(app: AppHandle) {
        let spawned = std::thread::Builder::new().name("pill-autopilot".into()).spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(30));
                app.state::<Arc<Agents>>().autopilot_tick(&app);
            }
        });
        if let Err(e) = spawned {
            log::error!("couldn't start autopilot: {e}");
        }
    }

    // -------------------------------------------------------------------------
    // Board and notifications
    // -------------------------------------------------------------------------

    /// Puts the task on Doing for a run the user (or autopilot) just started,
    /// approved or retried, wherever the card was, and clears any old reason.
    fn claim_task(&self, app: &AppHandle, record: &RunRecord) {
        let state = app.state::<AppState>();
        let update = AgentUpdate {
            status: Some(TaskStatus::Doing),
            run_id: Some(record.id.clone()),
            branch: Some(record.branch.clone()),
            pr_url: None,
            blocked: Some(None),
        };
        if let Err(e) = state.workspace.lock().unwrap().set_agent_state(&record.project_id, &record.task_id, update) {
            log::warn!("moving {} to Doing: {e}", record.task_id);
        }
        state.emit_snapshot(app);
    }

    /// Moves a run's task to the column its state calls for (see
    /// [`board_move`]) and records the run's links and blocking reason.
    fn place_task(&self, app: &AppHandle, record: &RunRecord) {
        if let Some((target, blocked)) = board_move(record.state, record.error.as_deref()) {
            self.place_with(app, record, target, blocked);
        }
    }

    fn place_with(&self, app: &AppHandle, record: &RunRecord, target: TaskStatus, blocked: Option<String>) {
        let state = app.state::<AppState>();
        let Ok((_, task)) = state.workspace.lock().unwrap().agent_context(&record.project_id, &record.task_id) else { return };
        // An older run finishing late (a stopped retry, a cleanup) must not
        // drag the card away from the run that owns the task now.
        let last = task.agent.last_run_id.as_deref().and_then(|id| self.get(id));
        if !owns_task(last.as_ref(), record) {
            return;
        }
        let update = AgentUpdate {
            status: settle(task.status, target),
            run_id: Some(record.id.clone()),
            branch: Some(record.branch.clone()),
            pr_url: record.pr_url.clone(),
            blocked: Some(blocked),
        };
        if let Err(e) = state.workspace.lock().unwrap().set_agent_state(&record.project_id, &record.task_id, update) {
            log::warn!("updating {} after run {}: {e}", record.task_id, record.id);
        }
        state.emit_snapshot(app);
    }

    fn project_name(app: &AppHandle, project_id: &str) -> String {
        app.state::<AppState>().workspace.lock().unwrap().project(project_id).map(|p| p.name.clone()).unwrap_or_default()
    }

    /// Tells the user about a finished run when it needs them.
    fn notify_finished(&self, app: &AppHandle, record: &RunRecord) {
        if let Some(notice) = finish_notice(record, &Self::project_name(app, &record.project_id)) {
            crate::notify::send(app, &notice);
        }
    }

    fn project_vanished(&self, run_id: &str) -> bool {
        self.runs.lock().unwrap().get(run_id).is_some_and(|l| !l.project_path.is_dir())
    }

    fn cancelled(&self, run_id: &str) -> bool {
        self.runs.lock().unwrap().get(run_id).is_some_and(|l| l.cancel.load(Ordering::SeqCst))
    }

    fn finish(&self, app: &AppHandle, run_id: &str, state: RunState, error: Option<String>) {
        self.update(app, run_id, |r| {
            r.state = state;
            r.error = error.clone();
            r.ended_at = Some(now_rfc3339());
            r.activity = Some(match state {
                RunState::PrOpen => "Draft PR ready".into(),
                RunState::Pushed => "Branch pushed".into(),
                RunState::Committed => "Committed on the local branch".into(),
                RunState::Ready => "Tests passed; ready to compare".into(),
                RunState::AwaitingApproval => "Plan ready for review".into(),
                RunState::Stopped => "Stopped".into(),
                _ => error.clone().unwrap_or_else(|| "Failed".into()),
            });
        });
        if let Some(e) = error {
            self.log_event(app, run_id, RunEvent::Result { ok: false, text: e });
        }
        let Some(record) = self.get(run_id) else { return };
        // Until one is picked, a comparison moves the card and notifies as a
        // group; the picked run then finishes like any other.
        let picked = self.picked.lock().unwrap().contains(run_id);
        if record.compare_group.is_some() && !picked {
            if !state.is_active() {
                self.settle_comparison(app, &record);
            }
            return;
        }
        self.place_task(app, &record);
        self.notify_finished(app, &record);
    }

    /// Once every run in a comparison has finished, moves the card (Review
    /// when a run passed, else back to To do) and tells the user how many
    /// passed. Runs that finish earlier stay quiet so there's one toast.
    fn settle_comparison(&self, app: &AppHandle, record: &RunRecord) {
        let Some(group) = &record.compare_group else { return };
        let runs: Vec<RunRecord> = self
            .runs
            .lock()
            .unwrap()
            .values()
            .filter(|l| l.record.compare_group.as_ref() == Some(group))
            .map(|l| l.record.clone())
            .collect();
        if runs.iter().any(|r| r.state.is_active() || r.state == RunState::AwaitingApproval) {
            return;
        }
        if runs.iter().any(|r| r.state.is_shipped()) {
            return;
        }
        let ready: Vec<&RunRecord> = runs.iter().filter(|r| r.state == RunState::Ready).collect();
        if runs.iter().all(|r| r.state == RunState::Stopped) {
            self.place_with(app, record, TaskStatus::Todo, Some("You stopped the comparison".into()));
            return;
        }
        let (target, blocked, need, title, body) = if ready.is_empty() {
            (
                TaskStatus::Todo,
                Some(format!("None of the {} compared runs passed the checks", runs.len())),
                crate::notify::Need::Failed,
                format!("Comparison failed: {}", record.task_title),
                format!("None of the {} runs passed the checks.", runs.len()),
            )
        } else {
            (
                TaskStatus::Review,
                None,
                crate::notify::Need::Compare,
                format!("Pick a version: {}", record.task_title),
                format!("{} of {} passed the checks. Pick one to ship.", ready.len(), runs.len()),
            )
        };
        let shown = ready.first().copied().unwrap_or(record);
        self.place_with(app, shown, target, blocked);
        crate::notify::send(
            app,
            &crate::notify::Notice {
                need,
                title,
                body,
                project: Self::project_name(app, &record.project_id),
                project_id: Some(record.project_id.clone()),
                run_id: Some(shown.id.clone()),
                task_id: Some(record.task_id.clone()),
                pr_url: None,
            },
        );
    }

    /// Ships one compared run (per the project's ship mode) and discards the others in its
    /// group (worktrees removed, branches deleted).
    pub fn pick(&self, app: &AppHandle, run_id: &str) -> Result<(), String> {
        let record = self.get(run_id).ok_or("Unknown run")?;
        let group = record.compare_group.clone().ok_or("This run isn't part of a comparison")?;
        if record.state != RunState::Ready {
            return Err("Only a run whose tests passed can be shipped".into());
        }
        let others: Vec<RunRecord> = self
            .runs
            .lock()
            .unwrap()
            .values()
            .filter(|l| l.record.compare_group.as_ref() == Some(&group) && l.record.id != run_id)
            .map(|l| l.record.clone())
            .collect();
        if others.iter().any(|r| r.state.is_active()) {
            return Err("Wait for the other runs to finish, or stop them".into());
        }
        let project = app.state::<AppState>().workspace.lock().unwrap().agent_context(&record.project_id, &record.task_id)?.0;
        // Marked before the others stop, so their finishing doesn't settle
        // the group again and re-announce a comparison that's decided.
        self.picked.lock().unwrap().insert(run_id.to_string());
        self.update(app, run_id, |r| r.state = RunState::Shipping);
        for other in &others {
            if other.state != RunState::Stopped || other.worktree.exists() {
                self.finish(app, &other.id, RunState::Stopped, Some(format!("Not picked; {} was shipped", record.provider.label())));
                if let Err(e) = self.cleanup(app, &other.id) {
                    log::warn!("cleanup {}: {e}", other.id);
                }
            }
        }
        let (me, handle, id) = (app.state::<Arc<Agents>>().inner().clone(), app.clone(), run_id.to_string());
        std::thread::Builder::new()
            .name(format!("ship-{id}"))
            .spawn(move || {
                if let Err(e) = me.ship_run(&handle, &id, &project) {
                    me.finish(&handle, &id, RunState::Failed, Some(e));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Worker
    // -------------------------------------------------------------------------

    fn work(&self, app: &AppHandle, run_id: &str, work: Work) {
        let result = match &work {
            Work::Fresh => self.fresh(app, run_id),
            Work::ApprovePlan(plan) => self.resume(app, run_id, Resume::Implement(plan.clone())),
            Work::Retry(feedback) => self.resume(app, run_id, Resume::Retry(feedback.clone())),
        };
        match result {
            Ok(Outcome::Shipped) | Ok(Outcome::AwaitingApproval) => {}
            Err(_) if self.cancelled(run_id) => self.finish(app, run_id, RunState::Stopped, Some("Stopped".into())),
            Err(_) if self.project_vanished(run_id) => {
                let why = "The project folder was moved, renamed or deleted while the agent worked".to_string();
                log::warn!("run {run_id} failed: {why}");
                self.finish(app, run_id, RunState::Failed, Some(why));
            }
            Err(e) => {
                log::warn!("run {run_id} failed: {e}");
                self.finish(app, run_id, RunState::Failed, Some(e));
            }
        }
        #[cfg(windows)]
        if let Some(live) = self.runs.lock().unwrap().get_mut(run_id) {
            live.job = None;
        }
    }

    fn context(&self, app: &AppHandle, run_id: &str) -> Result<(RunRecord, Project, crate::model::Task, PathBuf), String> {
        let record = self.get(run_id).ok_or("Unknown run")?;
        let (project, task) = app
            .state::<AppState>()
            .workspace
            .lock()
            .unwrap()
            .agent_context(&record.project_id, &record.task_id)?;
        let dir = run_dir(&project.path, run_id);
        Ok((record, project, task, dir))
    }

    fn fresh(&self, app: &AppHandle, run_id: &str) -> Result<Outcome, String> {
        self.update(app, run_id, |r| r.state = RunState::Preparing);
        let (record, project, task, dir) = self.context(app, run_id)?;

        self.step(app, run_id, format!("Checking {}, gh and project settings", record.provider.label()));
        let checks = preflight(&project, record.provider);
        if let Some(bad) = checks.iter().find(|c| !c.ok) {
            return Err(bad.message.clone());
        }
        if let Some(remote) = &project.remote {
            self.step(app, run_id, format!("Fetching {remote}/{}", project.base_branch));
        }
        let base = worktree::resolve_base(&project)?;
        if let Some(note) = &base.note {
            self.step(app, run_id, note.clone());
        }
        self.update(app, run_id, |r| r.base_ref = Some(base.base_ref.clone()));
        self.check_cancel(run_id)?;
        if has_user_changes(&project.path) {
            self.step(app, run_id, "Your working folder has uncommitted changes; they stay there and aren't part of this run");
        }

        // A previous run for the same task may have left its worktree behind.
        if record.worktree.exists() {
            self.step(app, run_id, "Removing the previous worktree for this task");
            worktree::remove(&project.path, &record.worktree)?;
        }
        self.step(app, run_id, format!("Creating worktree on {} from {}", record.branch, base.base_ref));
        worktree::add(&project.path, &record.branch, &record.worktree, &base.base_ref)?;
        self.recheck_worktree(run_id);
        self.check_cancel(run_id)?;

        let issue = match &task.issue {
            Some(link) => {
                self.step(app, run_id, format!("Reading issue #{}", link.number));
                let github = app.state::<Arc<crate::github::Github>>();
                match github.issue(&project.id, &project.path, link.number) {
                    Ok(issue) => Some(issue),
                    Err(e) => {
                        self.log_event(app, run_id, RunEvent::Output { text: format!("Couldn't read issue #{}: {e}", link.number) });
                        None
                    }
                }
            }
            None => None,
        };
        let pr_path = dir.join("pr.md");
        let test_command = effective_test_command(&project, &record.worktree);
        let ctx = PromptContext {
            project_name: &project.name,
            branch: &record.branch,
            task_title: &task.title,
            task_notes: &task.notes,
            pinned_notes: &task.pinned_notes,
            base_branch: &project.base_branch,
            test_command: &test_command,
            pr_body_path: &pr_path.to_string_lossy(),
            project_rules: &project.settings.agent_rules,
            plan_only: record.plan_mode,
            origin: task.source(),
            issue: task.issue.as_ref().map(|link| prompt::IssuePrompt {
                number: link.number,
                url: &link.url,
                title: issue.as_ref().map_or(task.title.as_str(), |i| i.title.as_str()),
                body: issue
                    .as_ref()
                    .map_or("(Pill couldn't load the issue text. Work from the task title and notes.)", |i| i.body.as_str()),
            }),
        };
        let text = prompt::render(&ctx);
        fs::write(dir.join("prompt.md"), &text).map_err(|e| e.to_string())?;

        self.update(app, run_id, |r| r.state = RunState::Running);
        if record.plan_mode {
            self.step(app, run_id, format!("{} is exploring and writing a plan", record.provider.label()));
            let finish = self.agent_cli(app, run_id, &project, &text, None, true)?;
            let plan = self.get(run_id).and_then(|r| r.plan).filter(|p| !p.trim().is_empty()).unwrap_or(finish.text);
            if plan.trim().is_empty() {
                return Err("Claude finished without producing a plan".into());
            }
            self.update(app, run_id, |r| r.plan = Some(plan));
            self.finish(app, run_id, RunState::AwaitingApproval, None);
            return Ok(Outcome::AwaitingApproval);
        }

        self.step(app, run_id, format!("{} is working", record.provider.label()));
        let finish = self.agent_cli(app, run_id, &project, &text, None, false)?;
        self.verify_and_ship(app, run_id, &project, &finish)
    }

    fn resume(&self, app: &AppHandle, run_id: &str, how: Resume) -> Result<Outcome, String> {
        let finish = match &how {
            Resume::Implement(plan) => {
                self.follow_up(app, run_id, |label| format!("Plan approved, {label} is building"), |ctx, _| prompt::implement_prompt(ctx, plan))?
            }
            Resume::Retry(feedback) => self.follow_up(
                app,
                run_id,
                |_| "Retrying with feedback".into(),
                |ctx, r| prompt::retry_prompt(ctx, feedback, r.tests.summary.as_deref().filter(|_| r.tests.passed == Some(false))),
            )?,
        };
        let project = self.context(app, run_id)?.1;
        self.verify_and_ship(app, run_id, &project, &finish)
    }

    /// Resumes the run's session with one more prompt (from `render`) and
    /// returns the CLI's result. `step` names what happens, given the
    /// provider's label.
    fn follow_up(
        &self,
        app: &AppHandle,
        run_id: &str,
        step: impl FnOnce(&str) -> String,
        render: impl FnOnce(&PromptContext, &RunRecord) -> String,
    ) -> Result<stream::Finish, String> {
        let (record, project, task, dir) = self.context(app, run_id)?;
        let session = record.claude_session_id.clone().ok_or("No Claude session to resume")?;
        if !record.worktree.is_dir() {
            return Err("The worktree for this run no longer exists".into());
        }
        if !worktree::is_pill_worktree(&record.worktree, &project.path) {
            return Err(format!("{} isn't this project's worktree folder; start a new run instead", record.worktree.display()));
        }
        let pr_path = dir.join("pr.md");
        let test_command = effective_test_command(&project, &record.worktree);
        let ctx = PromptContext {
            project_name: &project.name,
            branch: &record.branch,
            task_title: &task.title,
            task_notes: &task.notes,
            pinned_notes: &task.pinned_notes,
            base_branch: &project.base_branch,
            test_command: &test_command,
            pr_body_path: &pr_path.to_string_lossy(),
            project_rules: &project.settings.agent_rules,
            plan_only: false,
            origin: task.source(),
            // The resumed session already read the issue in its first prompt.
            issue: None,
        };
        self.step(app, run_id, step(record.provider.label()));
        let text = render(&ctx, &record);
        let _ = OpenOptions::new()
            .append(true)
            .create(true)
            .open(dir.join("prompt.md"))
            .and_then(|mut f| writeln!(f, "\n\n---\n\n{text}"));
        self.update(app, run_id, |r| r.state = RunState::Running);
        self.agent_cli(app, run_id, &project, &text, Some(&session), false)
    }

    fn check_cancel(&self, run_id: &str) -> Result<(), String> {
        if self.cancelled(run_id) { Err("Stopped".into()) } else { Ok(()) }
    }

    /// Runs the run's provider CLI headless and streams its events.
    /// Returns the final result.
    fn agent_cli(
        &self,
        app: &AppHandle,
        run_id: &str,
        project: &Project,
        prompt_text: &str,
        resume: Option<&str>,
        plan: bool,
    ) -> Result<stream::Finish, String> {
        let record = &self.get(run_id).ok_or("Unknown run")?;
        let config = app.state::<AppState>().config.read().unwrap().clone();
        let limits = config.agent.clone();
        let (model, effort) = crate::models::resolve(&config, project, record.provider, &record.choice);
        // NOTE: a task that has vanished counts as untrusted, so the tighter list is the fallback.
        let untrusted = app
            .state::<AppState>()
            .workspace
            .lock()
            .unwrap()
            .agent_context(&record.project_id, &record.task_id)
            .map_or(true, |(_, t)| t.source().is_untrusted());
        self.update(app, run_id, |r| {
            r.model = model.clone();
            r.effort = effort.clone();
        });
        let dir = run_dir(&project.path, run_id);
        let private = provider::private_state_dir(run_id);
        provider::adopt_legacy_codex_home(&dir, run_id);
        let mut cmd = provider::command(
            record.provider,
            &provider::Spec {
                project: Some(project),
                cwd: &record.worktree,
                extra_dirs: vec![dir.as_path()],
                resume,
                max_turns: limits.max_turns,
                phase: if plan { Phase::Plan } else { Phase::Build },
                state_dir: &private,
                system: None,
                model: model.as_deref(),
                effort: effort.as_deref(),
                partial: false,
                host_prompts: true,
                untrusted,
            },
        )?;
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

        let label = record.provider.label();
        let mut child = cmd.spawn().map_err(|e| format!("Couldn't start {label}: {e}"))?;
        self.attach_job(run_id, &child)?;
        let mut conversation = Conversation::start(record.provider, &mut child, prompt_text, &Turn { cwd: &record.worktree, resume, host: true })?;
        let approvals = app.state::<Arc<Approvals>>().inner().clone();
        let mut ask = |t: ToolAsk| {
            let mut a = t.to_approval(record.provider, &project.id, &record.task_title);
            a.run_id = Some(run_id.to_string());
            a.task_id = Some(record.task_id.clone());
            let waiting = format!("Waiting for you: {}", a.summary);
            self.update(app, run_id, |r| r.activity = Some(waiting));
            let answer = approvals.ask(app, a, &|| self.cancelled(run_id));
            self.update(app, run_id, |r| r.activity = None);
            answer
        };
        let stderr = child.stderr.take();
        let stderr_reader = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut s) = stderr {
                let _ = std::io::Read::read_to_string(&mut s, &mut buf);
            }
            buf
        });

        let timed_out = self.watchdog(app, run_id, limits.timeout_minutes);
        let mut transcript = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("transcript.jsonl"))
            .map_err(|e| e.to_string())?;
        let mut parser = StreamParser::new(record.provider, &record.worktree);
        let mut finish: Option<stream::Finish> = None;
        let mut limit: Option<UsageLimit> = None;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let lines = BufReader::new(stdout).lines().map_while(Result::ok).flat_map(|raw| conversation.on_line(&raw, &mut ask));
        for line in lines {
            let _ = writeln!(transcript, "{line}");
            for parsed in parser.feed(&line) {
                match parsed {
                    Parsed::Session(id) => self.update(app, run_id, |r| r.claude_session_id = Some(id)),
                    Parsed::Model(m) => self.update(app, run_id, |r| r.model = Some(m)),
                    Parsed::FileTouched(f) => self.update(app, run_id, |r| {
                        if !r.files_changed.contains(&f) {
                            r.files_changed.push(f);
                        }
                    }),
                    Parsed::Plan(p) => self.update(app, run_id, |r| r.plan = Some(p)),
                    Parsed::UsageLimit(l) => {
                        self.log_event(app, run_id, RunEvent::Output { text: l.describe() });
                        limit = Some(l);
                    }
                    Parsed::Event(ev) => {
                        if let RunEvent::Tool { name, detail } = &ev {
                            let label = format!("{name} {detail}");
                            self.update(app, run_id, |r| r.activity = Some(label));
                        }
                        self.log_event(app, run_id, ev);
                    }
                    Parsed::Finished(f) => {
                        self.update(app, run_id, |r| {
                            r.turns = f.turns.or(r.turns);
                            r.cost_usd = match (r.cost_usd, f.cost_usd) {
                                (Some(a), Some(b)) => Some(a + b),
                                (a, b) => b.or(a),
                            };
                        });
                        finish = Some(f);
                    }
                }
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        timed_out.1.store(true, Ordering::SeqCst);
        let stderr_text = stderr_reader.join().unwrap_or_default();
        self.check_cancel(run_id)?;
        if timed_out.0.load(Ordering::SeqCst) {
            return Err(format!("Timed out after {} minutes", limits.timeout_minutes));
        }
        let error = match finish {
            Some(f) if f.ok => {
                self.log_event(app, run_id, RunEvent::Result { ok: true, text: format!("{label} finished") });
                return Ok(f);
            }
            Some(f) => match f.subtype.as_str() {
                "error_max_turns" => format!("{label} hit the {}-turn limit before finishing", limits.max_turns),
                _ if !f.text.is_empty() => f.text,
                other => format!("{label} stopped with {other}"),
            },
            // Codex can exit cleanly with nothing on stdout when the login or
            // quota fails; the reason is only on stderr.
            None if stderr_text.trim().is_empty() => format!("{label} exited ({status}) without a result"),
            None => last_lines(&stderr_text, 8),
        };
        match limit.or_else(|| crate::limits::detect(record.provider, &error)) {
            Some(l) => {
                self.pause(app, &l);
                Err(l.describe())
            }
            None => Err(error),
        }
    }

    #[cfg(windows)]
    fn attach_job(&self, run_id: &str, child: &Child) -> Result<(), String> {
        let job = Arc::new(process::job::Job::new().map_err(|e| e.to_string())?);
        job.assign(child).map_err(|e| e.to_string())?;
        if let Some(live) = self.runs.lock().unwrap().get_mut(run_id) {
            live.job = Some(job);
        }
        // Stop may have been pressed between spawn and assignment.
        if self.cancelled(run_id) {
            if let Some(live) = self.runs.lock().unwrap().get(run_id) {
                if let Some(j) = &live.job {
                    j.kill();
                }
            }
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn attach_job(&self, _run_id: &str, _child: &Child) -> Result<(), String> {
        Ok(())
    }

    /// Kills the run's job if the guarded phase outlives the timeout. Each
    /// phase (Claude, tests) gets the full budget, so a retry started hours
    /// later isn't instantly out of time. Returns `(timed_out, done)`; set
    /// `done` when the guarded process exits.
    fn watchdog(&self, app: &AppHandle, run_id: &str, minutes: u32) -> (Arc<AtomicBool>, Arc<AtomicBool>) {
        let timed_out = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let mut deadline = chrono::Utc::now() + chrono::Duration::minutes(i64::from(minutes));
        // Time spent waiting for the user's approval doesn't count.
        let approvals = app.state::<Arc<Approvals>>().inner().clone();
        let rid = run_id.to_string();
        let (t, d) = (timed_out.clone(), done.clone());
        #[cfg(windows)]
        let job = self.runs.lock().unwrap().get(run_id).and_then(|l| l.job.clone());
        #[cfg(not(windows))]
        let _ = run_id;
        std::thread::spawn(move || {
            while !d.load(Ordering::SeqCst) {
                if approvals.waiting_on_run(&rid) {
                    deadline += chrono::Duration::milliseconds(500);
                }
                if chrono::Utc::now() >= deadline {
                    t.store(true, Ordering::SeqCst);
                    #[cfg(windows)]
                    if let Some(j) = &job {
                        j.kill();
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        (timed_out, done)
    }

    /// Verifies the run's work and ships it. When the tests or lint fail and
    /// fix turns are left ([`crate::config::AgentLimits::fix_attempts`]),
    /// the failing output goes back to the same session and the checks run
    /// again; only then does the run fail.
    fn verify_and_ship(&self, app: &AppHandle, run_id: &str, project: &Project, finish: &stream::Finish) -> Result<Outcome, String> {
        let mut finish = finish.clone();
        loop {
            let failure = match self.verify(app, run_id, project, &finish)? {
                Verdict::Passed => break,
                Verdict::NoChanges(answer) => {
                    self.update(app, run_id, |r| r.answer = Some(answer).filter(|a| !a.is_empty()));
                    self.log_event(app, run_id, RunEvent::Result { ok: true, text: "Finished without changing code".into() });
                    self.finish(app, run_id, RunState::Answered, None);
                    return Ok(Outcome::Shipped);
                }
                Verdict::Failed(failure) => failure,
            };
            let record = self.get(run_id).ok_or("Unknown run")?;
            let allowed = app.state::<AppState>().config.read().unwrap().agent.fix_attempts;
            let label = record.provider.label();
            if record.fix_attempts >= allowed || record.claude_session_id.is_none() {
                return Err(checks_failed_message(failure.check, label, record.fix_attempts));
            }
            let attempt = record.fix_attempts + 1;
            self.update(app, run_id, |r| r.fix_attempts = attempt);
            finish = self.follow_up(
                app,
                run_id,
                |label| format!("{} failed, so {label} gets the output to fix it ({attempt} of {allowed})", failure.check),
                |ctx, _| prompt::retry_prompt(ctx, "", Some(&failure.output)),
            )?;
        }

        if self.get(run_id).is_some_and(|r| r.compare_group.is_some()) {
            self.finish(app, run_id, RunState::Ready, None);
            self.step(app, run_id, "Ready to compare");
            return Ok(Outcome::Shipped);
        }
        self.ship_run(app, run_id, project)
    }

    /// Checks the run's work: it made commits, the tests pass and the lint
    /// passes. `Err` is a failure no fix turn can help with.
    fn verify(&self, app: &AppHandle, run_id: &str, project: &Project, finish: &stream::Finish) -> Result<Verdict, String> {
        let record = self.get(run_id).ok_or("Unknown run")?;
        let dir = run_dir(&project.path, run_id);
        let pr_body = fs::read_to_string(dir.join("pr.md")).unwrap_or_default();
        if !record.worktree.is_dir() {
            return Err(format!("The worktree folder {} was deleted while the agent worked", record.worktree.display()));
        }

        let base = record.base_ref.clone().unwrap_or_else(|| base_ref(project));
        let ahead: u32 = git(&record.worktree, &["rev-list", "--count", "--end-of-options", &format!("{base}..HEAD")])?
            .parse()
            .unwrap_or(0);
        // NOTE: tracked files only. Installs and builds leave untracked output
        // (node_modules, caches) in repositories that don't ignore it, and
        // that alone must not fail a run.
        let dirty = !git(&record.worktree, &["status", "--porcelain", "--untracked-files=no"])?.is_empty();
        let label = record.provider.label();
        if let Ok(stat) = git(&record.worktree, &["diff", "--shortstat", "--end-of-options", &format!("{base}...HEAD")]) {
            let diff = parse_shortstat(&stat);
            self.update(app, run_id, |r| r.diff = Some(diff));
        }
        if ahead == 0 {
            if dirty {
                return Err(format!("{label} left uncommitted changes and made no commits"));
            }
            // NOTE: not every task is a code change. Research, reviews and
            // questions end with an answer; that is a result, not a failure.
            let answer = if finish.text.trim().is_empty() { pr_body.trim().to_string() } else { finish.text.trim().to_string() };
            return Ok(Verdict::NoChanges(answer));
        }

        self.update(app, run_id, |r| r.state = RunState::Verifying);
        let configured = project.settings.test_command.trim();
        // NOTE: with no command set, look in the worktree, not the project:
        // the run may be the one that set tests up.
        let detected = if configured.is_empty() { crate::checks::detect(&record.worktree).test } else { None };
        let tests = detected.as_ref().map_or(configured, |f| f.command.as_str());
        if tests.is_empty() {
            self.log_event(app, run_id, RunEvent::Output { text: "This project has no tests Pill can find, so the changes weren't verified".into() });
        } else {
            match &detected {
                Some(found) => {
                    let command = found.command.clone();
                    self.update(app, run_id, |r| r.tests_detected = Some(command));
                    self.step(app, run_id, format!("Verifying: {tests} (found in {})", found.source));
                }
                None => self.step(app, run_id, format!("Verifying: {tests}")),
            }
            let (passed, summary) = self.run_tests(app, run_id, project, &record, tests, "tests.log")?;
            self.update(app, run_id, |r| r.tests = TestsResult { passed: Some(passed), summary: Some(summary.clone()) });
            if !passed {
                self.log_event(app, run_id, RunEvent::Output { text: summary.clone() });
                return Ok(Verdict::Failed(CheckFailure { check: "Tests", output: summary }));
            }
            self.log_event(app, run_id, RunEvent::Result { ok: true, text: "Tests passed".into() });
            self.check_cancel(run_id)?;
        }

        let lint = project.settings.lint_command.trim();
        if !lint.is_empty() {
            self.step(app, run_id, format!("Linting: {lint}"));
            let (passed, summary) = self.run_tests(app, run_id, project, &record, lint, "lint.log")?;
            if !passed {
                self.log_event(app, run_id, RunEvent::Output { text: summary.clone() });
                return Ok(Verdict::Failed(CheckFailure { check: "Lint", output: summary }));
            }
            self.log_event(app, run_id, RunEvent::Result { ok: true, text: "Lint passed".into() });
            self.check_cancel(run_id)?;
        }
        Ok(Verdict::Passed)
    }

    /// Ships a verified run as the project's [`ShipMode`] says: keep it on the
    /// local branch, push the branch, or push and open (or update) a draft PR.
    fn ship_run(&self, app: &AppHandle, run_id: &str, project: &Project) -> Result<Outcome, String> {
        let record = self.get(run_id).ok_or("Unknown run")?;
        let dir = run_dir(&project.path, run_id);
        let pr_body = fs::read_to_string(dir.join("pr.md")).unwrap_or_default();
        let mode = ship_mode(project);
        if !is_agent_branch(project, &record.task_id, &record.branch) {
            return Err(format!("Refusing to ship {}: it isn't one of this project's agent branches", record.branch));
        }
        if !worktree::is_pill_worktree(&record.worktree, &project.path) {
            return Err("Refusing to ship from a folder that isn't this run's worktree".into());
        }
        self.update(app, run_id, |r| r.state = RunState::Shipping);
        if mode.pushes() {
            let remote = project.remote.as_deref().ok_or("This project has no git remote to push to")?;
            self.step(app, run_id, format!("Pushing {} to {remote}", record.branch));
            // Only ever the agent branch, and never with --force.
            git(&record.worktree, &["push", "-u", remote, &format!("HEAD:refs/heads/{}", record.branch)])?;
        }
        if mode != ShipMode::Pr {
            return Ok(self.ship_without_pr(app, run_id, project, &record, mode));
        }

        let pr_url = match record.pr_url.clone() {
            Some(url) => {
                self.step(app, run_id, "Updated the existing pull request");
                url
            }
            None => {
                self.step(app, run_id, "Opening a draft pull request");
                let gh = process::which("gh").ok_or("GitHub CLI (gh) isn't installed")?;
                let body_file = dir.join("pr.md");
                let mut body =
                    if pr_body.trim().is_empty() { format!("Automated change for: {}\n", record.task_title) } else { pr_body.clone() };
                let issue = app
                    .state::<AppState>()
                    .workspace
                    .lock()
                    .unwrap()
                    .agent_context(&record.project_id, &record.task_id)
                    .ok()
                    .and_then(|(_, t)| t.issue);
                if let Some(link) = issue {
                    body = crate::github::with_closing_line(&body, link.number);
                }
                if body != pr_body {
                    fs::write(&body_file, &body).map_err(|e| e.to_string())?;
                }
                let mut cmd = process::tool(&gh);
                cmd.current_dir(&record.worktree)
                    .args(["pr", "create", "--draft"])
                    // NOTE: `--flag=value` so a value starting with `-` can never read as a flag.
                    .arg(format!("--base={}", project.base_branch))
                    .arg(format!("--head={}", record.branch))
                    .arg(format!("--title={}", record.task_title))
                    .arg("--body-file")
                    .arg(&body_file);
                let out = process::run(cmd).map_err(|e| format!("gh pr create: {e}"))?;
                out.lines().rev().find(|l| l.starts_with("https://")).unwrap_or(out.trim()).to_string()
            }
        };

        self.update(app, run_id, |r| r.pr_url = Some(pr_url.clone()));
        self.log_event(app, run_id, RunEvent::Result { ok: true, text: format!("Draft PR: {pr_url}") });
        self.finish(app, run_id, RunState::PrOpen, None);
        app.state::<Arc<crate::github::Github>>().request(Some(record.project_id.clone()));
        Ok(Outcome::Shipped)
    }

    /// Finishes a run shipped as [`ShipMode::Push`] or [`ShipMode::Local`]:
    /// the task moves to Review with the branch linked, and nothing else.
    fn ship_without_pr(&self, app: &AppHandle, run_id: &str, project: &Project, record: &RunRecord, mode: ShipMode) -> Outcome {
        let (state_after, text) = if mode.pushes() {
            (RunState::Pushed, format!("Pushed {} to {}", record.branch, project.remote.as_deref().unwrap_or("the remote")))
        } else {
            (RunState::Committed, format!("Committed on local branch {}", record.branch))
        };
        self.log_event(app, run_id, RunEvent::Result { ok: true, text });
        self.finish(app, run_id, state_after, None);
        Outcome::Shipped
    }

    /// Runs a check command (tests or lint) in the worktree, logging to
    /// `log_name` in the run folder. Returns (passed, last lines).
    fn run_tests(
        &self,
        app: &AppHandle,
        run_id: &str,
        project: &Project,
        record: &RunRecord,
        check: &str,
        log_name: &str,
    ) -> Result<(bool, String), String> {
        let dir = run_dir(&project.path, run_id);
        let log_path = dir.join(log_name);
        let log_file = File::create(&log_path).map_err(|e| e.to_string())?;
        let mut cmd = process::shell(check);
        cmd.current_dir(&record.worktree)
            .env_remove("ANTHROPIC_API_KEY")
            .env("CI", "1")
            .stdin(Stdio::null())
            .stdout(log_file.try_clone().map_err(|e| e.to_string())?)
            .stderr(log_file);
        let started = Instant::now();
        let mut child = cmd.spawn().map_err(|e| format!("Couldn't run tests: {e}"))?;
        self.attach_job(run_id, &child)?;
        let limits = app.state::<AppState>().config.read().unwrap().agent.clone();
        let guard = self.watchdog(app, run_id, limits.timeout_minutes);
        let status = child.wait().map_err(|e| e.to_string())?;
        guard.1.store(true, Ordering::SeqCst);
        self.check_cancel(run_id)?;
        if guard.0.load(Ordering::SeqCst) {
            return Err(format!("Timed out after {} minutes", limits.timeout_minutes));
        }
        let output = fs::read_to_string(&log_path).unwrap_or_default();
        log::info!("run {run_id}: {log_name} {} in {:?}", if status.success() { "passed" } else { "failed" }, started.elapsed());
        Ok((status.success(), last_lines(&output, 30)))
    }
}

enum Resume {
    Implement(String),
    Retry(String),
}

enum Outcome {
    Shipped,
    AwaitingApproval,
}

/// Whether the working folder has uncommitted changes to tracked files,
/// beyond the ignore rules Pill itself adds to `.gitignore`.
fn has_user_changes(repo: &Path) -> bool {
    let Ok(status) = git(repo, &["status", "--porcelain", "--untracked-files=no"]) else { return false };
    status.lines().any(|line| {
        line.get(3..).is_none_or(|path| path.trim() != ".gitignore")
            || git(repo, &["diff", "--no-color", "-U0", "--", ".gitignore"]).map_or(true, |d| !only_pill_ignore_lines(&d))
    })
}

/// True when a `.gitignore` diff only adds Pill's own block.
fn only_pill_ignore_lines(diff: &str) -> bool {
    diff.lines()
        .filter(|l| (l.starts_with('+') && !l.starts_with("+++")) || (l.starts_with('-') && !l.starts_with("---")))
        .all(|l| {
            let body = l[1..].trim();
            l.starts_with('+') && (body.is_empty() || body == "# Pill" || body.starts_with(".pill/"))
        })
}

/// The project's test command, else one found in the worktree, else empty.
fn effective_test_command(project: &Project, worktree: &Path) -> String {
    let configured = project.settings.test_command.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    crate::checks::detect(worktree).test.map(|f| f.command).unwrap_or_default()
}

/// What verifying a finished run found.
enum Verdict {
    /// Commits made and every configured check passed.
    Passed,
    /// No commits and a clean worktree: the reply is the result.
    NoChanges(String),
    Failed(CheckFailure),
}

/// A verification check that failed, with the output to show the agent.
struct CheckFailure {
    /// "Tests" or "Lint".
    check: &'static str,
    output: String,
}

/// Why a run failed its checks, counting the fix turns it already had.
fn checks_failed_message(check: &str, label: &str, fix_attempts: u32) -> String {
    match fix_attempts {
        0 => format!("{check} failed after {label} finished"),
        1 => format!("{check} still failed after one fix attempt"),
        n => format!("{check} still failed after {n} fix attempts"),
    }
}

/// The column a run's task belongs in once the run reaches `state`, and the
/// reason to show on its card (`None` clears it). `None` overall leaves the
/// card alone: a compared run that's ready waits for its group.
///
/// - working: Doing
/// - plan waiting for approval, or work shipped: Review
/// - failed, stopped or interrupted: back to To do, with the reason
pub(crate) fn board_move(state: RunState, error: Option<&str>) -> Option<(TaskStatus, Option<String>)> {
    let reason = |fallback: &str| Some(error.filter(|e| !e.trim().is_empty()).unwrap_or(fallback).to_string());
    match state {
        s if s.is_active() => Some((TaskStatus::Doing, None)),
        s if s.is_shipped() => Some((TaskStatus::Review, None)),
        RunState::AwaitingApproval | RunState::Answered => Some((TaskStatus::Review, None)),
        RunState::Ready => None,
        RunState::Stopped => Some((TaskStatus::Todo, Some("You stopped the run".into()))),
        RunState::Failed => Some((TaskStatus::Todo, reason("The run failed"))),
        RunState::Interrupted => Some((TaskStatus::Todo, reason("Pill quit while the agent was working"))),
        _ => None,
    }
}

/// The status to write when an agent moves a card from `current` toward
/// `target`, or `None` to leave it. A card the user put in Done stays there.
pub(crate) fn settle(current: TaskStatus, target: TaskStatus) -> Option<TaskStatus> {
    (current != TaskStatus::Done && current != target).then_some(target)
}

/// Whether `record` may move its task: it is the task's latest run, or part
/// of the same comparison as the latest run.
fn owns_task(last: Option<&RunRecord>, record: &RunRecord) -> bool {
    match last {
        Some(last) if last.id == record.id => true,
        Some(last) => record.compare_group.is_some() && last.compare_group == record.compare_group,
        // The task never recorded a run; this one is the first to report.
        None => true,
    }
}

/// The notification a run that just reached its state should send, if the
/// state needs the user. Stops are the user's own doing and stay quiet.
fn finish_notice(record: &RunRecord, project: &str) -> Option<crate::notify::Notice> {
    use crate::notify::{Need, Notice};
    let task = &record.task_title;
    let (need, title, body) = match record.state {
        RunState::AwaitingApproval => (Need::Plan, format!("Plan ready: {task}"), "Approve it and the agent starts building.".to_string()),
        RunState::PrOpen => (Need::Review, format!("Draft PR ready: {task}"), "Checks passed. It's ready for your review.".to_string()),
        RunState::Pushed => (Need::Review, format!("Ready for review: {task}"), format!("Checks passed. Pushed {}.", record.branch)),
        RunState::Committed => (Need::Review, format!("Ready for review: {task}"), format!("Checks passed. Committed on {}.", record.branch)),
        RunState::Answered => (
            Need::Review,
            format!("Finished: {task}"),
            first_paragraph(record.answer.as_deref().unwrap_or("The agent answered without changing code.")),
        ),
        RunState::Failed => (
            Need::Failed,
            format!("Run failed: {task}"),
            first_paragraph(record.error.as_deref().unwrap_or("The agent couldn't finish.")),
        ),
        _ => return None,
    };
    Some(Notice {
        need,
        title,
        body,
        project: project.to_string(),
        project_id: Some(record.project_id.clone()),
        run_id: Some(record.id.clone()),
        task_id: Some(record.task_id.clone()),
        pr_url: record.pr_url.clone().filter(|_| record.state == RunState::PrOpen),
    })
}

/// Tools every build run may use without asking: file edits, local git and
/// commands that only read or list.
const BASE_TOOLS: &[&str] = &[
    "Read", "Edit", "Write", "MultiEdit", "Glob", "Grep", "LS", "TodoWrite", "NotebookEdit",
    "Bash(git status:*)", "Bash(git diff:*)", "Bash(git log:*)", "Bash(git show:*)", "Bash(git add:*)",
    "Bash(git commit:*)", "Bash(git rm:*)", "Bash(git mv:*)", "Bash(git restore:*)", "Bash(git stash:*)",
    "Bash(ls:*)", "Bash(dir:*)", "Bash(mkdir:*)", "Bash(cat:*)", "Bash(type:*)", "Bash(echo:*)", "Bash(grep:*)",
];

/// Interpreters, package managers and build tools. Each runs arbitrary code
/// (install scripts, build scripts, `node -e`, `find -exec`), network access
/// included, so they are only pre-approved for tasks the user wrote.
const CODE_RUNNING_TOOLS: &[&str] = &[
    "Bash(npm:*)", "Bash(npx:*)", "Bash(pnpm:*)", "Bash(yarn:*)", "Bash(bun:*)", "Bash(node:*)",
    "Bash(cargo:*)", "Bash(rustc:*)", "Bash(go:*)", "Bash(python:*)", "Bash(python3:*)", "Bash(pip:*)",
    "Bash(uv:*)", "Bash(pytest:*)", "Bash(dotnet:*)", "Bash(make:*)", "Bash(find:*)",
];

/// Tools a Claude build run may use without asking Pill's approval prompt.
///
/// SECURITY: this list is the only thing between the agent and the machine.
/// Claude Code's own sandbox doesn't run natively on Windows, so a command
/// on this list runs with the user's rights and network access. For a user's
/// own task, package managers and the project's test and lint commands are
/// allowed so a fresh worktree can install dependencies and run its tests.
/// For an `untrusted` task (made from a GitHub issue or suggested by a
/// plugin) every command that can run code goes to the approval prompt
/// instead, since injected text in the task could steer the agent.
pub(crate) fn allowed_tools(project: &Project, untrusted: bool) -> Vec<String> {
    let mut tools: Vec<String> = BASE_TOOLS.iter().map(|s| s.to_string()).collect();
    if untrusted {
        return tools;
    }
    tools.extend(CODE_RUNNING_TOOLS.iter().map(|s| s.to_string()));
    for cmd in [&project.settings.test_command, &project.settings.lint_command] {
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            tools.push(format!("Bash({cmd}:*)"));
        }
    }
    tools
}

/// Never allowed, even with approval: pushing, GitHub access, moving
/// branches, and the obvious download commands and web tools.
///
/// NOTE: this does not keep the agent off the network. Any interpreter or
/// build tool can still reach it, which is why [`allowed_tools`] holds those
/// back for untrusted tasks.
pub(crate) const DENIED_TOOLS: &[&str] = &[
    "Bash(git push:*)",
    "Bash(git remote:*)",
    "Bash(git checkout:*)",
    "Bash(git switch:*)",
    "Bash(git branch:*)",
    "Bash(git reset --hard:*)",
    "Bash(git worktree:*)",
    "Bash(git rebase:*)",
    "Bash(gh:*)",
    "Bash(curl:*)",
    "Bash(wget:*)",
    "Bash(Invoke-WebRequest:*)",
    "WebFetch",
    "WebSearch",
];

/// The ref agent branches are compared against when the run didn't record
/// one (runs from before [`RunRecord::base_ref`]): the remote's copy of the
/// base branch when there is a remote, else the local base branch.
fn base_ref(project: &Project) -> String {
    base_ref_for(project.remote.as_deref(), &project.base_branch)
}

fn base_ref_for(remote: Option<&str>, base_branch: &str) -> String {
    match remote {
        Some(r) => format!("{r}/{base_branch}"),
        None => base_branch.to_string(),
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.trim_end().lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn first_paragraph(s: &str) -> String {
    let p = s.split("\n\n").next().unwrap_or(s).trim();
    if p.chars().count() > 240 { format!("{}…", p.chars().take(239).collect::<String>()) } else { p.to_string() }
}

/// Install and login status of every CLI Pill drives, independent of any project.
pub fn cli_status() -> Vec<Check> {
    let mut checks = vec![git_check()];
    checks.extend(claude_checks());
    checks.extend(provider::checks(Provider::Codex));
    checks.push(gh_check());
    if let Some(gh) = process::which("gh") {
        let mut cmd = process::tool(&gh);
        cmd.arg("--version");
        if let Ok(v) = process::run(cmd) {
            if let Some(c) = checks.iter_mut().find(|c| c.name == "GitHub CLI" && c.ok) {
                c.message = format!("Logged in · {}", v.lines().next().unwrap_or_default());
            }
        }
    }
    if let Some(claude) = process::which("claude") {
        let mut cmd = process::tool(&claude);
        cmd.arg("--version");
        if let Ok(v) = process::run(cmd) {
            if let Some(c) = checks.iter_mut().find(|c| c.name == "Claude Code") {
                c.message = format!("Installed · {v}");
            }
        }
    }
    checks
}

/// The finish mode a run uses. Without a remote there is nowhere to push, so
/// work stays local whatever the setting says; the setting takes effect once
/// the repository gets a remote.
pub fn ship_mode(project: &Project) -> ShipMode {
    if project.remote.is_some() { project.settings.ship } else { ShipMode::Local }
}

/// Checks everything a run with `provider` needs, with fix-it messages.
pub fn preflight(project: &Project, provider: Provider) -> Vec<Check> {
    let mut checks = provider::checks(provider);
    let git_ok = process::which("git").is_some();
    if !git_ok {
        checks.push(git_check());
    }
    if !project.path.is_dir() {
        checks.push(Check { name: "Repository".into(), ok: false, message: crate::workspace::PROJECT_GONE.into() });
        return checks;
    }
    let is_repo = scan::is_repo(&project.path);
    let has_commits = is_repo && git_ok && git(&project.path, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok();
    checks.push(Check {
        name: "Repository".into(),
        ok: is_repo && (has_commits || !git_ok),
        message: if !is_repo {
            format!("{} is a plain folder. Run `git init` and make a first commit so agents can work on a branch.", project.name)
        } else if git_ok && !has_commits {
            format!("{} has no commits yet. Make a first commit so agents have something to branch from.", project.name)
        } else {
            "Git repository".into()
        },
    });
    let mode = ship_mode(project);
    if mode == ShipMode::Pr {
        checks.push(gh_check());
    }
    if mode.pushes() {
        checks.push(Check {
            name: "Remote".into(),
            ok: project.remote.is_some(),
            message: match &project.remote {
                Some(r) => format!("Pushes to {r}"),
                None => format!(
                    "{} has no git remote. Add one (e.g. `gh repo create`), or set “When an agent finishes” to keep work local in project settings.",
                    project.name
                ),
            },
        });
    }
    // NOTE: not a blocker. Research and review tasks need no tests, and a
    // code change without them is still shown, marked as not verified.
    let has_tests = !project.settings.test_command.trim().is_empty();
    checks.push(Check {
        name: "Test command".into(),
        ok: true,
        message: if has_tests {
            project.settings.test_command.clone()
        } else {
            "None set, so code changes won't be verified. Add one in project settings.".into()
        },
    });
    checks
}

/// Git installed, with its version.
pub(crate) fn git_check() -> Check {
    let version = process::which("git").and_then(|exe| {
        let mut cmd = process::tool(&exe);
        cmd.arg("--version");
        process::run(cmd).ok()
    });
    Check {
        name: "Git".into(),
        ok: version.is_some(),
        message: match version {
            Some(v) => format!("Installed · {}", v.trim_start_matches("git version ")),
            None => process::GIT_MISSING.into(),
        },
    }
}

/// Claude Code installed and logged in.
pub(crate) fn claude_checks() -> Vec<Check> {
    let mut checks = Vec::new();
    let claude = process::which("claude");
    checks.push(Check {
        name: "Claude Code".into(),
        ok: claude.is_some(),
        message: if claude.is_some() {
            "Installed".into()
        } else {
            "Claude Code isn't installed. Install it with `npm install -g @anthropic-ai/claude-code`, then run `claude` once in a terminal and use /login.".into()
        },
    });
    if let Some(exe) = &claude {
        let mut cmd = process::tool(exe);
        cmd.args(["auth", "status"]).env_remove("ANTHROPIC_API_KEY");
        let status = process::run(cmd).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
        let logged_in = status.as_ref().is_some_and(|v| v["loggedIn"] == true);
        let method = status.as_ref().and_then(|v| v["authMethod"].as_str().map(str::to_string)).unwrap_or_default();
        checks.push(Check {
            name: "Claude login".into(),
            ok: logged_in,
            message: if logged_in {
                format!("Logged in ({method})")
            } else {
                "Not logged in. Run `claude` in a terminal and use /login with your Pro or Max account.".into()
            },
        });
    }
    checks
}

/// GitHub CLI installed and logged in.
pub(crate) fn gh_check() -> Check {
    let auth = process::which("gh").map(|exe| {
        let mut cmd = process::tool(&exe);
        cmd.args(["auth", "status"]);
        process::run(cmd).map(|_| ())
    });
    let (ok, message) = gh_status(auth);
    Check { name: "GitHub CLI".into(), ok, message }
}

/// Reads `gh auth status`: `None` when gh isn't installed, else its result.
fn gh_status(auth: Option<Result<(), String>>) -> (bool, String) {
    match auth {
        None => (false, "GitHub CLI isn't installed. Install it with `winget install --id GitHub.cli`, then run `gh auth login`.".into()),
        Some(Ok(())) => (true, "Logged in".into()),
        Some(Err(e)) => {
            let lower = e.to_lowercase();
            if ["not logged", "gh auth login", "no oauth token", "not authenticated"].iter().any(|s| lower.contains(s)) {
                (false, "GitHub CLI isn't logged in. Run `gh auth login` in a terminal.".into())
            } else if ["timeout", "could not resolve", "connect", "network", "dial tcp"].iter().any(|s| lower.contains(s)) {
                (false, "Couldn't reach GitHub to check the gh login. Check your connection and try again.".into())
            } else {
                let first = e.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("unknown error");
                (false, format!("GitHub CLI login problem: {first}. Run `gh auth login` in a terminal to fix it."))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_shortstat() {
        assert_eq!(
            parse_shortstat(" 3 files changed, 40 insertions(+), 2 deletions(-)"),
            DiffStat { files: 3, insertions: 40, deletions: 2 }
        );
        assert_eq!(parse_shortstat(" 1 file changed, 1 deletion(-)"), DiffStat { files: 1, insertions: 0, deletions: 1 });
        assert_eq!(parse_shortstat(""), DiffStat::default());
    }

    #[test]
    fn active_states() {
        assert!(RunState::Running.is_active());
        assert!(RunState::Queued.is_active());
        assert!(!RunState::AwaitingApproval.is_active());
        assert!(!RunState::PrOpen.is_active());
        assert!(!RunState::Ready.is_active());
    }

    #[test]
    fn record_round_trips_with_spec_field_names() {
        let json = r#"{
          "id": "r_20261119_2110_8f2k1", "taskId": "t_8f2k1", "projectId": "p_1",
          "state": "pr_open", "branch": "agent/t_8f2k1-fix", "worktree": "C:\\dev\\.pill-worktrees\\outlaw\\t_8f2k1",
          "claudeSessionId": "s", "startedAt": "2026-11-19T21:10:00Z", "endedAt": null,
          "tests": { "passed": true, "summary": "ok" }, "prUrl": "https://github.com/x/y/pull/1"
        }"#;
        let r: RunRecord = serde_json::from_str(json).unwrap();
        assert_eq!(r.state, RunState::PrOpen);
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["state"], "pr_open");
        assert_eq!(back["prUrl"], "https://github.com/x/y/pull/1");
    }

    #[test]
    fn denied_tools_block_push_and_github() {
        assert!(DENIED_TOOLS.contains(&"Bash(git push:*)"));
        assert!(DENIED_TOOLS.contains(&"Bash(gh:*)"));
    }

    #[test]
    fn untrusted_tasks_get_no_code_running_tools_without_asking() {
        let mut project = crate::workspace::test_project(Path::new(r"C:\dev\app"));
        project.settings.test_command = "pnpm test".into();
        project.settings.lint_command = "pnpm lint".into();
        let mine = allowed_tools(&project, false);
        for t in ["Bash(npm:*)", "Bash(node:*)", "Bash(python:*)", "Bash(pnpm test:*)", "Bash(pnpm lint:*)", "Edit"] {
            assert!(mine.iter().any(|m| m == t), "{t}");
        }
        let theirs = allowed_tools(&project, true);
        for t in CODE_RUNNING_TOOLS.iter().copied().chain(["Bash(pnpm test:*)", "Bash(pnpm lint:*)"]) {
            assert!(!theirs.iter().any(|m| m == t), "{t}");
        }
        assert!(theirs.iter().any(|m| m == "Edit") && theirs.iter().any(|m| m == "Bash(git commit:*)"));
    }

    #[test]
    fn local_only_repos_branch_from_the_local_base() {
        assert_eq!(base_ref_for(Some("origin"), "main"), "origin/main");
        assert_eq!(base_ref_for(Some("upstream"), "main"), "upstream/main");
        assert_eq!(base_ref_for(None, "trunk"), "trunk");
    }

    #[test]
    fn shipped_states_cover_every_ship_mode() {
        assert!(RunState::PrOpen.is_shipped());
        assert!(RunState::Pushed.is_shipped());
        assert!(RunState::Committed.is_shipped());
        assert!(!RunState::Ready.is_shipped());
        assert!(!RunState::Committed.is_active());
        assert_eq!(serde_json::to_value(RunState::Committed).unwrap(), "committed");
    }

    #[test]
    fn last_lines_keeps_the_tail() {
        assert_eq!(last_lines("a\nb\nc\n", 2), "b\nc");
        assert_eq!(last_lines("a", 5), "a");
    }

    fn record(id: &str, state: RunState) -> RunRecord {
        let json = format!(
            r#"{{ "id": "{id}", "taskId": "t_1", "projectId": "p_1", "taskTitle": "Fix scatter", "state": "{}",
                 "branch": "agent/t_1-fix", "worktree": "C:/wt", "startedAt": "2026-11-19T21:10:00Z" }}"#,
            serde_json::to_value(state).unwrap().as_str().unwrap()
        );
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn old_run_files_load_without_hands_off_fields() {
        let r = record("r_1", RunState::PrOpen);
        assert!(!r.hands_off);
        assert_eq!(r.fix_attempts, 0);
    }

    #[test]
    fn board_follows_the_run() {
        for s in [RunState::Queued, RunState::Preparing, RunState::Running, RunState::Verifying, RunState::Shipping] {
            assert_eq!(board_move(s, None), Some((TaskStatus::Doing, None)), "{s:?}");
        }
        for s in [RunState::AwaitingApproval, RunState::PrOpen, RunState::Pushed, RunState::Committed] {
            assert_eq!(board_move(s, None), Some((TaskStatus::Review, None)), "{s:?}");
        }
        assert_eq!(board_move(RunState::Ready, None), None, "a comparison moves as a group");
    }

    #[test]
    fn failures_go_back_to_todo_with_a_reason() {
        assert_eq!(
            board_move(RunState::Failed, Some("Tests failed after Claude Code finished")),
            Some((TaskStatus::Todo, Some("Tests failed after Claude Code finished".into())))
        );
        assert_eq!(board_move(RunState::Failed, Some("  ")), Some((TaskStatus::Todo, Some("The run failed".into()))));
        assert_eq!(board_move(RunState::Stopped, Some("Stopped")), Some((TaskStatus::Todo, Some("You stopped the run".into()))));
        let (col, reason) = board_move(RunState::Interrupted, None).unwrap();
        assert_eq!(col, TaskStatus::Todo);
        assert!(reason.unwrap().contains("Pill quit"));
    }

    #[test]
    fn pills_own_gitignore_block_is_not_a_user_change() {
        let ours = "diff --git a/.gitignore b/.gitignore\n--- a/.gitignore\n+++ b/.gitignore\n@@ -1 +1,5 @@\n+\n+# Pill\n+.pill/runs/\n+.pill/tasks.json\n";
        assert!(only_pill_ignore_lines(ours));
        assert!(!only_pill_ignore_lines("+++ b/.gitignore\n+dist/\n"));
        assert!(!only_pill_ignore_lines("--- a/.gitignore\n-node_modules\n"));
    }

    #[test]
    fn an_answer_without_code_changes_goes_to_review() {
        assert_eq!(board_move(RunState::Answered, None), Some((TaskStatus::Review, None)));
        let mut r = record("r_1", RunState::Answered);
        r.answer = Some("No affected versions are deployed.\n\nDetails follow.".into());
        let n = finish_notice(&r, "game").unwrap();
        assert_eq!(n.need, crate::notify::Need::Review);
        assert_eq!(n.title, "Finished: Fix scatter");
        assert_eq!(n.body, "No affected versions are deployed.");
    }

    #[test]
    fn agents_never_pull_a_card_out_of_done() {
        assert_eq!(settle(TaskStatus::Todo, TaskStatus::Doing), Some(TaskStatus::Doing));
        assert_eq!(settle(TaskStatus::Doing, TaskStatus::Review), Some(TaskStatus::Review));
        assert_eq!(settle(TaskStatus::Review, TaskStatus::Todo), Some(TaskStatus::Todo));
        assert_eq!(settle(TaskStatus::Review, TaskStatus::Review), None);
        assert_eq!(settle(TaskStatus::Done, TaskStatus::Todo), None);
        assert_eq!(settle(TaskStatus::Done, TaskStatus::Review), None);
    }

    #[test]
    fn only_the_latest_run_or_its_comparison_moves_the_card() {
        let latest = record("r_2", RunState::Running);
        let old = record("r_1", RunState::Stopped);
        assert!(owns_task(Some(&latest), &latest));
        assert!(!owns_task(Some(&latest), &old));
        assert!(owns_task(None, &old));
        let mut a = record("r_a", RunState::Ready);
        let mut b = record("r_b", RunState::Ready);
        a.compare_group = Some("g_1".into());
        b.compare_group = Some("g_1".into());
        assert!(owns_task(Some(&b), &a));
        b.compare_group = Some("g_2".into());
        assert!(!owns_task(Some(&b), &a));
    }

    #[test]
    fn notices_only_when_the_user_is_needed() {
        let mut r = record("r_1", RunState::AwaitingApproval);
        let n = finish_notice(&r, "game").unwrap();
        assert_eq!(n.need, crate::notify::Need::Plan);
        assert_eq!(n.title, "Plan ready: Fix scatter");
        assert_eq!((n.run_id.as_deref(), n.task_id.as_deref()), (Some("r_1"), Some("t_1")));

        r.state = RunState::PrOpen;
        r.pr_url = Some("https://github.com/x/y/pull/3".into());
        let n = finish_notice(&r, "game").unwrap();
        assert_eq!(n.need, crate::notify::Need::Review);
        assert_eq!(n.pr_url.as_deref(), Some("https://github.com/x/y/pull/3"));

        r.state = RunState::Committed;
        let n = finish_notice(&r, "game").unwrap();
        assert!(n.body.contains("agent/t_1-fix"));
        assert_eq!(n.pr_url, None);

        r.state = RunState::Failed;
        r.error = Some("Tests still failed after one fix attempt\n\nmore detail".into());
        let n = finish_notice(&r, "game").unwrap();
        assert_eq!(n.need, crate::notify::Need::Failed);
        assert_eq!(n.body, "Tests still failed after one fix attempt");

        for quiet in [RunState::Stopped, RunState::Interrupted, RunState::Running, RunState::Ready] {
            r.state = quiet;
            assert!(finish_notice(&r, "game").is_none(), "{quiet:?}");
        }
    }

    #[test]
    fn failed_check_messages_count_fix_attempts() {
        assert_eq!(checks_failed_message("Tests", "Codex", 0), "Tests failed after Codex finished");
        assert_eq!(checks_failed_message("Lint", "Codex", 1), "Lint still failed after one fix attempt");
        assert_eq!(checks_failed_message("Tests", "Codex", 3), "Tests still failed after 3 fix attempts");
    }

    #[test]
    fn run_and_group_ids_are_unique_within_a_second_and_stay_valid() {
        let a = new_run_id("t_8f2k1", None);
        let b = new_run_id("t_8f2k1", None);
        assert_ne!(a, b);
        assert!(a.starts_with("r_") && a.contains("_8f2k1_"), "{a}");
        let long = new_run_id(&"x".repeat(crate::model::MAX_ID_LEN), Some("claude-1"));
        assert!(crate::model::is_valid_id(&long) && long.ends_with("_claude-1"), "{long}");
        let (g1, g2) = (new_group_id("t_1"), new_group_id("t_2"));
        assert!(g1.starts_with("g_1_") && g2.starts_with("g_2_"));
        assert_ne!(new_group_id("t_1"), new_group_id("t_1"));
    }

    #[test]
    fn activity_only_changes_are_told_apart() {
        let before = record("r_1", RunState::Running);
        let mut after = before.clone();
        after.activity = Some("Edit src/a.ts".into());
        assert!(only_activity_changed(before.clone(), &after));
        after.files_changed.push("src/a.ts".into());
        assert!(!only_activity_changed(before.clone(), &after));
        assert!(!only_activity_changed(before.clone(), &before), "an unchanged record is still saved");
    }

    #[test]
    fn a_panicking_worker_still_frees_its_slot() {
        let active = Mutex::new(1usize);
        let result = std::panic::catch_unwind(|| {
            let _slot = Slot(&active);
            panic!("run blew up");
        });
        assert!(result.is_err());
        assert_eq!(*active.lock().unwrap_or_else(std::sync::PoisonError::into_inner), 0);
    }

    #[test]
    fn usage_limits_hold_only_their_own_provider() {
        use Provider::{Claude, Codex};
        assert_eq!(next_runnable(&[Claude, Codex, Claude], &[]), Some(0));
        assert_eq!(next_runnable(&[Claude, Codex, Claude], &[Claude]), Some(1));
        assert_eq!(next_runnable(&[Claude, Claude], &[Claude]), None);
        assert_eq!(next_runnable(&[], &[Codex]), None);
    }

    #[test]
    fn missing_gh_logins_and_network_errors_read_differently() {
        assert!(gh_status(None).1.contains("winget install --id GitHub.cli"));
        assert_eq!(gh_status(Some(Ok(()))), (true, "Logged in".into()));
        let logged_out = gh_status(Some(Err("You are not logged into any GitHub hosts. To log in, run: gh auth login".into())));
        assert!(!logged_out.0 && logged_out.1.contains("isn't logged in"));
        let offline = gh_status(Some(Err("error connecting to api.github.com\ncheck your internet connection".into())));
        assert!(!offline.0 && offline.1.contains("Couldn't reach GitHub"));
    }

    fn write_run(project: &Path, id: &str, state: &str) {
        let dir = run_dir(project, id);
        fs::create_dir_all(&dir).unwrap();
        let json = format!(
            r#"{{"id":"{id}","taskId":"t_1","projectId":"p_old","state":"{state}","branch":"agent/t_1","worktree":"C:/x","startedAt":"2026-11-19T10:00:00Z"}}"#
        );
        fs::write(dir.join("run.json"), json).unwrap();
    }

    #[test]
    fn committed_run_records_with_path_ids_are_skipped() {
        let root = std::env::temp_dir().join(format!("pill-agent-badid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let repo = root.join("app");
        write_run(&repo, "r_ok", "failed");
        let sneaky = run_dir(&repo, "r_sneaky");
        fs::create_dir_all(&sneaky).unwrap();
        let json = r#"{"id":"..\\..\\..","taskId":"t_1","projectId":"p","state":"failed","branch":"main","worktree":"C:/","startedAt":"x"}"#;
        fs::write(sneaky.join("run.json"), json).unwrap();
        let elsewhere = run_dir(&repo, "r_elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("run.json"), json.replace(r"..\\..\\..", "r_ok2")).unwrap();
        let bad_task = run_dir(&repo, "r_task");
        fs::create_dir_all(&bad_task).unwrap();
        fs::write(bad_task.join("run.json"), json.replace(r"..\\..\\..", "r_task").replace("t_1", r"..\\x")).unwrap();

        let agents = Agents::default();
        agents.load(&[("p".into(), repo.clone())]);
        let ids: Vec<String> = agents.list().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, ["r_ok"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn only_agent_branches_of_the_task_may_be_deleted_or_pushed() {
        let mut project = crate::workspace::test_project(Path::new(r"C:\dev\app"));
        assert!(is_agent_branch(&project, "t_1", "agent/t_1-fix-it"));
        assert!(is_agent_branch(&project, "t_1", "agent/t_1-fix-it-codex-2"));
        for bad in ["main", "agent/t_2-other", "agent/t_1", "feature/t_1-x", "agent/t_1-x/../../main"] {
            assert!(!is_agent_branch(&project, "t_1", bad), "{bad}");
        }
        assert!(!is_agent_branch(&project, r"..\x", r"agent/..\x-y"));
        project.settings.branch_prefix = "bot-".into();
        assert!(is_agent_branch(&project, "t_1", "bot-t_1-fix"));
        assert!(!is_agent_branch(&project, "t_1", "agent/t_1-fix"));
    }

    #[test]
    fn runs_follow_a_renamed_repository() {
        let root = std::env::temp_dir().join(format!("pill-agent-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let before = root.join("app");
        write_run(&before, "r_1", "pr_open");
        write_run(&before, "r_2", "running");

        let agents = Agents::default();
        agents.load(&[("p_old".into(), before.clone())]);
        assert_eq!(agents.get("r_2").unwrap().state, RunState::Interrupted);
        assert!(agents.get("r_1").is_some());

        let after = root.join("app-renamed");
        fs::rename(&before, &after).unwrap();
        let (moved, _) = agents.sync_projects(&[("p_new".into(), after.clone())]);
        assert_eq!(moved, vec![after.clone()]);
        assert_eq!(agents.get("r_1").unwrap().project_id, "p_new");
        assert!(worktree::same_path(&agents.runs.lock().unwrap()["r_1"].project_path, &after));
        // A second scan finds nothing new to move.
        assert!(agents.sync_projects(&[("p_new".into(), after.clone())]).0.is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
