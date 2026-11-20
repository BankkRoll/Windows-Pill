//! Windows toast notifications.
//!
//! Toasts are built from XML through WinRT directly so clicks can use
//! protocol activation: Windows opens the URL itself, with nothing for Pill
//! to keep alive. "Open PR" buttons open the PR's https URL; clicking the
//! toast body opens a `pill://` link, which Windows hands to a new
//! `pill.exe` whose single-instance check forwards it to the running Pill
//! (see [`crate::notify::parse_link`]).
//!
//! Windows only shows toasts for an AppUserModelID that a Start menu shortcut
//! carries. Both installers (NSIS and MSI) stamp the bundle identifier from
//! `tauri.conf.json` onto Pill's shortcut, so installed builds use it as their
//! AUMID; builds run straight from `target\` have no shortcut and borrow
//! PowerShell's.
//!
//! NOTE: Pill is unpackaged, so foreground (in-process) activation would
//! need a registered COM activator and a shortcut carrying a CLSID. Protocol
//! activation needs only a per-user URL scheme in `HKCU\Software\Classes`,
//! which [`register_protocol`] writes at startup, and it also works for
//! toasts clicked from the notification center after Pill has quit.

use tauri::AppHandle;

/// URL scheme of links that bring Pill up on a run or task.
pub const SCHEME: &str = "pill";

/// AUMID Windows accepts for unregistered (development) builds.
const DEV_AUMID: &str = r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Toast XML for a ready pull request.
pub fn pr_ready_xml(title: &str, project: &str, url: &str) -> String {
    let (t, p, u) = (escape(title), escape(project), escape(url));
    format!(
        r#"<toast activationType="protocol" launch="{u}"><visual><binding template="ToastGeneric"><text>Draft PR ready</text><text>{t}</text><text placement="attribution">{p}</text></binding></visual><actions><action content="Open PR" activationType="protocol" arguments="{u}"/></actions></toast>"#
    )
}

/// Toast XML for a moment that needs the user. Clicking it opens `launch`;
/// `pr_url` adds an "Open PR" button.
///
/// @param title First line, e.g. "Plan ready".
/// @param body Second line, usually the task title and what to do.
/// @param attribution Small grey line, usually the project name.
/// @param launch Link opened when the toast itself is clicked.
/// @param pr_url Pull request to offer next to it, if any.
pub fn need_xml(title: &str, body: &str, attribution: &str, launch: &str, pr_url: Option<&str>) -> String {
    let actions = pr_url
        .map(|u| format!(r#"<actions><action content="Open PR" activationType="protocol" arguments="{}"/></actions>"#, escape(u)))
        .unwrap_or_default();
    format!(
        r#"<toast activationType="protocol" launch="{}"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text><text placement="attribution">{}</text></binding></visual>{actions}</toast>"#,
        escape(launch),
        escape(title),
        escape(body),
        escape(attribution),
    )
}

/// Shows "Draft PR ready" with an Open PR button. Failures are logged only:
/// a missing toast must never fail a run.
pub fn pr_ready(app: &AppHandle, title: &str, project: &str, url: &str) {
    show_logged(app, &pr_ready_xml(title, project, url));
}

/// Shows a toast built by [`need_xml`]. Failures are logged only.
pub fn need(app: &AppHandle, title: &str, body: &str, attribution: &str, launch: &str, pr_url: Option<&str>) {
    show_logged(app, &need_xml(title, body, attribution, launch, pr_url));
}

/// A plain two-line notification that opens nothing when clicked, for news
/// that isn't about one task, such as a usage limit pausing queued runs.
pub fn message(app: &AppHandle, title: &str, body: &str) {
    let xml = format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        escape(title),
        escape(body)
    );
    show_logged(app, &xml);
}

fn show_logged(app: &AppHandle, xml: &str) {
    #[cfg(windows)]
    if let Err(e) = show(app, xml) {
        log::warn!("toast failed: {e}");
    }
    #[cfg(not(windows))]
    let _ = (app, xml);
}

/// AUMID for toasts: the bundle identifier for installed builds, or the
/// borrowed development one when the executable runs from a Cargo `target`
/// folder.
#[cfg_attr(not(windows), allow(dead_code))]
fn app_user_model_id(identifier: &str, exe: Option<&std::path::Path>) -> String {
    let dev = exe.is_none_or(|p| p.components().any(|c| c.as_os_str() == "target"));
    if dev { DEV_AUMID.to_string() } else { identifier.to_string() }
}

/// Gives the process the installed AUMID so its taskbar button, jump list and
/// toasts group under Pill's Start menu shortcut. A no-op for development
/// builds, which have no shortcut to match.
pub fn register_app_id(app: &AppHandle) {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().ok();
        let id = app_user_model_id(&app.config().identifier, exe.as_deref());
        if id != DEV_AUMID {
            // SAFETY: called once on the main thread during setup, before any window is shown.
            if let Err(e) = unsafe { windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(&windows::core::HSTRING::from(id)) } {
                log::warn!("setting the AppUserModelID: {e}");
            }
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
fn show(app: &AppHandle, xml: &str) -> windows::core::Result<()> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    // Installed builds have a Start menu shortcut carrying the app identifier;
    // development builds run from `target\` and borrow PowerShell's AUMID.
    let dev = std::env::current_exe()
        .map(|p| p.components().any(|c| c.as_os_str() == "target"))
        .unwrap_or(true);
    let aumid = if dev { DEV_AUMID.to_string() } else { app.config().identifier.clone() };
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(aumid))?.Show(&toast)
}

/// Registers the `pill://` scheme for the current user so clicking a toast
/// launches this executable with the link. A scheme another program owns
/// (its command names a different executable) is left alone.
#[cfg(windows)]
pub fn register_protocol() -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
        REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    };

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let command = format!("\"{}\" \"%1\"", exe.display());
    let base = format!(r"Software\Classes\{SCHEME}");
    let command_key = format!(r"{base}\shell\open\command");

    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: `buf` outlives the call and `len` holds its size in bytes.
    let existing = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(command_key.as_str()),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if existing.is_ok() {
        let current = String::from_utf16_lossy(&buf[..(len as usize / 2)]).trim_end_matches('\0').to_string();
        if current == command {
            return Ok(());
        }
        if !protocol_is_ours(&current, &exe) {
            return Err(format!("{SCHEME}:// belongs to another program ({current}); toast clicks won't open Pill"));
        }
    }

    let set = |key: &str, name: Option<&str>, value: &str| -> Result<(), String> {
        let mut hkey = HKEY::default();
        // SAFETY: plain registry calls on a key this closure opens and closes.
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(key),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut hkey,
                None,
            )
            .ok()
            .map_err(|e| format!("{key}: {e}"))?;
            let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
            let name = name.map(HSTRING::from);
            let result = RegSetValueExW(
                hkey,
                name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())),
                None,
                REG_SZ,
                Some(bytes),
            );
            let _ = RegCloseKey(hkey);
            result.ok().map_err(|e| format!("{key}: {e}"))
        }
    };
    set(&base, None, "URL:Pill")?;
    set(&base, Some("URL Protocol"), "")?;
    set(&command_key, None, &command)?;
    Ok(())
}

/// Whether a registered `pill://` command launches a Pill build: the same
/// executable file name, wherever it lives (an older install, another
/// checkout), so the newest Pill to start takes the scheme over.
pub fn protocol_is_ours(command: &str, exe: &std::path::Path) -> bool {
    let Some(name) = exe.file_name().map(|n| n.to_string_lossy().to_lowercase()) else { return false };
    let program = command.trim().trim_start_matches('"');
    let program = program.split('"').next().unwrap_or(program);
    std::path::Path::new(program)
        .file_name()
        .is_some_and(|n| n.to_string_lossy().to_lowercase() == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn xml_escapes_user_text() {
        let xml = pr_ready_xml("Fix <b> & \"quotes\"", "a&b", "https://x/pull/1?a=1&b=2");
        assert!(xml.contains("Fix &lt;b&gt; &amp; &quot;quotes&quot;"));
        assert!(xml.contains(r#"arguments="https://x/pull/1?a=1&amp;b=2""#));
        assert!(!xml.contains("<b>"));
    }

    #[test]
    fn need_toasts_launch_pill_and_offer_the_pr() {
        let xml = need_xml("Plan ready", "Fix <scatter>", "game", "pill://open?run=r_1&task=t_1", Some("https://x/pull/2"));
        assert!(xml.contains(r#"launch="pill://open?run=r_1&amp;task=t_1""#));
        assert!(xml.contains("Fix &lt;scatter&gt;"));
        assert!(xml.contains(r#"arguments="https://x/pull/2""#));
        let plain = need_xml("Run failed", "x", "game", "pill://open?task=t_1", None);
        assert!(!plain.contains("<actions>"));
    }

    #[test]
    fn protocol_ownership_matches_the_executable_name() {
        let exe = Path::new(r"C:\Program Files\Pill\pill.exe");
        assert!(protocol_is_ours(r#""C:\dev\pill\target\debug\pill.exe" "%1""#, exe));
        assert!(protocol_is_ours(r#""C:\Program Files\Pill\PILL.EXE" "%1""#, exe));
        assert!(!protocol_is_ours(r#""C:\Other\pillbox.exe" "%1""#, exe));
        assert!(!protocol_is_ours("", exe));
    }

    #[test]
    fn installed_builds_use_the_bundle_identifier() {
        let installed = std::path::Path::new(r"C:\Users\me\AppData\Local\Pill\pill.exe");
        assert_eq!(app_user_model_id("io.github.bankkroll.pill", Some(installed)), "io.github.bankkroll.pill");
        let dev = std::path::Path::new(r"C:\src\pill\src-tauri\target\debug\pill.exe");
        assert_eq!(app_user_model_id("io.github.bankkroll.pill", Some(dev)), DEV_AUMID);
        assert_eq!(app_user_model_id("io.github.bankkroll.pill", None), DEV_AUMID);
    }
}
