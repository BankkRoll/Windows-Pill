//! Process-wide state shared by commands, the watcher and the overlay.

use crate::config::{self, Config};
use crate::watch::{self, Watch};
use crate::workspace::{Snapshot, Workspace};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, RwLock};
use tauri::{AppHandle, Emitter, Manager};

/// Event carrying a full [`Snapshot`] whenever projects or tasks change.
pub const SNAPSHOT_EVENT: &str = "pill://snapshot";
/// Event carrying the latest [`Config`] after it changes.
pub const CONFIG_EVENT: &str = "pill://config";

/// Global app state.
pub struct AppState {
    pub config_path: PathBuf,
    pub config: RwLock<Config>,
    pub config_error: RwLock<Option<String>>,
    pub workspace: Mutex<Workspace>,
    pub watch: OnceLock<Watch>,
    /// Set when the global hotkey couldn't be registered (e.g. taken by another app).
    pub hotkey_error: RwLock<Option<String>>,
}

impl AppState {
    /// Rescans roots and manual projects, re-syncs file watches and emits a snapshot.
    pub fn rescan(&self, app: &AppHandle) {
        let (roots, manual) = {
            let c = self.config.read().unwrap();
            (c.roots.clone(), c.projects.clone())
        };
        let started = std::time::Instant::now();
        let projects: Vec<(String, PathBuf)> = {
            let mut ws = self.workspace.lock().unwrap();
            ws.rescan(&roots, &manual);
            ws.project_paths().map(|(id, p)| (id.to_string(), p.to_path_buf())).collect()
        };
        log::info!("scanned {} roots: {} projects in {:?}", roots.len(), projects.len(), started.elapsed());
        // Agents are managed after the first scan at launch; later scans let
        // runs follow repositories that were renamed or moved.
        if let Some(agents) = app.try_state::<std::sync::Arc<crate::agent::Agents>>() {
            agents.after_rescan(app, &projects);
        }
        self.sync_watches();
        self.emit_snapshot(app);
    }

    /// Points the file watcher at the current set of projects.
    pub fn sync_watches(&self) {
        let Some(w) = self.watch.get() else { return };
        let roots = self.config.read().unwrap().roots.clone();
        let desired = watch::desired_paths(&self.workspace.lock().unwrap(), &roots);
        w.sync(desired);
    }

    /// Current snapshot of every project and the Inbox.
    pub fn snapshot(&self) -> Snapshot {
        self.workspace.lock().unwrap().snapshot()
    }

    /// Sends the current snapshot to the UI, and the project list to the
    /// plugins allowed to see it.
    pub fn emit_snapshot(&self, app: &AppHandle) {
        let snapshot = self.snapshot();
        if let Some(plugins) = app.try_state::<std::sync::Arc<crate::plugins::Plugins>>() {
            plugins.set_projects(crate::plugins::project_list(&snapshot));
        }
        let _ = app.emit(SNAPSHOT_EVENT, snapshot);
    }

    /// Applies `edit` to the config, saves it and notifies the UI.
    pub fn update_config(&self, app: &AppHandle, edit: impl FnOnce(&mut Config)) -> Result<Config, String> {
        let next = {
            let mut c = self.config.write().unwrap();
            let mut draft = c.clone();
            edit(&mut draft);
            let draft = draft.sanitized();
            config::save(&self.config_path, &draft).map_err(|e| e.to_string())?;
            *c = draft.clone();
            draft
        };
        *self.config_error.write().unwrap() = None;
        let _ = app.emit(CONFIG_EVENT, &next);
        Ok(next)
    }
}
