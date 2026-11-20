//! Per-app volume through the Windows audio session mixer (the one behind
//! Volume mixer in Settings), for the Now playing card.
//!
//! Media sessions (SMTC) and audio sessions are separate Windows systems with
//! no shared id, so an app's audio sessions are found by the file name of the
//! process that owns them, matched against the executable names the media
//! plugin derives from the player's app id.

use serde::Serialize;

/// An app's volume as the mixer shows it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppVolume {
    /// 0.0–1.0, the app's own slider (not the system volume).
    pub level: f32,
    pub muted: bool,
}

/// Whether a process image path belongs to one of `names` (lowercase
/// executable file names, e.g. `chrome.exe`).
///
/// @example matches(r"C:\Program Files\Google\Chrome\Application\chrome.exe", &["chrome.exe".into()]) // true
pub fn matches(path: &str, names: &[String]) -> bool {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    names.iter().any(|n| file.eq_ignore_ascii_case(n))
}

/// Clamps a requested level to the mixer's range; NaN reads as silence.
pub fn clamp_level(level: f32) -> f32 {
    if level.is_nan() { 0.0 } else { level.clamp(0.0, 1.0) }
}

#[cfg(windows)]
pub use imp::{process_path, read, set_level, set_muted};

/// Non-Windows builds have no audio mixer.
#[cfg(not(windows))]
pub fn read(_names: &[String]) -> Option<AppVolume> {
    None
}

/// Non-Windows builds have no audio mixer.
#[cfg(not(windows))]
pub fn set_level(_names: &[String], _level: f32) -> Result<(), String> {
    Err("Per-app volume needs Windows".into())
}

/// Non-Windows builds have no audio mixer.
#[cfg(not(windows))]
pub fn set_muted(_names: &[String], _muted: bool) -> Result<(), String> {
    Err("Per-app volume needs Windows".into())
}

#[cfg(windows)]
mod imp {
    use super::{clamp_level, matches, AppVolume};
    use windows::core::{Interface, GUID, PWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

    /// Full image path of a process, or `None` when it's gone or protected.
    pub fn process_path(pid: u32) -> Option<String> {
        // SAFETY: the handle is closed before returning and the buffer outlives the call.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
            let _ = CloseHandle(handle);
            ok.ok()?;
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// Volume controls of every audio session on the default output device
    /// owned by a process named in `names`.
    ///
    /// NOTE: the calling thread must have COM initialised; the media
    /// worker, the only caller, owns that for its whole life.
    fn controls(names: &[String]) -> windows::core::Result<Vec<ISimpleAudioVolume>> {
        // SAFETY: plain COM calls on interfaces this function owns.
        unsafe {
            let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = devices.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
            let sessions = manager.GetSessionEnumerator()?;
            let mut out = Vec::new();
            for i in 0..sessions.GetCount()? {
                let Ok(control) = sessions.GetSession(i) else { continue };
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
                // NOTE: pid 0 is the system sounds session.
                let pid = control2.GetProcessId().unwrap_or(0);
                if pid == 0 || !process_path(pid).is_some_and(|p| matches(&p, names)) {
                    continue;
                }
                if let Ok(volume) = control.cast::<ISimpleAudioVolume>() {
                    out.push(volume);
                }
            }
            Ok(out)
        }
    }

    /// The app's volume, or `None` when it has no audio session right now
    /// (some players only open one while sound is playing).
    ///
    /// NOTE: browsers can hold several sessions; the first one's level is
    /// shown, and the app counts as muted only when every session is.
    pub fn read(names: &[String]) -> Option<AppVolume> {
        let list = controls(names).ok()?;
        let first = list.first()?;
        // SAFETY: COM getters on live interfaces.
        unsafe {
            let level = first.GetMasterVolume().ok()?;
            let muted = list.iter().all(|v| v.GetMute().map(|m| m.as_bool()).unwrap_or(false));
            Some(AppVolume { level: clamp_level(level), muted })
        }
    }

    /// Sets every one of the app's sessions to `level` (0.0–1.0) and unmutes
    /// them, the way dragging the mixer's slider does.
    pub fn set_level(names: &[String], level: f32) -> Result<(), String> {
        apply(names, |v| unsafe {
            v.SetMasterVolume(clamp_level(level), &GUID::zeroed())?;
            v.SetMute(false, &GUID::zeroed())
        })
    }

    /// Mutes or unmutes every one of the app's sessions.
    pub fn set_muted(names: &[String], muted: bool) -> Result<(), String> {
        apply(names, |v| unsafe { v.SetMute(muted, &GUID::zeroed()) })
    }

    fn apply(names: &[String], f: impl Fn(&ISimpleAudioVolume) -> windows::core::Result<()>) -> Result<(), String> {
        let list = controls(names).map_err(|e| format!("Couldn't reach the volume mixer ({e})"))?;
        if list.is_empty() {
            return Err("This player has no sound playing to adjust".into());
        }
        list.iter().try_for_each(&f).map_err(|e| format!("Windows didn't change the volume ({e})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_by_file_name_only() {
        let names = vec!["chrome.exe".to_string(), "spotify.exe".to_string()];
        assert!(matches(r"C:\Program Files\Google\Chrome\Application\chrome.exe", &names));
        assert!(matches(r"C:\Users\me\AppData\Roaming\Spotify\Spotify.exe", &names));
        assert!(matches("chrome.exe", &names));
        assert!(!matches(r"C:\chrome.exe.backup\vlc.exe", &names));
        assert!(!matches(r"C:\tools\notchrome.exe", &names));
    }

    #[test]
    fn levels_stay_in_range() {
        assert_eq!(clamp_level(1.4), 1.0);
        assert_eq!(clamp_level(-0.2), 0.0);
        assert_eq!(clamp_level(f32::NAN), 0.0);
        assert_eq!(clamp_level(0.35), 0.35);
    }

    /// The mixer itself, against whatever is playing; nothing is changed.
    #[cfg(windows)]
    #[test]
    fn reads_the_mixer_without_failing() {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
        // SAFETY: balanced by CoUninitialize below on this thread.
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let _ = read(&["explorer.exe".to_string()]);
        if com {
            unsafe { CoUninitialize() };
        }
    }
}
