//! In-memory view of every project and the global Inbox, backed by files.
//!
//! Responsibilities:
//! - Discover projects (root scan plus manually added folders).
//! - Load and validate `.pill/project.json` and `.pill/tasks.json`.
//! - Apply task mutations and write them atomically.
//! - Remember the hash of every file Pill writes, so the watcher can ignore
//!   the echo of our own writes and only react to outside edits.
//! - Maintain `.gitignore` entries for `.pill/`.
//!
//! Files on disk are the source of truth: a file that fails to parse is
//! reported on its project and is never overwritten until the user fixes it.

use crate::config::write_atomic;
use crate::model::{is_valid_id, validate_fields, IssueLink, NewTask, ProjectSettings, Task, TaskOrigin, TaskPatch, TaskStatus, TasksFile};
use crate::scan;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Colors assigned to projects without one, picked by path hash.
const PALETTE: &[&str] = &[
    "#2F7BFF", "#30D158", "#FFB340", "#FF453A", "#BF5AF2", "#64D2FF", "#FF9F0A", "#FF375F",
    "#5E5CE6", "#AC8E68",
];

/// A discovered project and its loaded data.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Stable id derived from the folder path.
    pub id: String,
    pub path: PathBuf,
    /// Display name (settings name or folder name).
    pub name: String,
    /// Resolved color.
    pub color: String,
    /// Resolved base branch.
    pub base_branch: String,
    pub settings: ProjectSettings,
    pub tasks: Vec<Task>,
    /// Contents of `.pill/notes.md` (empty if missing).
    pub notes: String,
    pub has_remote: bool,
    /// Remote agents fetch from and push to (`origin`, else the first one).
    pub remote: Option<String>,
    /// Unix seconds of the latest commit/checkout or task change.
    pub last_active: i64,
    /// Parse or IO problems with this project's files.
    pub errors: Vec<String>,
    /// True while `tasks.json` is unreadable; mutations are refused.
    #[serde(skip)]
    pub tasks_locked: bool,
    /// True when the project came from the manual list rather than a root scan.
    pub manual: bool,
}

/// Everything the UI needs to render, sent on every change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Most recently active first.
    pub projects: Vec<Project>,
    pub inbox: Vec<Task>,
    pub inbox_error: Option<String>,
}

/// A change the GitHub sync wants on one task. `None` fields are left alone.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskSync {
    pub task_id: String,
    pub status: Option<TaskStatus>,
    /// New value for the linked issue's `closed` flag.
    pub issue_closed: Option<bool>,
}

/// A change to a task's agent fields. `None` leaves a field alone.
#[derive(Debug, Clone, Default)]
pub struct AgentUpdate {
    pub status: Option<TaskStatus>,
    pub run_id: Option<String>,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    /// `Some(None)` clears the reason, `Some(Some(_))` sets it.
    pub blocked: Option<Option<String>>,
}

/// Why an agent or chat can't find its project: its folder left Pill's view.
pub const PROJECT_GONE: &str =
    "This project's folder was moved, renamed or deleted. Rescan, or add the new location in Settings.";

/// Why an agent can't find its task.
pub const TASK_GONE: &str = "This task no longer exists. It may have been deleted or moved to another project.";

/// Where a task lives: a project id, or the Inbox when `None`.
pub type Location<'a> = Option<&'a str>;

/// The full set of projects plus the Inbox.
pub struct Workspace {
    projects: BTreeMap<String, Project>,
    inbox: Vec<Task>,
    inbox_path: PathBuf,
    inbox_error: Option<String>,
    inbox_extra: TasksFile,
    /// Hash of the last bytes Pill wrote to each file.
    written: HashMap<PathBuf, u64>,
}

/// Stable project id from its path (case-insensitive, as on Windows).
pub fn project_id(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    // FNV-1a: stable across Rust versions, unlike DefaultHasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in normalized.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("p_{:010x}", h & 0xff_ffff_ffff)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Current time as RFC 3339 UTC with second precision, e.g. `2026-11-19T21:04:00Z`.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// A short random id like `t_8f2k1`.
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", random_token())
}

/// Five random lowercase letters and digits.
pub fn random_token() -> String {
    const ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buf = [0u8; 5];
    getrandom::fill(&mut buf).expect("OS random source unavailable");
    buf.iter().map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char).collect()
}

fn mtime_secs(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// Path of a project's `.pill` directory.
pub fn pill_dir(project: &Path) -> PathBuf {
    project.join(".pill")
}

/// Splits off tasks whose id fails [`is_valid_id`], returning the rest and,
/// when any were skipped, a message naming them.
fn checked_tasks(tasks: Vec<Task>) -> (Vec<Task>, Option<String>) {
    let (good, bad): (Vec<Task>, Vec<Task>) = tasks.into_iter().partition(|t| is_valid_id(&t.id));
    if bad.is_empty() {
        return (good, None);
    }
    let ids: Vec<String> = bad.iter().take(3).map(|t| format!("{:?}", t.id)).collect();
    let more = if bad.len() > 3 { format!(" and {} more", bad.len() - 3) } else { String::new() };
    let msg = format!(
        "skipped tasks with invalid ids {}{more}. Ids may only use letters, digits, `_` and `-`; fix them to edit tasks again.",
        ids.join(", ")
    );
    (good, Some(msg))
}

impl Workspace {
    /// Creates an empty workspace whose Inbox lives at `inbox_path`.
    pub fn new(inbox_path: PathBuf) -> Self {
        let mut ws = Self {
            projects: BTreeMap::new(),
            inbox: Vec::new(),
            inbox_path,
            inbox_error: None,
            inbox_extra: TasksFile::default(),
            written: HashMap::new(),
        };
        ws.reload_inbox();
        ws
    }

    /// Paths of all known projects.
    pub fn project_paths(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.projects.values().map(|p| (p.id.as_str(), p.path.as_path()))
    }

    /// Path of the Inbox file.
    pub fn inbox_path(&self) -> &Path {
        &self.inbox_path
    }

    /// Finds a project by id.
    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.get(id)
    }

    /// Re-discovers projects under `roots` plus `manual` paths. Existing
    /// projects are reloaded; vanished ones are dropped.
    pub fn rescan(&mut self, roots: &[PathBuf], manual: &[PathBuf]) {
        let mut found: Vec<(PathBuf, bool)> = Vec::new();
        let mut seen = HashSet::new();
        for root in roots {
            for repo in scan::find_repos(root) {
                if seen.insert(project_id(&repo)) {
                    found.push((repo, false));
                }
            }
        }
        for path in manual {
            if path.is_dir() && seen.insert(project_id(path)) {
                found.push((path.clone(), true));
            }
        }
        let mut next = BTreeMap::new();
        for (path, manual) in found {
            let project = self.load_project(&path, manual);
            next.insert(project.id.clone(), project);
        }
        self.projects = next;
    }

    /// Reloads one project's files from disk. Returns false when the reload
    /// changed nothing visible (e.g. the echo of our own write).
    pub fn reload_project(&mut self, id: &str) -> bool {
        let Some(old) = self.projects.get(id) else { return false };
        let (path, manual) = (old.path.clone(), old.manual);
        if !path.is_dir() {
            self.projects.remove(id);
            return true;
        }
        let fresh = self.load_project(&path, manual);
        let changed = self.projects.get(id).map(|o| {
            o.tasks != fresh.tasks
                || o.notes != fresh.notes
                || o.settings != fresh.settings
                || o.errors != fresh.errors
        });
        self.projects.insert(id.to_string(), fresh);
        changed.unwrap_or(true)
    }

    /// Whether `bytes` at `path` are exactly what Pill last wrote there.
    pub fn is_own_write(&self, path: &Path, bytes: &[u8]) -> bool {
        self.written.get(path) == Some(&hash_bytes(bytes))
    }

    fn load_project(&self, path: &Path, manual: bool) -> Project {
        let id = project_id(path);
        let dir = pill_dir(path);
        let mut errors = Vec::new();

        let settings = match fs::read_to_string(dir.join("project.json")) {
            Ok(text) => serde_json::from_str::<ProjectSettings>(&text).unwrap_or_else(|e| {
                errors.push(format!("project.json: {e}"));
                ProjectSettings::default()
            }),
            Err(_) => ProjectSettings::default(),
        };

        let notes = fs::read_to_string(dir.join("notes.md")).unwrap_or_default();
        let tasks_path = dir.join("tasks.json");
        let mut tasks_locked = false;
        let tasks = match fs::read_to_string(&tasks_path) {
            Ok(text) => match serde_json::from_str::<TasksFile>(&text) {
                Ok(file) => {
                    let (tasks, bad) = checked_tasks(file.tasks);
                    if let Some(bad) = bad {
                        log::warn!("{}: {bad}", tasks_path.display());
                        errors.push(format!("tasks.json: {bad}"));
                        // NOTE: saving would drop the skipped tasks from the
                        // file, so it stays read-only until the user fixes it.
                        tasks_locked = true;
                    }
                    tasks
                }
                Err(e) => {
                    errors.push(format!("tasks.json: {e}"));
                    tasks_locked = true;
                    // Keep showing the last good tasks rather than an empty list.
                    self.projects.get(&id).map(|p| p.tasks.clone()).unwrap_or_default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                errors.push(format!("tasks.json: {e}"));
                tasks_locked = true;
                Vec::new()
            }
        };

        let git = scan::git_info(path);
        let folder = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = if settings.name.trim().is_empty() { folder } else { settings.name.clone() };
        let color = if settings.color.is_empty() {
            let idx = usize::from_str_radix(&id[2..6], 16).unwrap_or(0) % PALETTE.len();
            PALETTE[idx].to_string()
        } else {
            settings.color.clone()
        };
        let base_branch = if settings.base_branch.is_empty() {
            git.default_branch.clone().unwrap_or_else(|| "main".into())
        } else {
            settings.base_branch.clone()
        };
        let last_active = git.last_activity.unwrap_or(0).max(mtime_secs(&tasks_path).unwrap_or(0));

        Project {
            id,
            path: path.to_path_buf(),
            name,
            color,
            base_branch,
            settings,
            tasks,
            notes,
            has_remote: git.remote.is_some(),
            remote: git.remote,
            last_active,
            errors,
            tasks_locked,
            manual,
        }
    }

    /// Reloads the Inbox. Returns false when nothing visible changed.
    pub fn reload_inbox(&mut self) -> bool {
        let before = (self.inbox.clone(), self.inbox_error.clone());
        match fs::read_to_string(&self.inbox_path) {
            Ok(text) => match serde_json::from_str::<TasksFile>(&text) {
                Ok(mut file) => {
                    let (tasks, bad) = checked_tasks(std::mem::take(&mut file.tasks));
                    if let Some(bad) = &bad {
                        log::warn!("{}: {bad}", self.inbox_path.display());
                    }
                    self.inbox = tasks;
                    self.inbox_extra = file;
                    self.inbox_error = bad.map(|b| format!("inbox.json: {b}"));
                }
                Err(e) => self.inbox_error = Some(format!("inbox.json: {e}")),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.inbox.clear();
                self.inbox_error = None;
            }
            Err(e) => self.inbox_error = Some(format!("inbox.json: {e}")),
        }
        before != (self.inbox.clone(), self.inbox_error.clone())
    }

    /// Serializable view, projects sorted by recent activity.
    pub fn snapshot(&self) -> Snapshot {
        let mut projects: Vec<Project> = self.projects.values().cloned().collect();
        projects.sort_by(|a, b| b.last_active.cmp(&a.last_active).then_with(|| a.name.cmp(&b.name)));
        Snapshot { projects, inbox: self.inbox.clone(), inbox_error: self.inbox_error.clone() }
    }

    fn tasks_mut(&mut self, at: Location) -> Result<&mut Vec<Task>, String> {
        match at {
            None => {
                if let Some(e) = &self.inbox_error {
                    return Err(format!("Fix {e} before editing the Inbox"));
                }
                Ok(&mut self.inbox)
            }
            Some(id) => {
                let p = self.projects.get_mut(id).ok_or(PROJECT_GONE)?;
                if p.tasks_locked {
                    return Err(format!(
                        "{}\\.pill\\tasks.json can't be read; fix it before editing tasks",
                        p.path.display()
                    ));
                }
                Ok(&mut p.tasks)
            }
        }
    }

    fn all_task_ids(&self) -> HashSet<&str> {
        self.inbox
            .iter()
            .chain(self.projects.values().flat_map(|p| p.tasks.iter()))
            .map(|t| t.id.as_str())
            .collect()
    }

    fn fresh_task_id(&self) -> String {
        let taken = self.all_task_ids();
        loop {
            let id = new_id("t");
            if !taken.contains(id.as_str()) {
                return id;
            }
        }
    }

    /// Creates a task in a project or the Inbox.
    pub fn create_task(&mut self, at: Location, draft: NewTask) -> Result<Task, String> {
        let title = draft.title.trim().to_string();
        if title.is_empty() {
            return Err("A task needs a title".into());
        }
        validate_fields(draft.priority, draft.due.as_deref())?;
        let id = self.fresh_task_id();
        let now = now_rfc3339();
        let status = draft.status.unwrap_or(if at.is_none() { TaskStatus::Inbox } else { TaskStatus::Todo });
        let tasks = self.tasks_mut(at)?;
        let order = tasks.iter().filter(|t| t.status == status).map(|t| t.order).fold(0.0, f64::max) + 1.0;
        let task = Task {
            id,
            title,
            notes: draft.notes,
            status,
            priority: draft.priority,
            tags: draft.tags,
            due: draft.due,
            order,
            agent_ready: draft.agent_ready,
            pinned_notes: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
            agent: Default::default(),
            issue: None,
            origin: draft.origin,
            extra: Default::default(),
        };
        tasks.push(task.clone());
        self.persist(at)?;
        Ok(task)
    }

    /// Applies a partial update to a task.
    pub fn update_task(&mut self, at: Location, id: &str, patch: TaskPatch) -> Result<Task, String> {
        validate_fields(patch.priority.flatten(), patch.due.as_ref().and_then(|d| d.as_deref()))?;
        let tasks = self.tasks_mut(at)?;
        let task = tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| format!("Unknown task {id}"))?;
        if let Some(title) = patch.title {
            let title = title.trim().to_string();
            if title.is_empty() {
                return Err("A task needs a title".into());
            }
            task.title = title;
        }
        if let Some(v) = patch.notes { task.notes = v; }
        if let Some(v) = patch.status { task.status = v; }
        if let Some(v) = patch.priority { task.priority = v; }
        if let Some(v) = patch.tags { task.tags = v; }
        if let Some(v) = patch.due { task.due = v; }
        if let Some(v) = patch.order { task.order = v; }
        if let Some(v) = patch.agent_ready {
            // Turning hands-off on is the user saying "go", so a reason that
            // held autopilot back no longer applies.
            if v && !task.agent_ready {
                task.agent.blocked = None;
            }
            task.agent_ready = v;
        }
        if let Some(v) = patch.pinned_notes { task.pinned_notes = v; }
        task.updated_at = now_rfc3339();
        let updated = task.clone();
        self.persist(at)?;
        Ok(updated)
    }

    /// Moves a task between projects and/or the Inbox. Moving out of the
    /// Inbox promotes it to To do; moving into the Inbox resets its status.
    pub fn move_task(&mut self, from: Location, id: &str, to: Location) -> Result<Task, String> {
        if from == to {
            return self.update_task(from, id, TaskPatch::default());
        }
        // Validate the destination first so a failure can't lose the task.
        self.tasks_mut(to)?;
        let source = self.tasks_mut(from)?;
        let idx = source.iter().position(|t| t.id == id).ok_or_else(|| format!("Unknown task {id}"))?;
        let mut task = source.remove(idx);
        task.status = match (to, task.status) {
            (None, _) => TaskStatus::Inbox,
            (Some(_), TaskStatus::Inbox) => TaskStatus::Todo,
            (_, s) => s,
        };
        task.updated_at = now_rfc3339();
        let dest = self.tasks_mut(to)?;
        task.order = dest.iter().filter(|t| t.status == task.status).map(|t| t.order).fold(0.0, f64::max) + 1.0;
        dest.push(task.clone());
        // Write the destination first: a crash in between duplicates the task
        // rather than losing it.
        self.persist(to)?;
        self.persist(from)?;
        Ok(task)
    }

    /// Deletes a task.
    pub fn delete_task(&mut self, at: Location, id: &str) -> Result<(), String> {
        let tasks = self.tasks_mut(at)?;
        let before = tasks.len();
        tasks.retain(|t| t.id != id);
        if tasks.len() == before {
            return Err(format!("Unknown task {id}"));
        }
        self.persist(at)
    }

    /// Records agent progress on a task: its column and agent links.
    /// `None` fields of `update` are left unchanged. Returns the task as saved.
    pub fn set_agent_state(&mut self, project_id: &str, task_id: &str, update: AgentUpdate) -> Result<Task, String> {
        let tasks = self.tasks_mut(Some(project_id))?;
        let task = tasks.iter_mut().find(|t| t.id == task_id).ok_or_else(|| format!("Unknown task {task_id}"))?;
        if let Some(s) = update.status {
            task.status = s;
        }
        if let Some(r) = update.run_id {
            task.agent.last_run_id = Some(r);
        }
        if let Some(b) = update.branch {
            task.agent.branch = Some(b);
        }
        if let Some(u) = update.pr_url {
            task.agent.pr_url = Some(u);
        }
        if let Some(b) = update.blocked {
            task.agent.blocked = b;
        }
        task.updated_at = now_rfc3339();
        let saved = task.clone();
        self.persist(Some(project_id))?;
        Ok(saved)
    }

    /// Turns a GitHub issue into a task in `status`, or returns the task
    /// already linked to it so an issue never shows up twice. The bool is
    /// true when a task was created.
    pub fn task_for_issue(
        &mut self,
        project_id: &str,
        issue: IssueLink,
        title: &str,
        tags: Vec<String>,
        status: TaskStatus,
    ) -> Result<(Task, bool), String> {
        let project = self.projects.get(project_id).ok_or_else(|| format!("Unknown project {project_id}"))?;
        if let Some(existing) = project.tasks.iter().find(|t| t.issue.as_ref().is_some_and(|i| i.number == issue.number)) {
            return Ok((existing.clone(), false));
        }
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        let title = if title.is_empty() { format!("Issue #{}", issue.number) } else { title };
        let draft = NewTask { title, status: Some(status), tags, origin: TaskOrigin::Issue, ..Default::default() };
        let created = self.create_task(Some(project_id), draft)?;
        let tasks = self.tasks_mut(Some(project_id))?;
        let task = tasks.iter_mut().find(|t| t.id == created.id).ok_or("Task vanished while linking it")?;
        task.issue = Some(issue);
        let linked = task.clone();
        self.persist(Some(project_id))?;
        Ok((linked, true))
    }

    /// Applies what the GitHub sync learned about tasks (column moves and
    /// issue state) with a single write. Returns whether anything changed.
    pub fn apply_task_sync(&mut self, project_id: &str, updates: &[TaskSync]) -> Result<bool, String> {
        if updates.is_empty() {
            return Ok(false);
        }
        let now = now_rfc3339();
        let tasks = self.tasks_mut(Some(project_id))?;
        let mut changed = false;
        for u in updates {
            let Some(task) = tasks.iter_mut().find(|t| t.id == u.task_id) else { continue };
            let before = (task.status, task.issue.as_ref().map(|i| i.closed));
            if let Some(s) = u.status {
                task.status = s;
            }
            if let (Some(closed), Some(link)) = (u.issue_closed, task.issue.as_mut()) {
                link.closed = closed;
            }
            if before != (task.status, task.issue.as_ref().map(|i| i.closed)) {
                task.updated_at = now.clone();
                changed = true;
            }
        }
        if changed {
            self.persist(Some(project_id))?;
        }
        Ok(changed)
    }

    /// Paths and settings needed to run an agent on a task.
    pub fn agent_context(&self, project_id: &str, task_id: &str) -> Result<(Project, Task), String> {
        let p = self.projects.get(project_id).ok_or(PROJECT_GONE)?;
        let t = p.tasks.iter().find(|t| t.id == task_id).ok_or(TASK_GONE)?;
        Ok((p.clone(), t.clone()))
    }

    /// Every hands-off task waiting in Inbox or To do that no failed, stopped
    /// or interrupted run is holding back, across projects.
    pub fn agent_ready_tasks(&self) -> Vec<(String, String)> {
        self.projects
            .values()
            .flat_map(|p| {
                p.tasks
                    .iter()
                    .filter(|t| t.agent_ready && t.agent.blocked.is_none() && matches!(t.status, TaskStatus::Inbox | TaskStatus::Todo))
                    .map(move |t| (p.id.clone(), t.id.clone()))
            })
            .collect()
    }

    /// Saves `.pill/notes.md`.
    ///
    /// `base` is the text the editor started from. If the file changed on
    /// disk since then (another editor or an agent wrote it), the save is
    /// refused so neither side's edits are silently lost.
    pub fn save_notes(&mut self, id: &str, text: &str, base: &str) -> Result<(), String> {
        let project = self.projects.get(id).ok_or(PROJECT_GONE)?;
        let path = pill_dir(&project.path).join("notes.md");
        let on_disk = fs::read_to_string(&path).unwrap_or_default();
        if on_disk != base && on_disk != text {
            return Err("notes.md changed on disk while you were editing".into());
        }
        self.write(&path, text.as_bytes())?;
        if let Some(p) = self.projects.get_mut(id) {
            p.notes = text.to_string();
        }
        Ok(())
    }

    /// Replaces a project's settings and writes `project.json`.
    pub fn save_settings(&mut self, id: &str, settings: ProjectSettings) -> Result<Project, String> {
        if !crate::models::valid_model(&settings.model) || !crate::models::valid_effort(&settings.effort) {
            return Err("That model or effort name isn't valid".into());
        }
        let path = self.projects.get(id).ok_or(PROJECT_GONE)?.path.clone();
        let bytes = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
        let file = pill_dir(&path).join("project.json");
        self.write(&file, &bytes)?;
        ensure_gitignore(&path, settings.commit_tasks).map_err(|e| format!(".gitignore: {e}"))?;
        let manual = self.projects[id].manual;
        let fresh = self.load_project(&path, manual);
        self.projects.insert(id.to_string(), fresh.clone());
        Ok(fresh)
    }

    fn persist(&mut self, at: Location) -> Result<(), String> {
        match at {
            None => {
                let file = TasksFile { tasks: self.inbox.clone(), ..self.inbox_extra.clone() };
                let bytes = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
                let path = self.inbox_path.clone();
                self.write(&path, &bytes)
            }
            Some(id) => {
                let p = &self.projects[id];
                // Writing would recreate a folder the user just deleted or moved.
                if !p.path.is_dir() {
                    return Err(PROJECT_GONE.into());
                }
                let path = pill_dir(&p.path).join("tasks.json");
                let (project_path, commit_tasks) = (p.path.clone(), p.settings.commit_tasks);
                // Preserve top-level unknown fields from the existing file.
                let existing = fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| serde_json::from_str::<TasksFile>(&t).ok())
                    .unwrap_or_default();
                let file = TasksFile { tasks: p.tasks.clone(), ..existing };
                let bytes = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
                self.write(&path, &bytes)?;
                ensure_gitignore(&project_path, commit_tasks).map_err(|e| format!(".gitignore: {e}"))?;
                if let Some(p) = self.projects.get_mut(id) {
                    p.last_active = p.last_active.max(chrono::Utc::now().timestamp());
                }
                Ok(())
            }
        }
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.written.insert(path.to_path_buf(), hash_bytes(bytes));
        write_atomic(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    }
}

const RUNS_LINE: &str = ".pill/runs/";
const CHATS_LINE: &str = ".pill/chats/";
const TASKS_LINE: &str = ".pill/tasks.json";

/// Makes sure `.gitignore` ignores agent run logs and chats, and ignores `tasks.json`
/// unless the project commits its tasks. Only Pill's own lines are touched.
pub fn ensure_gitignore(project: &Path, commit_tasks: bool) -> std::io::Result<()> {
    let path = project.join(".gitignore");
    let current = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let next = gitignore_with(&current, commit_tasks);
    if next != current {
        write_atomic(&path, next.as_bytes())?;
    }
    Ok(())
}

/// Pure transformation behind [`ensure_gitignore`].
pub fn gitignore_with(current: &str, commit_tasks: bool) -> String {
    let has = |line: &str| current.lines().any(|l| l.trim() == line);
    let mut lines: Vec<&str> = current.lines().collect();
    if commit_tasks {
        lines.retain(|l| l.trim() != TASKS_LINE);
    }
    let mut add = Vec::new();
    if !has(RUNS_LINE) {
        add.push(RUNS_LINE);
    }
    if !has(CHATS_LINE) {
        add.push(CHATS_LINE);
    }
    if !commit_tasks && !has(TASKS_LINE) {
        add.push(TASKS_LINE);
    }
    if !add.is_empty() {
        if lines.last().is_some_and(|l| !l.trim().is_empty()) {
            lines.push("");
        }
        if !current.contains("# Pill") {
            lines.push("# Pill");
        }
        lines.extend(add);
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// A project at `path` with default settings and no tasks, for tests.
#[cfg(test)]
pub(crate) fn test_project(path: &Path) -> Project {
    Project {
        id: project_id(path),
        path: path.to_path_buf(),
        name: "app".into(),
        color: String::new(),
        base_branch: "main".into(),
        settings: Default::default(),
        tasks: Vec::new(),
        notes: String::new(),
        has_remote: false,
        remote: None,
        last_active: 0,
        errors: Vec::new(),
        tasks_locked: false,
        manual: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pill-ws-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn repo(root: &Path, name: &str) -> PathBuf {
        let p = root.join(name);
        fs::create_dir_all(p.join(".git")).unwrap();
        p
    }

    fn draft(title: &str) -> NewTask {
        NewTask { title: title.into(), ..Default::default() }
    }

    #[test]
    fn deleted_projects_are_never_recreated_by_a_write() {
        let root = tmp("gone");
        let r = repo(&root, "app");
        let mut ws = Workspace::new(root.join("inbox.json"));
        ws.rescan(std::slice::from_ref(&root), &[]);
        let pid = project_id(&r);
        fs::remove_dir_all(&r).unwrap();
        assert_eq!(ws.create_task(Some(&pid), draft("x")).unwrap_err(), PROJECT_GONE);
        assert!(!r.exists());
        assert!(ws.reload_project(&pid));
        assert!(ws.project(&pid).is_none());
        assert_eq!(ws.agent_context(&pid, "t_1").unwrap_err(), PROJECT_GONE);
        ws.rescan(std::slice::from_ref(&root), &[]);
        assert_eq!(ws.project_paths().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ids_are_stable_and_case_insensitive() {
        assert_eq!(project_id(Path::new(r"C:\dev\Outlaw")), project_id(Path::new(r"c:\dev\outlaw\")));
        assert_ne!(project_id(Path::new(r"C:\dev\a")), project_id(Path::new(r"C:\dev\b")));
    }

    #[test]
    fn task_lifecycle_round_trips_through_disk() {
        let root = tmp("life");
        let r = repo(&root, "game");
        let mut ws = Workspace::new(root.join("inbox.json"));
        ws.rescan(std::slice::from_ref(&root), &[]);
        let pid = project_id(&r);

        let t = ws.create_task(Some(&pid), NewTask { priority: Some(1), ..draft("Fix scatter") }).unwrap();
        assert_eq!(t.status, TaskStatus::Todo);
        let inbox = ws.create_task(None, draft("Someday")).unwrap();
        assert_eq!(inbox.status, TaskStatus::Inbox);

        let moved = ws.move_task(None, &inbox.id, Some(&pid)).unwrap();
        assert_eq!(moved.status, TaskStatus::Todo);
        assert!(ws.snapshot().inbox.is_empty());

        let patch: TaskPatch = serde_json::from_str(r#"{"status":"doing","due":"2026-11-21"}"#).unwrap();
        ws.update_task(Some(&pid), &t.id, patch).unwrap();

        // A fresh workspace sees the same data on disk.
        let mut again = Workspace::new(root.join("inbox.json"));
        again.rescan(std::slice::from_ref(&root), &[]);
        let p = again.project(&pid).unwrap();
        assert_eq!(p.tasks.len(), 2);
        assert_eq!(p.tasks.iter().find(|x| x.id == t.id).unwrap().status, TaskStatus::Doing);

        let gi = fs::read_to_string(r.join(".gitignore")).unwrap();
        assert!(gi.contains(".pill/runs/") && gi.contains(".pill/tasks.json"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn blocked_hands_off_tasks_wait_until_the_user_says_go() {
        let root = tmp("handsoff");
        let r = repo(&root, "game");
        let mut ws = Workspace::new(root.join("inbox.json"));
        ws.rescan(std::slice::from_ref(&root), &[]);
        let pid = project_id(&r);
        let t = ws.create_task(Some(&pid), NewTask { agent_ready: true, ..draft("Fix scatter") }).unwrap();
        ws.create_task(Some(&pid), draft("Manual only")).unwrap();
        assert_eq!(ws.agent_ready_tasks(), vec![(pid.clone(), t.id.clone())]);

        let failed = AgentUpdate {
            status: Some(TaskStatus::Todo),
            run_id: Some("r_1".into()),
            blocked: Some(Some("Tests failed".into())),
            ..Default::default()
        };
        let saved = ws.set_agent_state(&pid, &t.id, failed).unwrap();
        assert_eq!(saved.agent.blocked.as_deref(), Some("Tests failed"));
        assert_eq!(saved.agent.last_run_id.as_deref(), Some("r_1"));
        assert!(ws.agent_ready_tasks().is_empty());

        // Toggling hands-off off and on again is the user saying "go".
        let off: TaskPatch = serde_json::from_str(r#"{"agentReady":false}"#).unwrap();
        ws.update_task(Some(&pid), &t.id, off).unwrap();
        let on: TaskPatch = serde_json::from_str(r#"{"agentReady":true}"#).unwrap();
        let task = ws.update_task(Some(&pid), &t.id, on).unwrap();
        assert_eq!(task.agent.blocked, None);
        assert_eq!(ws.agent_ready_tasks().len(), 1);

        let shipped = AgentUpdate {
            status: Some(TaskStatus::Review),
            pr_url: Some("https://github.com/x/y/pull/1".into()),
            ..Default::default()
        };
        ws.set_agent_state(&pid, &t.id, shipped).unwrap();
        assert!(ws.agent_ready_tasks().is_empty(), "Review is the user's turn");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_tasks_file_is_locked_not_overwritten() {
        let root = tmp("bad");
        let r = repo(&root, "p");
        fs::create_dir_all(r.join(".pill")).unwrap();
        fs::write(r.join(".pill/tasks.json"), "{ broken").unwrap();
        let mut ws = Workspace::new(root.join("inbox.json"));
        ws.rescan(std::slice::from_ref(&root), &[]);
        let pid = project_id(&r);
        assert!(!ws.project(&pid).unwrap().errors.is_empty());
        assert!(ws.create_task(Some(&pid), draft("x")).is_err());
        assert_eq!(fs::read_to_string(r.join(".pill/tasks.json")).unwrap(), "{ broken");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn committed_tasks_with_path_ids_are_skipped_and_never_rewritten() {
        let root = tmp("badid");
        let r = repo(&root, "p");
        fs::create_dir_all(r.join(".pill")).unwrap();
        let text = r#"{"version":1,"tasks":[
            {"id":"t_ok","title":"Fine","status":"todo","createdAt":"a","updatedAt":"a"},
            {"id":"..\\..\\..","title":"Climb","status":"todo","agentReady":true,"createdAt":"a","updatedAt":"a"}
        ]}"#;
        fs::write(r.join(".pill/tasks.json"), text).unwrap();
        let mut ws = Workspace::new(root.join("inbox.json"));
        ws.rescan(std::slice::from_ref(&root), &[]);
        let pid = project_id(&r);
        let p = ws.project(&pid).unwrap();
        assert_eq!(p.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["t_ok"]);
        assert!(p.errors.iter().any(|e| e.contains("invalid ids")), "{:?}", p.errors);
        assert!(ws.agent_ready_tasks().is_empty());
        assert_eq!(ws.agent_context(&pid, r"..\..\..").unwrap_err(), TASK_GONE);
        assert!(ws.create_task(Some(&pid), draft("x")).is_err());
        assert_eq!(fs::read_to_string(r.join(".pill/tasks.json")).unwrap(), text);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn own_writes_are_recognized() {
        let root = tmp("echo");
        let r = repo(&root, "p");
        let mut ws = Workspace::new(root.join("inbox.json"));
        ws.rescan(std::slice::from_ref(&root), &[]);
        let pid = project_id(&r);
        ws.create_task(Some(&pid), draft("x")).unwrap();
        let path = r.join(".pill/tasks.json");
        assert!(ws.is_own_write(&path, &fs::read(&path).unwrap()));
        assert!(!ws.is_own_write(&path, b"{}"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn notes_save_refuses_to_clobber_outside_edits() {
        let root = tmp("notes");
        let r = repo(&root, "p");
        let mut ws = Workspace::new(root.join("inbox.json"));
        ws.rescan(std::slice::from_ref(&root), &[]);
        let pid = project_id(&r);
        ws.save_notes(&pid, "# Ideas\n", "").unwrap();
        assert_eq!(ws.project(&pid).unwrap().notes, "# Ideas\n");
        fs::write(r.join(".pill/notes.md"), "# Changed elsewhere\n").unwrap();
        assert!(ws.save_notes(&pid, "# Mine\n", "# Ideas\n").is_err());
        assert_eq!(fs::read_to_string(r.join(".pill/notes.md")).unwrap(), "# Changed elsewhere\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn issues_become_one_task_and_sync_back() {
        let root = tmp("issues");
        let r = repo(&root, "p");
        let mut ws = Workspace::new(root.join("inbox.json"));
        ws.rescan(std::slice::from_ref(&root), &[]);
        let pid = project_id(&r);
        let link = || IssueLink { number: 7, url: "https://github.com/o/r/issues/7".into(), closed: false, extra: Default::default() };

        let (task, created) = ws.task_for_issue(&pid, link(), "  Crash\n on save ", vec!["bug".into()], TaskStatus::Todo).unwrap();
        assert!(created);
        assert_eq!(task.title, "Crash on save");
        assert_eq!(task.issue.as_ref().map(|i| i.number), Some(7));
        let (again, created) = ws.task_for_issue(&pid, link(), "Other title", Vec::new(), TaskStatus::Doing).unwrap();
        assert!(!created);
        assert_eq!(again.id, task.id);
        assert_eq!(ws.project(&pid).unwrap().tasks.len(), 1);

        let sync = TaskSync { task_id: task.id.clone(), status: Some(TaskStatus::Done), issue_closed: Some(true) };
        assert!(ws.apply_task_sync(&pid, std::slice::from_ref(&sync)).unwrap());
        assert!(!ws.apply_task_sync(&pid, &[sync]).unwrap());

        let mut fresh = Workspace::new(root.join("inbox.json"));
        fresh.rescan(std::slice::from_ref(&root), &[]);
        let saved = &fresh.project(&pid).unwrap().tasks[0];
        assert_eq!(saved.status, TaskStatus::Done);
        assert!(saved.issue.as_ref().unwrap().closed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_empty_titles_and_bad_fields() {
        let root = tmp("val");
        let mut ws = Workspace::new(root.join("inbox.json"));
        assert!(ws.create_task(None, draft("   ")).is_err());
        assert!(ws.create_task(None, NewTask { priority: Some(9), ..draft("x") }).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gitignore_edits_are_minimal_and_idempotent() {
        let once = gitignore_with("node_modules/\n", false);
        assert_eq!(once, "node_modules/\n\n# Pill\n.pill/runs/\n.pill/chats/\n.pill/tasks.json\n");
        assert_eq!(gitignore_with(&once, false), once);
        let committed = gitignore_with(&once, true);
        assert!(!committed.contains(".pill/tasks.json"));
        assert!(committed.contains(".pill/runs/"));
        assert_eq!(gitignore_with("", true), "# Pill\n.pill/runs/\n.pill/chats/\n");
    }
}
