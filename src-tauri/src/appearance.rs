//! Light and dark appearance.
//!
//! Responsibilities:
//! - Resolve [`Appearance::System`] from the Windows app mode
//!   (`AppsUseLightTheme`) and tell the UI when the user flips it.
//! - Keep the detached window's native theme and background in step with the
//!   palette, so a new or resized window never flashes the other colour
//!   before the page paints.
//!
//! The UI does the actual painting; this module only knows the window fill
//! of each surface, which must match `--window-fill` in `src/index.css`.

use crate::config::{Appearance, Theme};
use crate::state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};

/// Event carrying `true` when Windows switches apps to light mode, `false`
/// for dark.
pub const SYSTEM_APPEARANCE_EVENT: &str = "pill://system-appearance";

static SYSTEM_LIGHT: AtomicBool = AtomicBool::new(false);

/// Whether Windows currently asks apps for light mode, as last read.
pub fn system_light() -> bool {
    SYSTEM_LIGHT.load(Ordering::Relaxed)
}

/// Whether `appearance` paints light right now.
///
/// @param appearance The configured appearance.
/// @returns `true` for light surfaces with dark text.
pub fn is_light(appearance: Appearance) -> bool {
    match appearance {
        Appearance::Dark => false,
        Appearance::Light => true,
        Appearance::System => system_light(),
    }
}

/// The opaque colour a window shows before its page paints.
///
/// @param theme Surface style.
/// @param light Whether the light palette is in use.
/// @returns The native background, matching `--window-fill` for light.
pub fn window_fill(theme: Theme, light: bool) -> Color {
    if !light {
        return Color(0, 0, 0, 255);
    }
    match theme {
        Theme::Black => Color(0xff, 0xff, 0xff, 255),
        Theme::Graphite => Color(0xf2, 0xf2, 0xf4, 255),
        Theme::Glass => Color(0xec, 0xec, 0xef, 255),
    }
}

/// Applies the palette to a window's native chrome and background.
///
/// @param window The detached window.
/// @param theme Surface style.
/// @param light Whether the light palette is in use.
pub fn style_window<R: Runtime>(window: &WebviewWindow<R>, theme: Theme, light: bool) {
    let native = if light { tauri::Theme::Light } else { tauri::Theme::Dark };
    if let Err(e) = window.set_theme(Some(native)) {
        log::debug!("window theme: {e}");
    }
    if let Err(e) = window.set_background_color(Some(window_fill(theme, light))) {
        log::debug!("window background: {e}");
    }
}

/// Restyles the open detached window, if any, for the current settings.
pub fn sync<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(crate::detached::LABEL) else { return };
    let (theme, appearance) = {
        let state = app.state::<AppState>();
        let config = state.config.read().unwrap();
        (config.theme, config.appearance)
    };
    style_window(&window, theme, is_light(appearance));
}

/// Reads the Windows app mode now, then follows changes on a background
/// thread, emitting [`SYSTEM_APPEARANCE_EVENT`] whenever it flips.
pub fn watch<R: Runtime>(app: AppHandle<R>) {
    SYSTEM_LIGHT.store(platform::read_light(), Ordering::Relaxed);
    platform::spawn_watch(move |light| {
        if SYSTEM_LIGHT.swap(light, Ordering::Relaxed) == light {
            return;
        }
        let _ = app.emit(SYSTEM_APPEARANCE_EVENT, light);
        sync(&app);
    });
}

#[cfg(windows)]
mod platform {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY,
        KEY_QUERY_VALUE, REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_DWORD,
    };

    const KEY: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");

    /// `AppsUseLightTheme`; Windows itself treats a missing value as light.
    pub fn read_light() -> bool {
        let mut value = 1u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: `value` is a u32 and `size` says so; RegGetValueW writes at most that.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                KEY,
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut value as *mut u32).cast()),
                Some(&mut size),
            )
        };
        status.is_err() || value != 0
    }

    pub fn spawn_watch(on_change: impl Fn(bool) + Send + 'static) {
        let spawned = std::thread::Builder::new().name("appearance".into()).spawn(move || {
            let mut key = HKEY::default();
            // SAFETY: `key` receives the opened handle, closed below.
            let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, KEY, None, KEY_NOTIFY | KEY_QUERY_VALUE, &mut key) };
            if opened.is_err() {
                log::warn!("can't follow the Windows app mode: {opened:?}");
                return;
            }
            loop {
                // NOTE: Synchronous: blocks this thread until any value under
                // the key is written. The key also holds transparency and
                // accent settings, so the caller drops non-changes.
                // SAFETY: `key` stays open for the life of the loop.
                let status = unsafe { RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, None, false) };
                if status.is_err() {
                    log::warn!("stopped following the Windows app mode: {status:?}");
                    break;
                }
                on_change(read_light());
            }
            // SAFETY: opened above and no longer used.
            let _ = unsafe { RegCloseKey(key) };
        });
        if let Err(e) = spawned {
            log::warn!("appearance watcher didn't start: {e}");
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn read_light() -> bool {
        false
    }

    pub fn spawn_watch(_on_change: impl Fn(bool) + Send + 'static) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_appearances_ignore_the_system() {
        assert!(is_light(Appearance::Light));
        assert!(!is_light(Appearance::Dark));
    }

    #[test]
    fn dark_windows_stay_black() {
        for theme in [Theme::Black, Theme::Graphite, Theme::Glass] {
            assert_eq!(window_fill(theme, false), Color(0, 0, 0, 255));
        }
    }

    #[test]
    fn light_window_fills_match_the_stylesheet() {
        let css = include_str!("../../src/index.css");
        for theme in [Theme::Black, Theme::Graphite, Theme::Glass] {
            let Color(r, g, b, _) = window_fill(theme, true);
            let decl = format!("--window-fill: #{r:02x}{g:02x}{b:02x};");
            assert!(css.contains(&decl), "index.css has no `{decl}` for {theme:?}");
        }
    }
}
