//! Global quick-add hotkey.

use crate::overlay::OverlayState;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Event telling the UI to open quick-add with keyboard focus.
pub const QUICK_ADD_EVENT: &str = "pill://quick-add";

/// Builds the plugin with a handler that opens quick-add on key down.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                log::debug!("quick-add hotkey pressed");
                open_quick_add(app);
            }
        })
        .build()
}

/// Takes keyboard focus and tells the UI to show the quick-add input.
pub fn open_quick_add(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(crate::PILL_WINDOW) {
        #[cfg(windows)]
        crate::overlay::take_focus(&window, &app.state::<Arc<OverlayState>>());
        #[cfg(not(windows))]
        let _ = window.set_focus();
    }
    let _ = app.emit(QUICK_ADD_EVENT, ());
}

/// Registers `accelerator`, unregistering `previous` only once the new one
/// is known to be valid and free, so a bad choice never leaves no hotkey.
pub fn register(app: &AppHandle, accelerator: &str, previous: Option<&str>) -> Result<(), String> {
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|e| format!("\"{accelerator}\" isn't a valid shortcut: {e}"))?;
    let gs = app.global_shortcut();
    if let Some(prev) = previous.filter(|p| *p != accelerator) {
        if let Ok(prev) = prev.parse::<Shortcut>() {
            let _ = gs.unregister(prev);
        }
    }
    if gs.is_registered(shortcut) {
        return Ok(());
    }
    gs.register(shortcut).inspect(|()| log::info!("registered hotkey {accelerator}")).map_err(|e| {
        log::warn!("hotkey {accelerator} not registered: {e}");
        // Put the old one back so the user isn't left without a hotkey.
        if let Some(Ok(prev)) = previous.map(str::parse::<Shortcut>) {
            let _ = gs.register(prev);
        }
        format!("Couldn't register {accelerator} (another app may be using it): {e}")
    })
}
