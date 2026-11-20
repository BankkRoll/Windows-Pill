//! Watches project files so edits from an editor or an agent appear live.
//!
//! Watched (all non-recursive, which stays cheap even for huge repos):
//! - every root, to notice new repositories;
//! - every project folder, to notice `.pill/` being created or the repo vanishing;
//! - every project's `.pill/` folder, for `tasks.json`, `project.json`, `notes.md`;
//! - the folder holding the global `inbox.json`.
//!
//! Events are debounced, our own writes are recognized by content hash and
//! ignored, and a full rescan runs every minute as a safety net.

use crate::state::AppState;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const DEBOUNCE: Duration = Duration::from_millis(150);
const RESCAN_EVERY: Duration = Duration::from_secs(60);

/// The OS watcher plus the set of paths it currently watches.
pub struct Watch {
    watcher: Mutex<RecommendedWatcher>,
    watched: Mutex<HashSet<PathBuf>>,
}

impl Watch {
    /// Makes the watched set match the workspace's current projects.
    pub fn sync(&self, desired: HashSet<PathBuf>) {
        let mut watched = self.watched.lock().unwrap();
        let mut watcher = self.watcher.lock().unwrap();
        for gone in watched.difference(&desired).cloned().collect::<Vec<_>>() {
            let _ = watcher.unwatch(&gone);
            watched.remove(&gone);
        }
        for path in desired {
            if !watched.contains(&path) && path.is_dir() {
                match watcher.watch(&path, RecursiveMode::NonRecursive) {
                    Ok(()) => {
                        watched.insert(path);
                    }
                    Err(e) => log::warn!("watch {}: {e}", path.display()),
                }
            }
        }
    }
}

/// Starts the watcher thread. Returns the handle used to re-sync paths.
pub fn start(app: AppHandle) -> notify::Result<Watch> {
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let watcher = notify::recommended_watcher(tx)?;
    let handle = app.clone();
    std::thread::Builder::new()
        .name("pill-watch".into())
        .spawn(move || run(handle, rx))
        .map_err(|e| notify::Error::generic(&e.to_string()))?;
    Ok(Watch { watcher: Mutex::new(watcher), watched: Mutex::new(HashSet::new()) })
}

fn run(app: AppHandle, rx: mpsc::Receiver<notify::Result<Event>>) {
    let mut last_rescan = Instant::now();
    loop {
        let wait = RESCAN_EVERY.saturating_sub(last_rescan.elapsed());
        match rx.recv_timeout(wait) {
            Ok(first) => {
                let mut paths: HashSet<PathBuf> = HashSet::new();
                collect(first, &mut paths);
                let deadline = Instant::now() + DEBOUNCE;
                while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                    match rx.recv_timeout(left) {
                        Ok(ev) => collect(ev, &mut paths),
                        Err(_) => break,
                    }
                }
                if handle_paths(&app, &paths) {
                    last_rescan = Instant::now();
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                app.state::<AppState>().rescan(&app);
                last_rescan = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn collect(event: notify::Result<Event>, into: &mut HashSet<PathBuf>) {
    match event {
        Ok(ev) => into.extend(ev.paths.into_iter().filter(|p| !is_temp(p))),
        Err(e) => log::warn!("watch error: {e}"),
    }
}

/// Atomic writes go through `*.tmp` siblings; those events are noise.
fn is_temp(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "tmp")
}

/// What a changed path means for the workspace.
#[derive(Debug, PartialEq, Eq, Hash)]
enum Effect {
    Inbox,
    Project(String),
    Rescan,
}

/// Applies a batch of changed paths. Returns true when a full rescan ran.
fn handle_paths(app: &AppHandle, paths: &HashSet<PathBuf>) -> bool {
    let state = app.state::<AppState>();
    let effects: HashSet<Effect> = {
        let ws = state.workspace.lock().unwrap();
        let config = state.config.read().unwrap();
        paths
            .iter()
            .filter_map(|p| classify(p, &ws, &config.roots))
            .filter(|effect| match effect {
                // Skip the echo of a file Pill itself just wrote.
                Effect::Inbox => !own_write(&ws, ws.inbox_path()),
                Effect::Project(_) => !paths.iter().all(|p| {
                    is_data_file(p) && own_write(&ws, p)
                }),
                Effect::Rescan => true,
            })
            .collect()
    };
    if effects.contains(&Effect::Rescan) {
        state.rescan(app);
        return true;
    }
    let mut changed = false;
    {
        let mut ws = state.workspace.lock().unwrap();
        for effect in &effects {
            changed |= match effect {
                Effect::Inbox => ws.reload_inbox(),
                Effect::Project(id) => ws.reload_project(id),
                Effect::Rescan => unreachable!(),
            };
        }
    }
    if changed {
        // A `.pill` folder may have just been created; start watching it.
        state.sync_watches();
        state.emit_snapshot(app);
    }
    false
}

fn is_data_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("tasks.json" | "project.json" | "inbox.json" | "notes.md")
    )
}

fn own_write(ws: &crate::workspace::Workspace, path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| ws.is_own_write(path, &bytes))
}

fn classify(path: &Path, ws: &crate::workspace::Workspace, roots: &[PathBuf]) -> Option<Effect> {
    if path == ws.inbox_path() {
        return Some(Effect::Inbox);
    }
    let parent = path.parent()?;
    for (id, project) in ws.project_paths() {
        let pill = project.join(".pill");
        if parent == pill || path == pill {
            return Some(Effect::Project(id.to_string()));
        }
        if path == project {
            return Some(Effect::Rescan);
        }
    }
    // A folder appearing or disappearing directly under a root may be a repo.
    if roots.iter().any(|r| r == parent) {
        return Some(Effect::Rescan);
    }
    // Changes inside a project folder other than `.pill` (source edits) are noise.
    None
}

/// Paths that should be watched for the current workspace.
pub fn desired_paths(ws: &crate::workspace::Workspace, roots: &[PathBuf]) -> HashSet<PathBuf> {
    let mut set: HashSet<PathBuf> = roots.iter().cloned().collect();
    for (_, project) in ws.project_paths() {
        set.insert(project.to_path_buf());
        set.insert(project.join(".pill"));
    }
    if let Some(dir) = ws.inbox_path().parent() {
        set.insert(dir.to_path_buf());
    }
    set
}
