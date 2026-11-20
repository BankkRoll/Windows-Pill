//! The detached window and the theme surfaces it shares with the notch.
//!
//! The detached window is the expanded panel in a normal, resizable window
//! that can move to any display, maximize or pin on top. It activates like
//! any other app window, so none of the overlay's focus or click-through
//! handling applies. Its bounds persist via `tauri-plugin-window-state`.
//!
//! Glass on both surfaces is the same pipeline (see [`crate::backdrop`]): the
//! wallpaper behind the window, sampled at one density, blurred and tinted
//! identically, so the notch and the window always match. A background
//! thread keeps the notch's sample fresh; the window asks for its own
//! sample whenever it moves or resizes.

use crate::backdrop::{self, Backdrop};
use crate::config::{GlassSource, Theme};
use crate::state::AppState;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Window label, also used by the capability file and the window-state denylist.
pub const LABEL: &str = "detached";

/// Event that tells an already-open detached window which view to show.
pub const NAV_EVENT: &str = "pill://detached-nav";

/// Event carrying a fresh [`Backdrop`] for the notch.
pub const BACKDROP_EVENT: &str = "pill://backdrop";

static LATEST: Mutex<Option<Backdrop>> = Mutex::new(None);

/// The most recent wallpaper sample behind the notch.
pub fn latest_backdrop() -> Option<Backdrop> {
    LATEST.lock().unwrap().clone()
}

/// The view the detached window opens on.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Nav {
    pub tab: Option<String>,
    pub project_id: Option<String>,
    /// Run to select in the Agents tab, e.g. from a notification.
    pub run_id: Option<String>,
    /// Task whose page to open when there is no run to show.
    pub task_id: Option<String>,
}

/// Samples the glass backdrop for the whole monitor `window` is on: the
/// picked image when the glass source is one, otherwise the wallpaper.
pub fn wallpaper_for<R: Runtime>(window: &WebviewWindow<R>) -> Option<Backdrop> {
    let monitor = window.current_monitor().ok()??;
    let (mp, ms) = (monitor.position(), monitor.size());
    let rect = (mp.x, mp.y, ms.width, ms.height);
    let image = {
        let state = window.app_handle().state::<AppState>();
        let config = state.config.read().unwrap();
        (config.glass_source == GlassSource::Image).then(|| config.glass_image.clone()).flatten()
    };
    let mut sample = match image {
        Some(path) => crate::glass_image::backdrop(&path, rect)?,
        None => backdrop::sample(rect, rect)?,
    };
    sample.scale = monitor.scale_factor();
    Some(sample)
}

/// Opens the detached window on `nav`, or focuses it and switches view if
/// it is already open.
pub fn open(app: &AppHandle, nav: Nav) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        window.unminimize()?;
        window.show()?;
        window.set_focus()?;
        window.emit(NAV_EVENT, nav)?;
        return Ok(());
    }

    // The UI reads this before React mounts, so the first frame already
    // shows the right tab instead of flashing Today.
    let init = format!(
        "window.__PILL_DETACHED__ = {};",
        serde_json::to_string(&nav).unwrap_or_else(|_| "{}".into())
    );
    // A new window starts unfilled, whatever the last one was.
    *RESTORE.lock().unwrap() = None;
    let (theme, light) = {
        let state = app.state::<AppState>();
        let config = state.config.read().unwrap();
        (config.theme, crate::appearance::is_light(config.appearance))
    };
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Pill")
        .initialization_script(&init)
        .inner_size(960.0, 680.0)
        .min_inner_size(560.0, 420.0)
        .center()
        .decorations(false)
        .shadow(true)
        .resizable(true)
        .theme(Some(if light { tauri::Theme::Light } else { tauri::Theme::Dark }))
        .background_color(crate::appearance::window_fill(theme, light))
        .visible(false)
        .build()?;
    // NOTE: Shown after build so window-state has already restored the saved
    // bounds; showing in the builder would flash the window at the default spot.
    window.show()?;
    window.set_focus()?;
    Ok(())
}

static REFRESH: AtomicBool = AtomicBool::new(false);

/// Makes the backdrop watch re-sample now instead of at its next tick, so a
/// new glass source reaches the notch right away.
pub fn refresh_backdrop() {
    REFRESH.store(true, Ordering::Relaxed);
}

/// Samples the overlay monitor's wallpaper now and every few seconds,
/// emitting [`BACKDROP_EVENT`] whenever it changes (new wallpaper, slideshow,
/// moved to another monitor). Only runs the sampling while glass is on.
pub fn spawn_backdrop_watch<R: Runtime>(overlay: WebviewWindow<R>) {
    let spawned = std::thread::Builder::new().name("backdrop".into()).spawn(move || loop {
        let glass_theme = overlay.app_handle().state::<AppState>().config.read().unwrap().theme == Theme::Glass;
        if glass_theme {
            {
                let fresh = wallpaper_for(&overlay);
                let changed = {
                    let mut latest = LATEST.lock().unwrap();
                    let changed = *latest != fresh;
                    latest.clone_from(&fresh);
                    changed
                };
                if changed {
                    let _ = overlay.emit(BACKDROP_EVENT, fresh);
                }
            }
        }
        // PERF: painting the wallpaper takes a few ms; slideshows change at
        // most every minute, so a 10 s cadence is fresh without any load.
        // Short naps let a settings change through without that wait.
        for _ in 0..40 {
            if REFRESH.swap(false, Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
    if let Err(e) = spawned {
        log::warn!("backdrop watcher didn't start: {e}");
    }
}

// -----------------------------------------------------------------------------
// Maximize
// -----------------------------------------------------------------------------
//
// Windows maximizes a frameless window past the screen edges by the width of
// its invisible resize border (9 px at 125 %), and the webview doesn't
// compensate, which leaves a strip on the left and top. Pill therefore never
// stays natively maximized: "maximized" means the window exactly fills the
// monitor's work area, and the bounds to return to are remembered here.

/// Outer position and inner size to restore to while the window fills its
/// monitor. Inner size because `set_size` takes inner size; saving the outer
/// one would grow the window by its frame on every round trip.
static RESTORE: Mutex<Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)>> = Mutex::new(None);

/// True while the detached window fills its monitor's work area.
pub fn is_filled() -> bool {
    RESTORE.lock().unwrap().is_some()
}

fn fill<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else { return Ok(()) };
    let area = *monitor.work_area();
    {
        let mut restore = RESTORE.lock().unwrap();
        if restore.is_none() {
            *restore = Some((window.outer_position()?, window.inner_size()?));
        }
    }
    // NOTE: The frame keeps invisible resize borders (8 px left, right and
    // bottom at 125 %) outside the client area, and `set_position` places the
    // outer frame. Placing the client area on the work area means offsetting
    // the frame by the client's inset, measured after sizing.
    window.set_size(area.size)?;
    let (outer, inner) = (window.outer_position()?, window.inner_position()?);
    window.set_position(tauri::PhysicalPosition::new(
        area.position.x - (inner.x - outer.x),
        area.position.y - (inner.y - outer.y),
    ))?;
    Ok(())
}

fn restore<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    let saved = RESTORE.lock().unwrap().take();
    if let Some((pos, size)) = saved {
        window.set_size(size)?;
        window.set_position(pos)?;
    }
    Ok(())
}

/// Toggles between filling the monitor and the previous bounds.
/// Returns whether the window now fills the monitor.
pub fn toggle_fill<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<bool> {
    if window.is_maximized()? {
        window.unmaximize()?;
    }
    if is_filled() {
        restore(window)?;
        Ok(false)
    } else {
        fill(window)?;
        Ok(true)
    }
}

/// Turns a native maximize (double-click on the header, or dragging to the
/// top edge with Snap) into an exact fill; a second one restores.
/// Returns whether the window now fills the monitor.
pub fn normalize_maximize<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<bool> {
    if !window.is_maximized()? {
        return Ok(is_filled());
    }
    toggle_fill(window)
}

/// Before a drag starts on a filled window: return to the saved size, placed
/// so the cursor keeps its relative spot on the header, like native windows.
/// `ratio` is the cursor's x position across the header, 0 to 1.
pub fn restore_for_drag<R: Runtime>(window: &WebviewWindow<R>, ratio: f64) -> tauri::Result<()> {
    let saved = RESTORE.lock().unwrap().take();
    let Some((_, size)) = saved else { return Ok(()) };
    let cursor = window.cursor_position()?;
    let x = cursor.x - ratio.clamp(0.0, 1.0) * f64::from(size.width);
    let y = cursor.y - 20.0 * window.scale_factor()?;
    window.set_size(size)?;
    window.set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
    Ok(())
}
