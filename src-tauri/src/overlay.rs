//! Native window behavior that makes the pill feel like part of the OS.
//!
//! Responsibilities:
//! - Click-through: the window ignores the mouse except while the cursor is
//!   over the visible black shape, which the UI reports as a [`HitRect`].
//! - Hover signal: emits `pill://hover` to the UI on enter/leave, because the
//!   webview receives no mouse events while the window is click-through.
//! - No focus stealing: `WS_EX_NOACTIVATE` stays set until the UI explicitly
//!   asks for keyboard focus, and focus is handed back on release.
//! - Hides while a full-screen app owns the pill's monitor.
//! - Re-places the window when the display layout changes.

use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;

/// Event emitted with `true`/`false` when the cursor enters or leaves the pill.
pub const HOVER_EVENT: &str = "pill://hover";

/// Event emitted with `true` while another window is being moved or resized
/// and `false` after, so the notch can step aside for Windows' snap bar.
pub const STEP_ASIDE_EVENT: &str = "pill://step-aside";

/// The visible pill shape in logical (CSS) pixels relative to the window.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Radius of the bottom corners. The notch hangs from the screen edge, so
    /// its top corners are square.
    pub radius: f64,
}

impl HitRect {
    /// Whether a logical-pixel point lies inside the bottom-rounded rectangle.
    pub fn contains(&self, px: f64, py: f64) -> bool {
        if px < self.x || py < self.y || px >= self.x + self.width || py >= self.y + self.height {
            return false;
        }
        let r = self.radius.clamp(0.0, self.width.min(self.height) / 2.0);
        let arc_top = self.y + self.height - r;
        if py <= arc_top {
            return true;
        }
        // Below the arcs' centers, a point near a side must be within r of the arc center.
        let cx = px.clamp(self.x + r, self.x + self.width - r);
        let (dx, dy) = (px - cx, py - arc_top);
        dx * dx + dy * dy <= r * r
    }
}

/// Converts a physical screen cursor position into the window's logical
/// space and tests it against `rect`.
pub fn cursor_hits(rect: &HitRect, cursor: (i32, i32), window_origin: (i32, i32), scale: f64) -> bool {
    let lx = f64::from(cursor.0 - window_origin.0) / scale;
    let ly = f64::from(cursor.1 - window_origin.1) / scale;
    rect.contains(lx, ly)
}

/// Screen-space rectangle in physical pixels (left, top, right, bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Whether `window` covers all of `monitor`. Windows commonly overhang the
/// monitor edge by their invisible resize border, so overhang counts.
pub fn covers(window: Bounds, monitor: Bounds) -> bool {
    window.left <= monitor.left
        && window.top <= monitor.top
        && window.right >= monitor.right
        && window.bottom >= monitor.bottom
}

/// Whether an executable (lowercased file name) is a screen-capture or
/// utility overlay that covers the monitor without being a full-screen app.
pub fn is_overlay_tool(exe: &str) -> bool {
    const EXACT: &[&str] = &[
        "snippingtool.exe",
        "screenclippinghost.exe",
        "screensketch.exe",
        "sharex.exe",
        "greenshot.exe",
        "lightshot.exe",
        "snagit32.exe",
        "snagiteditor.exe",
        "flameshot.exe",
        "obs64.exe",
    ];
    EXACT.contains(&exe) || exe.starts_with("powertoys")
}

/// Shared state between Tauri commands and the overlay thread.
#[derive(Default)]
pub struct OverlayState {
    /// Current visible shape; `None` makes the whole window click-through.
    pub hit: Mutex<Option<HitRect>>,
    /// Monitor index from config; `None` = primary.
    pub monitor: Mutex<Option<usize>>,
    /// True while the UI holds keyboard focus (quick-add, editing).
    pub focused: AtomicBool,
    /// True while mouse input passes through the window to apps below.
    pub click_through: AtomicBool,
    /// Window that was in the foreground before Pill took focus.
    pub previous_foreground: AtomicIsize,
    /// Set when the display layout must be re-applied on the next tick.
    pub replace_requested: AtomicBool,
    /// Keep the window topmost (config `onTop`).
    pub on_top: AtomicBool,
}

#[cfg(windows)]
pub use win::{release_focus, show_without_focus, spawn, take_focus};

#[cfg(windows)]
mod win {
    use super::*;
    use crate::placement;
    use std::sync::Arc;
    use std::time::Duration;
    use tauri::{Emitter, Runtime, WebviewWindow};
    use windows::Win32::Foundation::{CloseHandle, COLORREF, HWND, POINT, RECT};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
        IsWindow, IsZoomed, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW,
        SetWindowPos, ShowWindowAsync, GWL_EXSTYLE, GWL_STYLE, HWND_NOTOPMOST, HWND_TOPMOST, LWA_ALPHA,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE,
        WS_CAPTION, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
        GetWindowThreadProcessId, GetGUIThreadInfo, GUITHREADINFO, GUI_INMOVESIZE,
    };

    const TICK: Duration = Duration::from_millis(16);
    const FULLSCREEN_EVERY: u64 = 15; // ~250 ms
    const TOPMOST_EVERY: u64 = 120; // ~2 s
    const LAYOUT_EVERY: u64 = 120; // ~2 s

    fn hwnd_of(raw: isize) -> HWND {
        HWND(raw as *mut core::ffi::c_void)
    }

    /// Starts the overlay loop on a dedicated thread.
    ///
    /// The thread polls at ~60 Hz. Polling is used instead of a low-level
    /// mouse hook because hooks run on every mouse event system-wide and
    /// Windows silently removes hooks that are slow to respond.
    pub fn spawn<R: Runtime>(window: WebviewWindow<R>, state: Arc<OverlayState>) {
        let raw = match window.hwnd() {
            Ok(h) => h.0 as isize,
            Err(e) => {
                log::error!("pill window has no HWND: {e}");
                return;
            }
        };
        state.click_through.store(true, Ordering::Relaxed);
        sync_style(hwnd_of(raw), &state);
        let spawned = std::thread::Builder::new()
            .name("pill-overlay".into())
            .spawn(move || run(window, state, raw));
        if let Err(e) = spawned {
            log::error!("failed to start overlay thread: {e}");
        }
    }

    fn run<R: Runtime>(window: WebviewWindow<R>, state: Arc<OverlayState>, raw: isize) {
        let mut interactive = false;
        let mut hidden = false;
        // Starts unknown so the first tick applies the configured band.
        let mut was_on_top = !state.on_top.load(Ordering::Relaxed);
        let mut aside = false;
        // Set when the cursor arrives with the button already down (a drag
        // from elsewhere); the pill ignores it until the button is released.
        let mut arrived_pressed = false;
        let mut layout = placement::layout_signature(&window);
        let mut tick: u64 = 0;

        loop {
            std::thread::sleep(TICK);
            tick = tick.wrapping_add(1);
            let hwnd = hwnd_of(raw);
            if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                return;
            }

            // Tao rewrites GWL_EXSTYLE from its own flags on some state changes,
            // which drops bits it doesn't know about, so re-assert ours.
            sync_style(hwnd, &state);

            if tick.is_multiple_of(FULLSCREEN_EVERY) {
                let fs = foreground_is_fullscreen(hwnd);
                if fs != hidden {
                    hidden = fs;
                    unsafe {
                        let _ = ShowWindowAsync(hwnd, if fs { SW_HIDE } else { SW_SHOWNOACTIVATE });
                    }
                    if !fs {
                        assert_topmost(hwnd);
                    }
                }
            }
            let on_top = state.on_top.load(Ordering::Relaxed) || state.focused.load(Ordering::Relaxed);
            if on_top != was_on_top {
                was_on_top = on_top;
                set_topmost(hwnd, on_top);
            }
            if on_top && !hidden && !aside && tick.is_multiple_of(TOPMOST_EVERY) {
                assert_topmost(hwnd);
            }
            let forced = state.replace_requested.swap(false, Ordering::Relaxed);
            if forced || tick.is_multiple_of(LAYOUT_EVERY) {
                let now = placement::layout_signature(&window);
                if forced || now != layout {
                    layout = now;
                    let monitor = *state.monitor.lock().unwrap();
                    if let Err(e) = placement::place(&window, monitor) {
                        log::warn!("re-placing pill failed: {e}");
                    }
                }
            }

            // Windows shows its snap-layouts bar at the top center while a
            // window is dragged, exactly where the notch sits, so the notch
            // steps aside (invisible and click-through) for the whole drag.
            let moving = other_window_moving(hwnd);
            if moving != aside {
                aside = moving;
                let _ = window.emit(STEP_ASIDE_EVENT, aside);
            }

            let pressed = unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) } < 0;
            let over = !hidden && !aside && cursor_over_shape(hwnd, &state);
            if !pressed {
                arrived_pressed = false;
            } else if over && !interactive {
                arrived_pressed = true;
            }
            // Keep input while a drag that started inside is still held, so
            // text selection or card drags don't fall through mid-gesture.
            let dragging = interactive && pressed;
            let want = (over && !arrived_pressed) || (dragging && !hidden && !aside);
            if want != interactive {
                interactive = want;
                state.click_through.store(!want, Ordering::Relaxed);
                sync_style(hwnd, &state);
                log::debug!(
                    "pointer {} (cursor {:?}, window {:?}, rect {:?})",
                    if want { "entered" } else { "left" },
                    debug_cursor(),
                    debug_window(hwnd),
                    *state.hit.lock().unwrap()
                );
                let _ = window.emit(HOVER_EVENT, want);
            }
        }
    }

    /// True while the foreground thread is in a move/size loop for a window
    /// other than the pill (a title-bar drag, Snap, or a border resize).
    fn other_window_moving(pill: HWND) -> bool {
        let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if unsafe { GetGUIThreadInfo(0, &mut info) }.is_err() {
            return false;
        }
        (info.flags.0 & GUI_INMOVESIZE.0) != 0 && info.hwndMoveSize != pill
    }

    fn debug_cursor() -> (i32, i32) {
        let mut p = POINT::default();
        let _ = unsafe { GetCursorPos(&mut p) };
        (p.x, p.y)
    }

    fn debug_window(hwnd: HWND) -> (i32, i32, i32, i32, u32) {
        let mut r = RECT::default();
        let _ = unsafe { GetWindowRect(hwnd, &mut r) };
        (r.left, r.top, r.right, r.bottom, unsafe { GetDpiForWindow(hwnd) })
    }

    fn cursor_over_shape(hwnd: HWND, state: &OverlayState) -> bool {
        let Some(rect) = *state.hit.lock().unwrap() else {
            return false;
        };
        let mut cursor = POINT::default();
        let mut win = RECT::default();
        unsafe {
            if GetCursorPos(&mut cursor).is_err() || GetWindowRect(hwnd, &mut win).is_err() {
                return false;
            }
        }
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let scale = if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 };
        cursor_hits(&rect, (cursor.x, cursor.y), (win.left, win.top), scale)
    }

    /// Sets the extended styles Pill depends on, touching only our bits.
    ///
    /// Click-through is `WS_EX_LAYERED | WS_EX_TRANSPARENT`: a layered window
    /// with the transparent bit is skipped by hit-testing, so clicks reach the
    /// app below. The layered bit stays on permanently and only the
    /// transparent bit is toggled.
    ///
    /// WARNING: Don't use Tauri's `set_ignore_cursor_events` for this. Tao
    /// rebuilds the whole window style from its own flags, and because the
    /// window is shown with a raw `ShowWindow` (so it never steals focus), tao
    /// thinks it's invisible and strips `WS_VISIBLE`, which hides the pill on
    /// the first hover.
    fn apply_ex_style(hwnd: HWND, focusable: bool, click_through: bool) {
        let layered = WS_EX_LAYERED.0 as isize;
        let transparent = WS_EX_TRANSPARENT.0 as isize;
        let noactivate = WS_EX_NOACTIVATE.0 as isize;
        unsafe {
            let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let mut next = current | WS_EX_TOOLWINDOW.0 as isize | layered;
            next = if focusable { next & !noactivate } else { next | noactivate };
            next = if click_through { next | transparent } else { next & !transparent };
            if next != current {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
                // A freshly layered window is invisible until its attributes
                // are set; full opacity keeps per-pixel webview transparency.
                if current & layered == 0 {
                    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
                }
            }
        }
    }

    fn sync_style(hwnd: HWND, state: &OverlayState) {
        apply_ex_style(
            hwnd,
            state.focused.load(Ordering::Relaxed),
            state.click_through.load(Ordering::Relaxed),
        );
    }

    /// Drops out of (or back into) the topmost band.
    fn set_topmost(hwnd: HWND, on: bool) {
        if on {
            return assert_topmost(hwnd);
        }
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        }
    }

    fn assert_topmost(hwnd: HWND) {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }

    fn class_name(hwnd: HWND) -> String {
        let mut buf = [0u16; 128];
        let len = unsafe { GetClassNameW(hwnd, &mut buf) };
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }

    fn monitor_bounds(hwnd: HWND) -> Option<(isize, Bounds)> {
        unsafe {
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            if !GetMonitorInfoW(mon, &mut info).as_bool() {
                return None;
            }
            let r = info.rcMonitor;
            Some((mon.0 as isize, Bounds { left: r.left, top: r.top, right: r.right, bottom: r.bottom }))
        }
    }

    fn foreground_is_fullscreen(own: HWND) -> bool {
        let fg = unsafe { GetForegroundWindow() };
        if fg.is_invalid() || fg == own {
            return false;
        }
        // The desktop and taskbar cover the monitor but aren't "apps".
        if matches!(
            class_name(fg).as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return false;
        }
        let (Some((own_mon, mon)), Some((fg_mon, _))) = (monitor_bounds(own), monitor_bounds(fg)) else {
            return false;
        };
        if own_mon != fg_mon {
            return false;
        }
        let mut r = RECT::default();
        if unsafe { GetWindowRect(fg, &mut r) }.is_err() {
            return false;
        }
        // With an auto-hiding taskbar, a maximized window's resize border can
        // overhang the whole monitor. A captioned, maximized window is a normal
        // app window, not full-screen.
        let style = unsafe { GetWindowLongPtrW(fg, GWL_STYLE) };
        let captioned = style & WS_CAPTION.0 as isize == WS_CAPTION.0 as isize;
        if captioned && unsafe { IsZoomed(fg) }.as_bool() {
            return false;
        }
        if !covers(Bounds { left: r.left, top: r.top, right: r.right, bottom: r.bottom }, mon) {
            return false;
        }
        // Screen-capture and utility overlays (Snipping Tool, PowerToys…) are
        // full-monitor, often topmost windows. Hiding for them would make the
        // pill vanish exactly when someone tries to screenshot it.
        let ex = unsafe { GetWindowLongPtrW(fg, GWL_EXSTYLE) };
        if ex & WS_EX_TOPMOST.0 as isize != 0 {
            return false;
        }
        !process_exe(fg).is_some_and(|exe| is_overlay_tool(&exe))
    }

    /// Lowercased executable file name of the process owning `hwnd`.
    fn process_exe(hwnd: HWND) -> Option<String> {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == 0 {
            return None;
        }
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(handle);
            ok.ok()?;
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit(['\\', '/']).next().map(str::to_ascii_lowercase)
        }
    }

    /// Shows the window without activating it (Tauri's `show()` activates).
    pub fn show_without_focus<R: Runtime>(window: &WebviewWindow<R>) {
        if let Ok(h) = window.hwnd() {
            let hwnd = hwnd_of(h.0 as isize);
            unsafe {
                let _ = ShowWindowAsync(hwnd, SW_SHOWNOACTIVATE);
            }
            assert_topmost(hwnd);
        }
    }

    /// Makes the pill focusable and brings it to the foreground, remembering
    /// which window had focus so [`release_focus`] can hand it back.
    pub fn take_focus<R: Runtime>(window: &WebviewWindow<R>, state: &OverlayState) {
        let Ok(h) = window.hwnd() else { return };
        let own = hwnd_of(h.0 as isize);
        if !state.focused.swap(true, Ordering::Relaxed) {
            let prev = unsafe { GetForegroundWindow() };
            if prev != own {
                state.previous_foreground.store(prev.0 as isize, Ordering::Relaxed);
            }
        }
        apply_ex_style(own, true, state.click_through.load(Ordering::Relaxed));
        // NOTE: Tauri's `set_focus` isn't used: tao rebuilds the window style
        // from its own flags there, which would undo our extended styles.
        let ok = unsafe { SetForegroundWindow(own) }.as_bool();
        log::debug!("take_focus: SetForegroundWindow -> {ok}");
    }

    /// Returns the pill to its non-activating state and restores the window
    /// that was focused before [`take_focus`].
    pub fn release_focus<R: Runtime>(window: &WebviewWindow<R>, state: &OverlayState) {
        if !state.focused.swap(false, Ordering::Relaxed) {
            return;
        }
        let Ok(h) = window.hwnd() else { return };
        let own = hwnd_of(h.0 as isize);
        sync_style(own, state);
        let prev = hwnd_of(state.previous_foreground.swap(0, Ordering::Relaxed));
        unsafe {
            // Only hand focus back if Pill still has it. If the user already
            // clicked into another app, yanking focus away from it would be rude.
            if GetForegroundWindow() == own && !prev.is_invalid() && IsWindow(Some(prev)).as_bool() {
                let _ = SetForegroundWindow(prev);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTCH: HitRect = HitRect { x: 278.0, y: 0.0, width: 204.0, height: 30.0, radius: 12.0 };

    #[test]
    fn center_hits_and_outside_misses() {
        assert!(NOTCH.contains(380.0, 15.0));
        assert!(!NOTCH.contains(277.0, 15.0));
        assert!(!NOTCH.contains(380.0, 30.0));
    }

    #[test]
    fn top_corners_are_square_bottom_corners_round() {
        // Flush with the screen edge: the top-left corner pixel is a hit.
        assert!(NOTCH.contains(278.0, 0.0));
        // Bottom-left bounding-box corner lies outside the 12 px arc.
        assert!(!NOTCH.contains(278.5, 29.5));
        assert!(NOTCH.contains(290.0, 29.5));
    }

    #[test]
    fn scale_converts_physical_cursor() {
        // 150% scaling: window at (100, 0), logical point (380, 15) → physical (670, 22.5).
        assert!(cursor_hits(&NOTCH, (100 + 570, 22), (100, 0), 1.5));
        assert!(!cursor_hits(&NOTCH, (100 + 100, 22), (100, 0), 1.5));
    }

    #[test]
    fn capture_tools_are_not_fullscreen_apps() {
        assert!(is_overlay_tool("snippingtool.exe"));
        assert!(is_overlay_tool("powertoys.screenruler.exe"));
        assert!(!is_overlay_tool("eldenring.exe"));
        assert!(!is_overlay_tool("chrome.exe"));
    }

    #[test]
    fn fullscreen_detection_allows_overhang() {
        let mon = Bounds { left: 0, top: 0, right: 1920, bottom: 1080 };
        assert!(covers(mon, mon));
        assert!(covers(Bounds { left: -8, top: -8, right: 1928, bottom: 1088 }, mon));
        assert!(!covers(Bounds { left: 0, top: 0, right: 1920, bottom: 1040 }, mon));
    }
}
