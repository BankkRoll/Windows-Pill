//! IPC commands invoked by the UI.
//!
//! Every mutation writes to disk first, then emits a fresh snapshot, so the
//! UI always renders what's actually on disk.

use crate::github::{self, Github, ProjectGithub};
use crate::model::{IssueLink, NewTask, ProjectSettings, Task, TaskPatch, TaskStatus};
use crate::overlay::{HitRect, OverlayState};
use crate::state::AppState;
use crate::workspace::{Project, Snapshot};
use crate::agent::{self, Agents, Check, LogLine, RunRecord};
use crate::chat::{self, Chat, ChatMode, Chats};
use crate::plugins::{page::ButtonKind, protocol::Severity, PageState, PluginState, Plugins, Source};
use crate::system_settings::SystemPage;
use crate::voice::{Voice, VoiceError};
use crate::{config::Config, editor, hotkey, prompt, scan};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

type CmdResult<T> = Result<T, String>;

/// Initial data the UI needs before its first render.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    config: Config,
    config_error: Option<String>,
    config_path: PathBuf,
    hotkey_error: Option<String>,
    snapshot: Snapshot,
    /// Whether Windows asks apps for light mode, for `Appearance::System`.
    system_light: bool,
}

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Bootstrap {
    Bootstrap {
        config: state.config.read().unwrap().clone(),
        config_error: state.config_error.read().unwrap().clone(),
        config_path: state.config_path.clone(),
        hotkey_error: state.hotkey_error.read().unwrap().clone(),
        snapshot: state.snapshot(),
        system_light: crate::appearance::system_light(),
    }
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    state.snapshot()
}

#[tauri::command]
pub fn create_task(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: Option<String>,
    task: NewTask,
) -> CmdResult<Task> {
    let created = state.workspace.lock().unwrap().create_task(project_id.as_deref(), task)?;
    state.emit_snapshot(&app);
    Ok(created)
}

#[tauri::command]
pub fn update_task(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: Option<String>,
    task_id: String,
    patch: TaskPatch,
) -> CmdResult<Task> {
    let updated = state.workspace.lock().unwrap().update_task(project_id.as_deref(), &task_id, patch)?;
    state.emit_snapshot(&app);
    Ok(updated)
}

#[tauri::command]
pub fn move_task(
    app: AppHandle,
    state: State<'_, AppState>,
    from_project_id: Option<String>,
    task_id: String,
    to_project_id: Option<String>,
) -> CmdResult<Task> {
    let moved = state.workspace.lock().unwrap().move_task(
        from_project_id.as_deref(),
        &task_id,
        to_project_id.as_deref(),
    )?;
    state.emit_snapshot(&app);
    Ok(moved)
}

#[tauri::command]
pub fn delete_task(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: Option<String>,
    task_id: String,
) -> CmdResult<()> {
    state.workspace.lock().unwrap().delete_task(project_id.as_deref(), &task_id)?;
    state.emit_snapshot(&app);
    // A deleted task's finished runs would only leave worktrees and branches
    // behind. Running ones keep going; the user can stop and delete them.
    std::thread::spawn(move || {
        let agents = app.state::<Arc<Agents>>();
        for run in agents.list().into_iter().filter(|r| r.task_id == task_id && !r.state.is_active()) {
            if let Err(e) = agents.delete(&app, &run.id) {
                log::warn!("deleting run {} of a deleted task: {e}", run.id);
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn save_project_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    settings: ProjectSettings,
) -> CmdResult<Project> {
    let project = state.workspace.lock().unwrap().save_settings(&project_id, settings)?;
    state.emit_snapshot(&app);
    app.state::<Arc<Github>>().request(Some(project_id));
    Ok(project)
}

#[tauri::command]
pub fn save_notes(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    text: String,
    base: String,
) -> CmdResult<()> {
    state.workspace.lock().unwrap().save_notes(&project_id, &text, &base)?;
    state.emit_snapshot(&app);
    Ok(())
}

/// Opens the project in its configured (or auto-detected) editor.
///
/// Returns the editor's display name, e.g. "Cursor".
#[tauri::command]
pub fn open_in_editor(state: State<'_, AppState>, project_id: String) -> CmdResult<String> {
    let (path, choice) = {
        let ws = state.workspace.lock().unwrap();
        let p = ws.project(&project_id).ok_or(crate::workspace::PROJECT_GONE)?;
        (p.path.clone(), p.settings.editor)
    };
    let editor = editor::resolve(choice);
    editor::open(&editor, &path).map_err(|e| format!("Couldn't open {}: {e}", editor::label(&editor)))?;
    Ok(editor::label(&editor).to_string())
}

/// Opens the project's folder in Explorer.
///
/// SECURITY: the UI names a project, never a path, so only folders Pill
/// already tracks can be opened; the opener's own path scope stays closed.
#[tauri::command]
pub fn open_project_folder(app: AppHandle, state: State<'_, AppState>, project_id: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let path = state.workspace.lock().unwrap().project(&project_id).ok_or(crate::workspace::PROJECT_GONE)?.path.clone();
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| format!("Couldn't open the folder: {e}"))
}

/// Which editors are installed, for the settings UI.
#[derive(Serialize)]
pub struct EditorsFound {
    cursor: bool,
    code: bool,
}

#[tauri::command]
pub fn detect_editors() -> EditorsFound {
    EditorsFound { cursor: editor::find_cursor().is_some(), code: editor::find_code().is_some() }
}

/// Renders the exact prompt an agent would get for a task.
#[tauri::command]
pub fn render_task_prompt(
    state: State<'_, AppState>,
    github: State<'_, Arc<Github>>,
    project_id: String,
    task_id: String,
) -> CmdResult<String> {
    let ws = state.workspace.lock().unwrap();
    let p = ws.project(&project_id).ok_or(crate::workspace::PROJECT_GONE)?;
    let task = p.tasks.iter().find(|t| t.id == task_id).ok_or_else(|| format!("Unknown task {task_id}"))?;
    let branch = prompt::branch_name(&p.settings.agent_branch_prefix(), task);
    let pr_body = format!(".pill/runs/manual-{}/pr.md", task.id);
    // Copying stays instant: it uses the synced copy of the issue, while a
    // real run reads the issue fresh from GitHub.
    let cached = task.issue.as_ref().and_then(|link| github.cached_issue(&project_id, link.number));
    let issue = task.issue.as_ref().map(|link| prompt::IssuePrompt {
        number: link.number,
        url: &link.url,
        title: cached.as_ref().map_or(task.title.as_str(), |i| i.title.as_str()),
        body: cached.as_ref().map_or("", |i| i.body.as_str()),
    });
    Ok(prompt::render(&prompt::PromptContext {
        project_name: &p.name,
        branch: &branch,
        task_title: &task.title,
        task_notes: &task.notes,
        pinned_notes: &task.pinned_notes,
        base_branch: &p.base_branch,
        test_command: &p.settings.test_command,
        pr_body_path: &pr_body,
        project_rules: &p.settings.agent_rules,
        plan_only: false,
        origin: task.source(),
        issue,
    }))
}

#[tauri::command]
pub fn set_roots(app: AppHandle, state: State<'_, AppState>, roots: Vec<PathBuf>) -> CmdResult<Config> {
    let mut clean: Vec<PathBuf> = Vec::new();
    for r in roots {
        if !r.is_dir() {
            return Err(format!("{} is not a folder", r.display()));
        }
        if !clean.contains(&r) {
            clean.push(r);
        }
    }
    let config = state.update_config(&app, |c| c.roots = clean)?;
    state.rescan(&app);
    Ok(config)
}

#[tauri::command]
pub fn add_project(app: AppHandle, state: State<'_, AppState>, path: PathBuf) -> CmdResult<Config> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    if !scan::is_repo(&path) {
        return Err(format!("{} is not a git repository", path.display()));
    }
    let config = state.update_config(&app, |c| {
        if !c.projects.contains(&path) {
            c.projects.push(path);
        }
    })?;
    state.rescan(&app);
    Ok(config)
}

#[tauri::command]
pub fn remove_project(app: AppHandle, state: State<'_, AppState>, path: PathBuf) -> CmdResult<Config> {
    let config = state.update_config(&app, |c| c.projects.retain(|p| p != &path))?;
    state.rescan(&app);
    Ok(config)
}

#[tauri::command]
pub fn rescan(app: AppHandle, state: State<'_, AppState>) {
    state.rescan(&app);
}

#[tauri::command]
pub fn list_drives() -> Vec<PathBuf> {
    scan::available_drives()
}

#[tauri::command]
pub fn set_hotkey(app: AppHandle, state: State<'_, AppState>, hotkey: String) -> CmdResult<Config> {
    let previous = state.config.read().unwrap().hotkey.clone();
    hotkey::register(&app, &hotkey, Some(&previous))?;
    *state.hotkey_error.write().unwrap() = None;
    state.update_config(&app, |c| c.hotkey = hotkey)
}

#[tauri::command]
pub fn set_hit_rect(rect: Option<HitRect>, overlay: State<'_, Arc<OverlayState>>) {
    *overlay.hit.lock().unwrap() = rect;
}

#[tauri::command]
pub fn focus_window(window: WebviewWindow, overlay: State<'_, Arc<OverlayState>>) {
    #[cfg(windows)]
    crate::overlay::take_focus(&window, &overlay);
    #[cfg(not(windows))]
    let _ = (window.set_focus(), overlay);
}

#[tauri::command]
pub fn release_focus(window: WebviewWindow, overlay: State<'_, Arc<OverlayState>>) {
    #[cfg(windows)]
    crate::overlay::release_focus(&window, &overlay);
    #[cfg(not(windows))]
    let _ = (window, overlay);
}

// -----------------------------------------------------------------------------
// Agents
// -----------------------------------------------------------------------------

#[tauri::command]
pub fn list_runs(agents: State<'_, Arc<Agents>>) -> Vec<RunRecord> {
    agents.list()
}

#[tauri::command]
pub fn run_log(agents: State<'_, Arc<Agents>>, run_id: String) -> Vec<LogLine> {
    agents.log(&run_id)
}

/// The prompt a run's agent was given, follow-ups included.
#[tauri::command]
pub fn run_prompt(agents: State<'_, Arc<Agents>>, run_id: String) -> Option<String> {
    agents.prompt(&run_id)
}

/// Starts an agent on a task. `plan` overrides the project's plan-mode default.
#[tauri::command]
pub fn start_agent(
    app: AppHandle,
    agents: State<'_, Arc<Agents>>,
    project_id: String,
    task_id: String,
    plan: Option<bool>,
    choice: Option<crate::models::RunChoice>,
) -> CmdResult<RunRecord> {
    agents.start(&app, &project_id, &task_id, plan, choice.unwrap_or_default())
}

/// Starts one task once per setup to compare the results.
#[tauri::command]
pub fn compare_agents(
    app: AppHandle,
    agents: State<'_, Arc<Agents>>,
    project_id: String,
    task_id: String,
    choices: Vec<crate::models::RunChoice>,
) -> CmdResult<Vec<RunRecord>> {
    agents.compare(&app, &project_id, &task_id, choices)
}

/// Ships the picked run of a comparison and discards the rest.
#[tauri::command]
pub async fn pick_run(app: AppHandle, run_id: String) -> CmdResult<()> {
    // PERF: picking removes the other runs' worktrees, which can take seconds.
    tauri::async_runtime::spawn_blocking(move || app.state::<Arc<Agents>>().pick(&app, &run_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn stop_agent(app: AppHandle, agents: State<'_, Arc<Agents>>, run_id: String) -> CmdResult<()> {
    agents.stop(&app, &run_id)
}

#[tauri::command]
pub fn retry_agent(app: AppHandle, agents: State<'_, Arc<Agents>>, run_id: String, feedback: String) -> CmdResult<()> {
    agents.retry(&app, &run_id, feedback)
}

#[tauri::command]
pub fn approve_plan(app: AppHandle, agents: State<'_, Arc<Agents>>, run_id: String, plan: String) -> CmdResult<()> {
    agents.approve(&app, &run_id, plan)
}

#[tauri::command]
pub async fn cleanup_run(app: AppHandle, run_id: String) -> CmdResult<()> {
    // Removing a worktree can take a while on big repos; keep it off the IPC thread.
    tauri::async_runtime::spawn_blocking(move || app.state::<Arc<Agents>>().cleanup(&app, &run_id))
        .await
        .map_err(|e| e.to_string())?
}

/// Deletes a finished run with its worktree and history; the task stays.
#[tauri::command]
pub async fn delete_run(app: AppHandle, run_id: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Arc<Agents>>().delete(&app, &run_id))
        .await
        .map_err(|e| e.to_string())?
}

/// Deletes every failed, stopped or interrupted run (in one project, when
/// given). Returns how many were deleted.
#[tauri::command]
pub async fn clear_failed_runs(app: AppHandle, project_id: Option<String>) -> CmdResult<usize> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Arc<Agents>>().clear_failed(&app, project_id.as_deref()))
        .await
        .map_err(|e| e.to_string())
}

/// Read-only repository facts for a project's settings page: plain folder or
/// repo, remote, branches, working-tree state and `gh` login.
#[tauri::command]
pub async fn project_git_info(state: State<'_, AppState>, project_id: String) -> CmdResult<crate::gitinfo::RepoInfo> {
    let path = {
        let ws = state.workspace.lock().unwrap();
        ws.project(&project_id).map(|p| p.path.clone()).ok_or(crate::workspace::PROJECT_GONE)?
    };
    tauri::async_runtime::spawn_blocking(move || crate::gitinfo::repo_info(&path)).await.map_err(|e| e.to_string())
}

/// Test and lint commands found in a project's files, offered when its
/// settings have none.
#[tauri::command]
pub async fn detect_checks(state: State<'_, AppState>, project_id: String) -> CmdResult<crate::checks::Detected> {
    let path = {
        let ws = state.workspace.lock().unwrap();
        ws.project(&project_id).map(|p| p.path.clone()).ok_or(crate::workspace::PROJECT_GONE)?
    };
    tauri::async_runtime::spawn_blocking(move || crate::checks::detect(&path)).await.map_err(|e| e.to_string())
}

/// Runs the preflight checks for a project without starting anything.
#[tauri::command]
pub async fn preflight(state: State<'_, AppState>, project_id: String) -> CmdResult<Vec<Check>> {
    let project = {
        let ws = state.workspace.lock().unwrap();
        ws.project(&project_id).cloned().ok_or(crate::workspace::PROJECT_GONE)?
    };
    let provider = project.settings.provider;
    tauri::async_runtime::spawn_blocking(move || agent::preflight(&project, provider)).await.map_err(|e| e.to_string())
}

/// Opens a run's worktree in the project's editor.
#[tauri::command]
pub fn open_worktree(state: State<'_, AppState>, agents: State<'_, Arc<Agents>>, run_id: String) -> CmdResult<String> {
    let run = agents.get(&run_id).ok_or("Unknown run")?;
    if !run.worktree.is_dir() {
        return Err("This run's worktree has been cleaned up".into());
    }
    let choice = state
        .workspace
        .lock()
        .unwrap()
        .project(&run.project_id)
        .map(|p| p.settings.editor)
        .unwrap_or_default();
    let ed = editor::resolve(choice);
    editor::open(&ed, &run.worktree).map_err(|e| e.to_string())?;
    Ok(editor::label(&ed).to_string())
}

// -----------------------------------------------------------------------------
// Settings
// -----------------------------------------------------------------------------

/// Global settings changeable from the Settings tab. Absent fields are kept.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatch {
    #[serde(deserialize_with = "crate::model::double_option")]
    monitor: Option<Option<usize>>,
    launch_at_startup: Option<bool>,
    accent: Option<String>,
    hover_open_delay_ms: Option<u32>,
    hover_close_delay_ms: Option<u32>,
    agent: Option<crate::config::AgentLimits>,
    autopilot: Option<crate::config::Autopilot>,
    models: Option<crate::models::ModelDefaults>,
    theme: Option<crate::config::Theme>,
    appearance: Option<crate::config::Appearance>,
    glass_tint: Option<crate::config::GlassTint>,
    open_on: Option<crate::config::OpenOn>,
    on_top: Option<bool>,
    glass_source: Option<crate::config::GlassSource>,
    glass_gradient: Option<String>,
    #[serde(deserialize_with = "crate::model::double_option")]
    glass_image: Option<Option<std::path::PathBuf>>,
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    overlay: State<'_, Arc<OverlayState>>,
    patch: SettingsPatch,
) -> CmdResult<Config> {
    if let Some(on) = patch.on_top {
        overlay.on_top.store(on, std::sync::atomic::Ordering::Relaxed);
    }
    if let Some(on) = patch.launch_at_startup {
        use tauri_plugin_autostart::ManagerExt;
        let launcher = app.autolaunch();
        let result = if on { launcher.enable() } else { launcher.disable() };
        result.map_err(|e| format!("Couldn't change launch at startup: {e}"))?;
    }
    if let Some(Some(path)) = &patch.glass_image {
        crate::glass_image::check_image_file(path)?;
    }
    let monitor_changed = patch.monitor.is_some();
    let backdrop_changed = patch.theme.is_some() || patch.glass_source.is_some() || patch.glass_image.is_some();
    let palette_changed = patch.theme.is_some() || patch.appearance.is_some();
    let config = state.update_config(&app, |c| {
        if let Some(m) = patch.monitor {
            c.monitor = m;
        }
        if let Some(v) = patch.launch_at_startup {
            c.launch_at_startup = v;
        }
        if let Some(v) = patch.accent {
            c.accent = v;
        }
        if let Some(v) = patch.hover_open_delay_ms {
            c.hover_open_delay_ms = v;
        }
        if let Some(v) = patch.hover_close_delay_ms {
            c.hover_close_delay_ms = v;
        }
        if let Some(v) = patch.agent {
            c.agent = v;
        }
        if let Some(v) = patch.autopilot {
            c.autopilot = v;
        }
        if let Some(v) = patch.models {
            c.models = v.sanitized();
        }
        if let Some(v) = patch.theme {
            c.theme = v;
        }
        if let Some(v) = patch.appearance {
            c.appearance = v;
        }
        if let Some(v) = patch.glass_tint {
            c.glass_tint = v;
        }
        if let Some(v) = patch.open_on {
            c.open_on = v;
        }
        if let Some(v) = patch.on_top {
            c.on_top = v;
        }
        if let Some(v) = patch.glass_source {
            c.glass_source = v;
        }
        if let Some(v) = patch.glass_gradient {
            c.glass_gradient = v;
        }
        if let Some(v) = patch.glass_image {
            c.glass_image = v;
        }
    })?;
    if backdrop_changed {
        crate::detached::refresh_backdrop();
    }
    if palette_changed {
        crate::appearance::sync(&app);
    }
    if monitor_changed {
        *overlay.monitor.lock().unwrap() = config.monitor;
        overlay.replace_requested.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(config)
}

/// A connected display, for the monitor picker.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    index: usize,
    name: String,
    width: u32,
    height: u32,
    scale: f64,
    primary: bool,
}

#[tauri::command]
pub fn list_monitors(window: WebviewWindow) -> Vec<MonitorInfo> {
    let primary = window.primary_monitor().ok().flatten();
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, m)| MonitorInfo {
            index,
            name: m.name().cloned().unwrap_or_else(|| format!("Display {}", index + 1)),
            width: m.size().width,
            height: m.size().height,
            scale: m.scale_factor(),
            primary: primary.as_ref().is_some_and(|p| p.position() == m.position() && p.size() == m.size()),
        })
        .collect()
}

/// Install and login status of Claude Code and the GitHub CLI.
#[tauri::command]
pub async fn cli_status() -> CmdResult<Vec<Check>> {
    tauri::async_runtime::spawn_blocking(agent::cli_status).await.map_err(|e| e.to_string())
}

// -----------------------------------------------------------------------------
// Chat
// -----------------------------------------------------------------------------

#[tauri::command]
pub fn list_chats(app: AppHandle, chats: State<'_, Arc<Chats>>, project_id: String) -> CmdResult<Vec<Chat>> {
    Ok(chats.list(&chat::project_for(&app, &project_id)?))
}

#[tauri::command]
pub fn create_chat(app: AppHandle, chats: State<'_, Arc<Chats>>, project_id: String, mode: ChatMode) -> CmdResult<Chat> {
    chats.create(&chat::project_for(&app, &project_id)?, mode)
}

#[tauri::command]
pub fn set_chat_mode(app: AppHandle, chats: State<'_, Arc<Chats>>, project_id: String, chat_id: String, mode: ChatMode) -> CmdResult<Chat> {
    let project = chat::project_for(&app, &project_id)?;
    chats.set_mode(&project, &chat_id, mode)
}

/// Sends a chat message; the reply streams as events.
#[tauri::command]
pub fn send_chat(
    app: AppHandle,
    chats: State<'_, Arc<Chats>>,
    project_id: String,
    chat_id: String,
    text: String,
    choice: Option<crate::models::RunChoice>,
) -> CmdResult<Chat> {
    let project = chat::project_for(&app, &project_id)?;
    chats.inner().send(&app, project, &chat_id, text, choice)
}

/// The latest wallpaper sample of the notch's monitor, for the glass theme.
#[tauri::command]
pub fn get_backdrop() -> Option<crate::backdrop::Backdrop> {
    crate::detached::latest_backdrop()
}

/// The wallpaper of the calling window's monitor, for its glass background.
#[tauri::command]
pub async fn sample_backdrop(window: WebviewWindow) -> Option<crate::backdrop::Backdrop> {
    tauri::async_runtime::spawn_blocking(move || crate::detached::wallpaper_for(&window)).await.ok().flatten()
}

/// The wallpapers Windows ships, for the glass backdrop picker.
#[tauri::command]
pub async fn stock_wallpapers() -> Vec<crate::glass_image::StockWallpaper> {
    tauri::async_runtime::spawn_blocking(crate::glass_image::stock_wallpapers).await.unwrap_or_default()
}

/// A small thumbnail of an image file as a `data:` URL.
#[tauri::command]
pub async fn image_thumbnail(path: std::path::PathBuf) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || crate::glass_image::thumbnail(&path))
        .await
        .map_err(|e| e.to_string())?
}

/// Models and efforts each provider offers. `refresh` re-reads the Codex catalog.
#[tauri::command]
pub async fn model_catalog(refresh: bool) -> crate::models::Catalog {
    tauri::async_runtime::spawn_blocking(move || crate::models::catalog(refresh))
        .await
        .unwrap_or_else(|e| crate::models::Catalog { claude: Vec::new(), claude_default: None, claude_error: Some(e.to_string()), codex: Vec::new(), codex_error: Some(e.to_string()) })
}

#[tauri::command]
pub fn stop_chat(chats: State<'_, Arc<Chats>>, chat_id: String) {
    chats.stop(&chat_id);
}

#[tauri::command]
pub async fn delete_chat(app: AppHandle, project_id: String, chat_id: String) -> CmdResult<()> {
    let project = chat::project_for(&app, &project_id)?;
    tauri::async_runtime::spawn_blocking(move || app.state::<Arc<Chats>>().delete(&project, &chat_id))
        .await
        .map_err(|e| e.to_string())?
}

/// Pushes an edit-mode chat's changes and opens a draft PR.
#[tauri::command]
pub async fn ship_chat(app: AppHandle, project_id: String, chat_id: String) -> CmdResult<String> {
    let project = chat::project_for(&app, &project_id)?;
    tauri::async_runtime::spawn_blocking(move || app.state::<Arc<Chats>>().ship(&app, &project, &chat_id))
        .await
        .map_err(|e| e.to_string())?
}

/// Opens the panel in its own movable, resizable window, or focuses it.
///
/// NOTE: Async on purpose: building a window inside a synchronous command
/// deadlocks the event loop on Windows.
#[tauri::command]
pub async fn open_detached(app: AppHandle, tab: Option<String>, project_id: Option<String>) -> CmdResult<()> {
    crate::detached::open(&app, crate::detached::Nav { tab, project_id, run_id: None, task_id: None }).map_err(|e| e.to_string())
}

/// Maximize button and header double-click: fill the monitor or restore.
#[tauri::command]
pub fn toggle_fill(window: WebviewWindow) -> CmdResult<bool> {
    crate::detached::toggle_fill(&window).map_err(|e| e.to_string())
}

/// After a resize: converts a native maximize (Snap) into an exact fill.
#[tauri::command]
pub fn normalize_maximize(window: WebviewWindow) -> CmdResult<bool> {
    crate::detached::normalize_maximize(&window).map_err(|e| e.to_string())
}

/// Before dragging a filled window: shrink back under the cursor.
#[tauri::command]
pub fn restore_for_drag(window: WebviewWindow, ratio: f64) -> CmdResult<()> {
    crate::detached::restore_for_drag(&window, ratio).map_err(|e| e.to_string())
}

/// The command bar's AI: reads across projects and proposes actions.
#[tauri::command]
pub async fn ask_assistant(app: AppHandle, text: String, choice: Option<crate::models::RunChoice>) -> CmdResult<crate::assistant::Plan> {
    tauri::async_runtime::spawn_blocking(move || crate::assistant::ask(&app, &text, choice.unwrap_or_default()))
        .await
        .map_err(|e| e.to_string())?
}

/// Stops the assistant request in progress.
#[tauri::command]
pub fn stop_assistant() {
    crate::assistant::stop();
}

// -----------------------------------------------------------------------------
// Approvals
// -----------------------------------------------------------------------------

/// Tool uses waiting for the user, oldest first.
#[tauri::command]
pub fn list_approvals(approvals: State<'_, Arc<crate::approvals::Approvals>>) -> Vec<crate::approvals::Approval> {
    approvals.list()
}

/// Answers a pending tool approval.
#[tauri::command]
pub fn answer_approval(approvals: State<'_, Arc<crate::approvals::Approvals>>, id: String, answer: crate::approvals::Answer) -> CmdResult<()> {
    approvals.answer(&id, answer)
}

// -----------------------------------------------------------------------------
// GitHub
// -----------------------------------------------------------------------------

/// Issues, PR status and sync status of every GitHub project, by project id.
#[tauri::command]
pub fn github_snapshot(github: State<'_, Arc<Github>>) -> HashMap<String, ProjectGithub> {
    github.snapshot()
}

/// Asks for a GitHub sync: of one project now, or (with no project) of every
/// project that hasn't synced in the last minute, which is what opening the
/// panel does.
#[tauri::command]
pub fn github_refresh(github: State<'_, Arc<Github>>, project_id: Option<String>) {
    github.request(project_id);
}

/// Turns an assigned issue into a task in `status`, or returns the task
/// already linked to it.
fn issue_task(app: &AppHandle, project_id: &str, number: u64, status: TaskStatus) -> CmdResult<Task> {
    let state = app.state::<AppState>();
    let path = state.workspace.lock().unwrap().project(project_id).map(|p| p.path.clone()).ok_or_else(|| format!("Unknown project {project_id}"))?;
    let gh = app.state::<Arc<Github>>();
    let issue = match gh.cached_issue(project_id, number) {
        Some(issue) => issue,
        None => gh.issue(project_id, &path, number)?,
    };
    let tags = issue.labels.iter().map(|l| github::label_tag(&l.name)).filter(|t| !t.is_empty()).collect();
    let link = IssueLink { number: issue.number, url: issue.url.clone(), closed: false, extra: Default::default() };
    let (task, created) = state.workspace.lock().unwrap().task_for_issue(project_id, link, &issue.title, tags, status)?;
    if created {
        state.emit_snapshot(app);
    }
    Ok(task)
}

/// Adds an assigned GitHub issue to the board as a task.
#[tauri::command]
pub async fn track_issue(app: AppHandle, project_id: String, number: u64, status: Option<TaskStatus>) -> CmdResult<Task> {
    tauri::async_runtime::spawn_blocking(move || issue_task(&app, &project_id, number, status.unwrap_or(TaskStatus::Todo)))
        .await
        .map_err(|e| e.to_string())?
}

/// Turns an assigned GitHub issue into a task (or finds its task) and starts
/// an agent on it.
#[tauri::command]
pub async fn start_issue(
    app: AppHandle,
    project_id: String,
    number: u64,
    plan: Option<bool>,
    choice: Option<crate::models::RunChoice>,
) -> CmdResult<RunRecord> {
    tauri::async_runtime::spawn_blocking(move || {
        let task = issue_task(&app, &project_id, number, TaskStatus::Todo)?;
        app.state::<Arc<Agents>>().start(&app, &project_id, &task.id, plan, choice.unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Reports what this window shows (`None` when it is collapsed, hidden or
/// in the background), so notifications about it can be skipped.
#[tauri::command]
pub fn set_viewing(window: WebviewWindow, presence: State<'_, Arc<crate::notify::Presence>>, viewing: Option<crate::notify::Viewing>) {
    presence.set(window.label(), viewing);
}

// -----------------------------------------------------------------------------
// Plugins
// -----------------------------------------------------------------------------

/// Every plugin's current state.
#[tauri::command]
pub fn plugin_states(plugins: State<'_, Arc<Plugins>>) -> Vec<PluginState> {
    plugins.states()
}

/// Turns a plugin on or off.
///
/// SECURITY: turning an external plugin on approves one exact command line
/// and the access it asks for (its `approval`). The UI passes the approval
/// it showed; if the manifest changed since, the approval is refused so the
/// user never approves something they didn't see.
#[tauri::command]
pub fn set_plugin_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    plugins: State<'_, Arc<Plugins>>,
    id: String,
    enabled: bool,
    command: Option<String>,
) -> CmdResult<Vec<PluginState>> {
    let current = plugins.states().into_iter().find(|p| p.id == id).ok_or_else(|| format!("No plugin {id}"))?;
    let config = match current.source {
        Source::Builtin => state.update_config(&app, |c| {
            c.plugins.builtin.insert(id.clone(), enabled);
        })?,
        Source::External if enabled => {
            let shown = command.ok_or("Missing the command to approve")?;
            let now = current.approval.ok_or_else(|| format!("{} has a broken plugin.json", current.name))?;
            if shown != now {
                return Err("This plugin's command, folder or access just changed. Check it again, then turn it on.".into());
            }
            state.update_config(&app, |c| {
                c.plugins.external.insert(id.clone(), now);
            })?
        }
        Source::External => state.update_config(&app, |c| {
            c.plugins.external.remove(&id);
        })?,
    };
    plugins.sync(&config.plugins);
    Ok(plugins.states())
}

/// Saves the order of the external plugins, as arranged in Settings. Ids of
/// plugins that are gone drop out; plugins the list misses keep their place
/// after it.
#[tauri::command]
pub fn set_plugin_order(
    app: AppHandle,
    state: State<'_, AppState>,
    plugins: State<'_, Arc<Plugins>>,
    ids: Vec<String>,
) -> CmdResult<Vec<PluginState>> {
    let known: Vec<String> = plugins.states().into_iter().filter(|p| p.source == Source::External).map(|p| p.id).collect();
    let order = crate::plugins::normalize_order(&ids, &known);
    let config = state.update_config(&app, |c| c.plugins.order = order)?;
    plugins.sync(&config.plugins);
    Ok(plugins.states())
}

/// Puts a plugin's tab in the header, or keeps it in the header's overflow
/// menu only.
#[tauri::command]
pub fn set_plugin_pinned(
    app: AppHandle,
    state: State<'_, AppState>,
    plugins: State<'_, Arc<Plugins>>,
    id: String,
    pinned: bool,
) -> CmdResult<Vec<PluginState>> {
    let known: Vec<String> = plugins.states().into_iter().map(|p| p.id).collect();
    if !known.contains(&id) {
        return Err(format!("No plugin {id}"));
    }
    let config = state.update_config(&app, |c| {
        c.plugins.menu_tabs.retain(|t| known.contains(t));
        if pinned {
            c.plugins.menu_tabs.remove(&id);
        } else {
            c.plugins.menu_tabs.insert(id.clone());
        }
    })?;
    plugins.sync(&config.plugins);
    Ok(plugins.states())
}

/// Re-reads the plugins folder.
#[tauri::command]
pub fn rescan_plugins(state: State<'_, AppState>, plugins: State<'_, Arc<Plugins>>) -> Vec<PluginState> {
    let settings = state.config.read().unwrap().plugins.clone();
    plugins.rescan(&settings);
    plugins.states()
}

/// Sends a widget button or status fix to a plugin.
#[tauri::command]
pub fn plugin_action(plugins: State<'_, Arc<Plugins>>, id: String, action: String) -> CmdResult<()> {
    plugins.action(&id, &action)
}

/// Stops and starts a plugin after an error.
#[tauri::command]
pub fn restart_plugin(plugins: State<'_, Arc<Plugins>>, id: String) -> CmdResult<()> {
    plugins.restart(&id)
}

/// Now playing's timeline, modes and session list; `None` when nothing plays.
#[tauri::command]
pub fn media_state() -> Option<crate::plugins::media::NowPlaying> {
    crate::plugins::media::latest()
}

/// The external plugins folder (created if missing).
#[tauri::command]
pub fn plugins_folder(plugins: State<'_, Arc<Plugins>>) -> CmdResult<PathBuf> {
    std::fs::create_dir_all(plugins.dir()).map_err(|e| e.to_string())?;
    Ok(plugins.dir().to_path_buf())
}

/// Opens the external plugins folder in Explorer, creating it if needed.
#[tauri::command]
pub fn open_plugins_folder(app: AppHandle, plugins: State<'_, Arc<Plugins>>) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = plugins_folder(plugins)?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| format!("Couldn't open the plugins folder: {e}"))
}

/// Dismisses one feed item for good.
#[tauri::command]
pub fn dismiss_feed_item(plugins: State<'_, Arc<Plugins>>, plugin_id: String, item_id: String) -> CmdResult<()> {
    plugins.dismiss(&plugin_id, vec![item_id])
}

/// Dismisses every feed item of one plugin, or of all plugins without one.
#[tauri::command]
pub fn clear_feed(plugins: State<'_, Arc<Plugins>>, plugin_id: Option<String>) -> CmdResult<()> {
    plugins.dismiss_all(plugin_id.as_deref())
}

/// Sends a feed item's plugin-defined button to its plugin.
#[tauri::command]
pub fn feed_item_action(plugins: State<'_, Arc<Plugins>>, plugin_id: String, item_id: String, action: String) -> CmdResult<()> {
    plugins.item_action(&plugin_id, &item_id, &action)
}

/// The task "Fix with an agent" created or found, from a feed item or a
/// page button.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedTaskResult {
    project_id: String,
    task: Task,
    /// False when an open task with the same title already existed.
    created: bool,
}

/// Turns a feed item's suggested task into a task in the item's project, or
/// finds the open task already made from it. The plugin's notes are fenced
/// as untrusted text (see [`prompt::plugin_task_notes`]).
#[tauri::command]
pub fn feed_task(
    app: AppHandle,
    state: State<'_, AppState>,
    plugins: State<'_, Arc<Plugins>>,
    plugin_id: String,
    item_id: String,
) -> CmdResult<FeedTaskResult> {
    let (plugin_name, item) = plugins.feed_item(&plugin_id, &item_id)?;
    let suggested = item.task.as_ref().ok_or("This item doesn't suggest a task")?;
    let project_id = item.project_id.clone().ok_or("This item isn't about a project")?;
    let suggestion = Suggestion {
        plugin: &plugin_name,
        title: &suggested.title,
        notes: &suggested.notes,
        url: item.url.as_deref(),
        urgent: matches!(item.severity, Severity::Critical | Severity::High),
    };
    add_suggested_task(&app, &state, project_id, suggestion)
}

/// A task a plugin suggested, before Pill writes it.
struct Suggestion<'a> {
    plugin: &'a str,
    title: &'a str,
    notes: &'a str,
    url: Option<&'a str>,
    /// Critical or high: the task gets priority 1.
    urgent: bool,
}

/// Adds a plugin's suggested task to a project, or finds the open task with
/// the same title made before. The notes are fenced as untrusted text.
fn add_suggested_task(app: &AppHandle, state: &AppState, project_id: String, s: Suggestion) -> CmdResult<FeedTaskResult> {
    let (task, created) = {
        let mut ws = state.workspace.lock().unwrap();
        let project = ws.project(&project_id).ok_or("That project is no longer in Pill")?;
        let existing = project.tasks.iter().find(|t| t.status != TaskStatus::Done && t.title.trim() == s.title).cloned();
        match existing {
            Some(task) => (task, false),
            None => {
                let draft = NewTask {
                    title: s.title.to_string(),
                    notes: prompt::plugin_task_notes(s.plugin, s.url, s.notes),
                    status: Some(TaskStatus::Todo),
                    priority: s.urgent.then_some(1),
                    origin: crate::model::TaskOrigin::Plugin,
                    ..Default::default()
                };
                (ws.create_task(Some(&project_id), draft)?, true)
            }
        }
    };
    if created {
        state.emit_snapshot(app);
    }
    Ok(FeedTaskResult { project_id, task, created })
}

/// Notes that this window shows a plugin's tab page and returns the cached
/// page. The plugin is told (`view`) when no window showed it already.
#[tauri::command]
pub fn open_plugin_view(
    window: WebviewWindow,
    plugins: State<'_, Arc<Plugins>>,
    plugin_id: String,
    project_id: Option<String>,
) -> CmdResult<Option<PageState>> {
    plugins.open_view(&plugin_id, window.label(), project_id)
}

/// Notes that this window stopped showing a plugin's tab.
#[tauri::command]
pub fn close_plugin_view(window: WebviewWindow, plugins: State<'_, Arc<Plugins>>, plugin_id: String) {
    plugins.close_view(&plugin_id, window.label());
}

/// Sends a page button to its plugin.
#[tauri::command]
pub fn plugin_page_action(
    plugins: State<'_, Arc<Plugins>>,
    plugin_id: String,
    project_id: Option<String>,
    item_id: Option<String>,
    action: String,
) -> CmdResult<()> {
    plugins.page_action(&plugin_id, project_id, item_id.as_deref(), &action)
}

/// Adds the task a page button suggests to the page's project, or finds the
/// open task made from it before. The row's link and severity go with it.
#[tauri::command]
pub fn plugin_page_task(
    app: AppHandle,
    state: State<'_, AppState>,
    plugins: State<'_, Arc<Plugins>>,
    plugin_id: String,
    project_id: String,
    item_id: Option<String>,
    action: String,
) -> CmdResult<FeedTaskResult> {
    let (plugin_name, button, row) = plugins.page_button(&plugin_id, Some(project_id.clone()), item_id.as_deref(), &action)?;
    let ButtonKind::Task { task } = &button.kind else {
        return Err("That button doesn't suggest a task".into());
    };
    let suggestion = Suggestion {
        plugin: &plugin_name,
        title: &task.title,
        notes: &task.notes,
        url: row.as_ref().and_then(|r| r.url.as_deref()),
        urgent: row.as_ref().and_then(|r| r.severity).is_some_and(|s| s >= Severity::High),
    };
    add_suggested_task(&app, &state, project_id, suggestion)
}

/// Opens a Windows Settings page from a fixed list.
#[tauri::command]
pub fn open_system_settings(app: AppHandle, page: SystemPage) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(page.uri(), None::<&str>).map_err(|e| format!("Couldn't open Windows Settings: {e}"))
}

// -----------------------------------------------------------------------------
// Windows notifications
// -----------------------------------------------------------------------------

/// The notifications the built-in plugin lists, or `None` while it's off or
/// can't read Action Center. Changes arrive as `pill://notifications`.
#[tauri::command]
pub fn notifications_feed() -> Option<crate::plugins::notifications::Feed> {
    crate::plugins::notifications::feed()
}

/// Dismisses one notification, in Action Center too.
#[tauri::command]
pub async fn dismiss_notification(id: u32) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || crate::plugins::notifications::dismiss(id))
        .await
        .map_err(|e| e.to_string())?
}

/// Clears every notification, in Action Center too.
#[tauri::command]
pub async fn clear_notifications() -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(crate::plugins::notifications::clear_all).await.map_err(|e| e.to_string())?
}

/// Opens the screenshot a Snipping Tool notification announced.
///
/// SECURITY: the UI names a notification, never a path, so only a file Pill
/// matched itself can be opened.
#[tauri::command]
pub fn open_notification_screenshot(app: AppHandle, id: u32) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let path = crate::plugins::notifications::screenshot_path(id).ok_or("That screenshot isn't there anymore")?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| format!("Couldn't open the screenshot: {e}"))
}

// -----------------------------------------------------------------------------
// Voice
// -----------------------------------------------------------------------------

/// Listens for one utterance and returns the transcript. Progress arrives as
/// `pill://voice` events.
#[tauri::command]
pub async fn voice_listen(app: AppHandle) -> Result<String, VoiceError> {
    let voice = app.state::<Arc<Voice>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        voice.listen(|update| {
            let _ = app.emit(crate::voice::VOICE_EVENT, update);
        })
    })
    .await
    .map_err(|e| VoiceError { code: crate::voice::VoiceErrorCode::Failed, message: e.to_string(), fix: None })?
}

/// Ends dictation; `commit` keeps what was heard so far.
#[tauri::command]
pub fn voice_stop(voice: State<'_, Arc<Voice>>, commit: bool) {
    voice.stop(commit);
}

// -----------------------------------------------------------------------------
// Updates
// -----------------------------------------------------------------------------

/// Whether this build has a real update endpoint. Source builds keep the
/// `OWNER/REPO` placeholder from `tauri.conf.json` until a maintainer (or the
/// release workflow) sets it, and checking it would only produce a 404.
#[tauri::command]
pub fn updates_configured(app: AppHandle) -> bool {
    let endpoints = app.config().plugins.0.get("updater").and_then(|u| u.get("endpoints")).cloned();
    endpoints_configured(endpoints.as_ref())
}

/// Pure half of [`updates_configured`]: at least one endpoint, none of them
/// the placeholder.
pub(crate) fn endpoints_configured(endpoints: Option<&serde_json::Value>) -> bool {
    let Some(list) = endpoints.and_then(|v| v.as_array()) else { return false };
    !list.is_empty() && list.iter().all(|e| e.as_str().is_some_and(|s| s.starts_with("https://") && !s.contains("OWNER/REPO")))
}

#[cfg(test)]
mod tests {
    use super::endpoints_configured;
    use serde_json::json;

    #[test]
    fn placeholder_update_endpoints_count_as_unconfigured() {
        assert!(!endpoints_configured(None));
        assert!(!endpoints_configured(Some(&json!([]))));
        assert!(!endpoints_configured(Some(&json!(["https://github.com/OWNER/REPO/releases/latest/download/latest.json"]))));
        assert!(!endpoints_configured(Some(&json!(["http://example.com/latest.json"]))));
        assert!(endpoints_configured(Some(&json!(["https://github.com/acme/pill/releases/latest/download/latest.json"]))));
    }
}
