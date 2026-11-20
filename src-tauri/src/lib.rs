//! Pill's Rust core: windows, files, git and processes.
//!
//! The React UI only renders what the core sends; files on disk are the
//! source of truth and the core watches them so outside edits appear live.

mod agent;
mod appearance;
mod approvals;
mod assistant;
mod backdrop;
mod chat;
mod checks;
mod commands;
mod config;
mod detached;
mod editor;
mod github;
mod glass_image;
mod gitinfo;
mod hotkey;
mod limits;
mod model;
mod notify;
mod models;
mod overlay;
mod placement;
mod plugins;
mod prompt;
mod provider;
mod process;
mod scan;
mod stream;
mod toast;
mod state;
mod system_settings;
mod voice;
mod watch;
mod workspace;
mod worktree;

use overlay::OverlayState;
use state::AppState;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use workspace::Workspace;

/// Label of the single pill window in `tauri.conf.json`.
pub(crate) const PILL_WINDOW: &str = "pill";

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let add = MenuItem::with_id(app, "quick-add", "Add task…", true, None::<&str>)?;
    let rescan = MenuItem::with_id(app, "rescan", "Rescan projects", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Pill", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&add, &rescan, &sep, &quit])?;
    let mut tray = TrayIconBuilder::with_id("pill-tray")
        .tooltip("Pill")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quick-add" => hotkey::open_quick_add(app),
            "rescan" => app.state::<AppState>().rescan(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Whether `url` is the app's own page: the bundled frontend, or the Vite
/// dev server when `dev_url` is given (debug builds only).
fn is_app_url(url: &tauri::Url, dev_url: Option<&tauri::Url>) -> bool {
    let host = url.host_str();
    match url.scheme() {
        "tauri" => host == Some("localhost"),
        "http" | "https" if host == Some("tauri.localhost") => true,
        _ => dev_url.is_some_and(|dev| dev.origin() == url.origin()),
    }
}

/// Keeps every webview on the app's own origin.
///
/// SECURITY: The webviews can invoke the core, so a page they were steered
/// to (a stray link in rendered markdown, a redirect) must never load in
/// their place. Links open in the browser through the opener plugin;
/// anything else is refused here.
fn navigation_guard<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("navigation-guard")
        .on_navigation(|webview, url| {
            let dev_url = if cfg!(debug_assertions) { webview.config().build.dev_url.clone() } else { None };
            let allowed = is_app_url(url, dev_url.as_ref());
            if !allowed {
                log::warn!("blocked navigation to {url}");
            }
            allowed
        })
        .build()
}

/// Builds and runs the Tauri application.
pub fn run() {
    tauri::Builder::default()
        // NOTE: Registered first so a second launch exits before creating
        // another always-on-top window; the running one opens quick-add, or
        // the run or task a clicked notification links to.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            match argv.iter().find_map(|a| notify::parse_link(a)) {
                Some(target) => notify::open_target(app, target),
                None => hotkey::open_quick_add(app),
            }
        }))
        .plugin(navigation_guard())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info })
                // Third-party crates are chatty at debug level.
                .level_for("tao", log::LevelFilter::Warn)
                .level_for("notify", log::LevelFilter::Warn)
                .max_file_size(2_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .build(),
        )
        .plugin(hotkey::plugin())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(
            // The overlay places itself every time; only the detached window's
            // bounds are remembered. Visibility is left out so the window
            // never reappears on its own at startup.
            tauri_plugin_window_state::Builder::default()
                .with_denylist(&["pill"])
                .with_state_flags(tauri_plugin_window_state::StateFlags::all() - tauri_plugin_window_state::StateFlags::VISIBLE)
                .build(),
        )
        .setup(|app| {
            toast::register_app_id(app.handle());
            let pill_dir = app.path().config_dir()?.join("Pill");
            let config_path = pill_dir.join("config.json");
            let loaded = config::load(&config_path);
            if let Some(err) = &loaded.error {
                log::warn!("config: {err}");
            }
            let cfg = loaded.config.clone();

            let overlay_state = Arc::new(OverlayState::default());
            *overlay_state.monitor.lock().unwrap() = cfg.monitor;
            overlay_state.on_top.store(cfg.on_top, std::sync::atomic::Ordering::Relaxed);
            app.manage(overlay_state.clone());

            let mut ws = Workspace::new(pill_dir.join("inbox.json"));
            ws.rescan(&cfg.roots, &cfg.projects);
            app.manage(AppState {
                config_path,
                config: RwLock::new(cfg.clone()),
                config_error: RwLock::new(loaded.error),
                workspace: Mutex::new(ws),
                watch: OnceLock::new(),
                hotkey_error: RwLock::new(None),
            });

            let agents = Arc::new(agent::Agents::default());
            let interrupted = {
                let ws = app.state::<AppState>();
                let ws = ws.workspace.lock().unwrap();
                let paths: Vec<_> = ws.project_paths().map(|(id, p)| (id.to_string(), p.to_path_buf())).collect();
                agents.load(&paths)
            };
            app.manage(agents.clone());
            app.manage(Arc::new(chat::Chats::default()));
            app.manage(Arc::new(approvals::Approvals::default()));
            app.manage(Arc::new(github::Github::default()));
            app.manage(Arc::new(notify::Presence::default()));
            agents.settle_interrupted(app.handle(), &interrupted);
            agent::Agents::spawn_autopilot(app.handle().clone());
            github::Github::spawn(app.handle().clone());
            #[cfg(windows)]
            if let Err(e) = toast::register_protocol() {
                log::warn!("notification clicks won't open Pill: {e}");
            }

            let plugins = plugins::Plugins::new(app.handle().clone(), pill_dir.join("plugins"));
            plugins.sync(&cfg.plugins);
            app.manage(plugins);
            app.manage(Arc::new(voice::Voice::default()));

            let state = app.state::<AppState>();
            match watch::start(app.handle().clone()) {
                Ok(w) => {
                    let _ = state.watch.set(w);
                    state.sync_watches();
                }
                Err(e) => log::error!("file watching unavailable: {e}"),
            }
            if let Err(e) = hotkey::register(app.handle(), &cfg.hotkey, None) {
                *state.hotkey_error.write().unwrap() = Some(e);
            }

            let window = app
                .get_webview_window(PILL_WINDOW)
                .ok_or("pill window missing from tauri.conf.json")?;
            placement::place(&window, cfg.monitor)?;

            #[cfg(windows)]
            {
                overlay::spawn(window.clone(), overlay_state);
                detached::spawn_backdrop_watch(window.clone());
                appearance::watch(app.handle().clone());
                overlay::show_without_focus(&window);
            }
            #[cfg(not(windows))]
            window.show()?;

            build_tray(app.handle())?;
            // A notification clicked after Pill quit starts it with the link.
            if let Some(target) = std::env::args().skip(1).find_map(|a| notify::parse_link(&a)) {
                notify::open_target(app.handle(), target);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                window.state::<Arc<notify::Presence>>().set(window.label(), None);
                if let Some(plugins) = window.try_state::<Arc<plugins::Plugins>>() {
                    plugins.close_window(window.label());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::open_detached,
            commands::model_catalog,
            commands::sample_backdrop,
            commands::stock_wallpapers,
            commands::image_thumbnail,
            commands::toggle_fill,
            commands::compare_agents,
            commands::ask_assistant,
            commands::stop_assistant,
            commands::pick_run,
            commands::normalize_maximize,
            commands::restore_for_drag,
            commands::get_backdrop,
            commands::get_snapshot,
            commands::create_task,
            commands::update_task,
            commands::move_task,
            commands::delete_task,
            commands::save_project_settings,
            commands::save_notes,
            commands::open_in_editor,
            commands::open_project_folder,
            commands::detect_editors,
            commands::render_task_prompt,
            commands::set_roots,
            commands::add_project,
            commands::remove_project,
            commands::rescan,
            commands::list_drives,
            commands::set_hotkey,
            commands::set_hit_rect,
            commands::focus_window,
            commands::release_focus,
            commands::list_runs,
            commands::run_log,
            commands::run_prompt,
            commands::start_agent,
            commands::stop_agent,
            commands::retry_agent,
            commands::approve_plan,
            commands::cleanup_run,
            commands::delete_run,
            commands::clear_failed_runs,
            commands::preflight,
            commands::detect_checks,
            commands::project_git_info,
            commands::open_worktree,
            commands::update_settings,
            commands::list_monitors,
            commands::cli_status,
            commands::list_chats,
            commands::create_chat,
            commands::send_chat,
            commands::set_chat_mode,
            commands::list_approvals,
            commands::answer_approval,
            commands::set_viewing,
            commands::stop_chat,
            commands::delete_chat,
            commands::ship_chat,
            commands::github_snapshot,
            commands::github_refresh,
            commands::track_issue,
            commands::start_issue,
            commands::plugin_states,
            commands::set_plugin_enabled,
            commands::set_plugin_order,
            commands::set_plugin_pinned,
            commands::rescan_plugins,
            commands::plugin_action,
            commands::restart_plugin,
            commands::media_state,
            commands::plugins_folder,
            commands::open_plugins_folder,
            commands::dismiss_feed_item,
            commands::clear_feed,
            commands::feed_item_action,
            commands::feed_task,
            commands::open_plugin_view,
            commands::close_plugin_view,
            commands::plugin_page_action,
            commands::plugin_page_task,
            commands::open_system_settings,
            commands::notifications_feed,
            commands::dismiss_notification,
            commands::clear_notifications,
            commands::open_notification_screenshot,
            commands::voice_listen,
            commands::voice_stop,
            commands::updates_configured,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Pill")
        .run(|app, event| {
            // Job objects end plugin processes when Pill dies anyway; this
            // also stops the built-in workers cleanly on a normal quit.
            if let tauri::RunEvent::Exit = event {
                if let Some(p) = app.try_state::<Arc<plugins::Plugins>>() {
                    p.shutdown();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::is_app_url;
    use tauri::Url;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn app_origins_are_allowed() {
        assert!(is_app_url(&url("tauri://localhost/index.html"), None));
        assert!(is_app_url(&url("http://tauri.localhost/index.html?w=detached"), None));
        assert!(is_app_url(&url("https://tauri.localhost/"), None));
    }

    #[test]
    fn everything_else_is_refused() {
        assert!(!is_app_url(&url("https://example.com/"), None));
        assert!(!is_app_url(&url("http://tauri.localhost.evil.com/"), None));
        assert!(!is_app_url(&url("tauri://evil/"), None));
        assert!(!is_app_url(&url("file:///C:/Windows/win.ini"), None));
        assert!(!is_app_url(&url("http://localhost:1420/"), None));
        assert!(!is_app_url(&url("javascript:alert(1)"), None));
    }

    #[test]
    fn dev_server_is_allowed_only_when_given() {
        let dev = url("http://localhost:1420");
        assert!(is_app_url(&url("http://localhost:1420/src/main.tsx"), Some(&dev)));
        assert!(!is_app_url(&url("http://localhost:1421/"), Some(&dev)));
        assert!(!is_app_url(&url("https://example.com/"), Some(&dev)));
    }
}
