//! Positions the pill window at the top-center of the chosen monitor.
//!
//! The window is always sized to the largest pill state (in logical pixels),
//! so the morph animation never resizes the native window.

use tauri::{LogicalSize, Monitor, PhysicalPosition, Runtime, WebviewWindow};

/// Logical size of the transparent host window. Large enough for the
/// expanded panel (680 × 440) plus its shoulders, drop shadow and spring overshoot.
pub const WINDOW_SIZE: LogicalSize<f64> = LogicalSize { width: 800.0, height: 500.0 };

/// Top-left corner that centers a window of `win_width` horizontally on a
/// monitor and pins it to the monitor's top edge. All values are physical px.
pub fn top_center(mon_x: i32, mon_y: i32, mon_width: u32, win_width: u32) -> (i32, i32) {
    let dx = (i64::from(mon_width) - i64::from(win_width)) / 2;
    (mon_x + dx as i32, mon_y)
}

/// Picks the monitor from the config index, falling back to the primary
/// monitor and then to whichever monitor the OS lists first.
pub fn choose_monitor<R: Runtime>(window: &WebviewWindow<R>, index: Option<usize>) -> Option<Monitor> {
    let monitors = window.available_monitors().ok()?;
    index
        .and_then(|i| monitors.get(i).cloned())
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| monitors.into_iter().next())
}

/// A cheap fingerprint of the display layout, used to notice monitors being
/// plugged in, rearranged or rescaled.
pub fn layout_signature<R: Runtime>(window: &WebviewWindow<R>) -> Vec<(i32, i32, u32, u32, u64)> {
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width, s.height, m.scale_factor().to_bits())
        })
        .collect()
}

/// Moves and sizes the window onto the chosen monitor.
///
/// NOTE: The window is moved before it is resized. Moving onto a monitor with
/// a different scale triggers `WM_DPICHANGED`, which rescales the window; sizing
/// afterwards guarantees the final physical size matches the new monitor.
pub fn place<R: Runtime>(window: &WebviewWindow<R>, monitor_index: Option<usize>) -> tauri::Result<()> {
    let Some(mon) = choose_monitor(window, monitor_index) else {
        return Ok(());
    };
    let pos = mon.position();
    window.set_position(PhysicalPosition::new(pos.x, pos.y))?;
    let size = WINDOW_SIZE.to_physical::<u32>(mon.scale_factor());
    window.set_size(size)?;
    let (x, y) = top_center(pos.x, pos.y, mon.size().width, size.width);
    window.set_position(PhysicalPosition::new(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_on_primary() {
        assert_eq!(top_center(0, 0, 1920, 760), (580, 0));
    }

    #[test]
    fn centers_on_offset_monitor_with_negative_origin() {
        assert_eq!(top_center(-2560, -200, 2560, 1140), (-2560 + 710, -200));
    }

    #[test]
    fn window_wider_than_monitor_overhangs_evenly() {
        assert_eq!(top_center(0, 0, 600, 760), (-80, 0));
    }
}
