//! Windows notifications: the Action Center count as Pill's notification
//! signal, and a card on the Today tab that lists and dismisses them.
//!
//! Responsibilities:
//! - Read Action Center through `UserNotificationListener`, which needs the
//!   user's permission ("Notification access" in Windows privacy settings).
//! - Publish the count as a plugin signal and the list as a typed [`Feed`]
//!   on [`NOTIFICATIONS_EVENT`]. The generic plugin widget has no room for
//!   per-item dismiss, app icons or screenshots, so this plugin has its own.
//! - Dismiss one notification or clear them all, in Action Center itself.
//! - Find each app's icon, and for Snipping Tool notifications the
//!   screenshot they announce: the listener exposes only a toast's text,
//!   never its images, so the file is matched in the Screenshots folder by
//!   write time instead.
//!
//! COMPAT: verified on Windows 11 from an unpackaged build: `Current()`,
//! `GetAccessStatus()`, `RequestAccessAsync()` and `GetNotificationsAsync()`
//! all work, but subscribing to `NotificationChanged` fails with
//! `0x80070490` (Element not found) because that event needs package
//! identity. So Pill polls every four seconds, and refreshes right after a
//! dismiss.
//!
//! SECURITY: notification text stays in memory and in the UI; it's never
//! logged or written to disk.

use super::protocol::{clean_text, Icon, PluginSignal, Tone};
use super::{Runtime, Sink};
use base64::Engine;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Built-in plugin id.
pub const ID: &str = "notifications";

/// Event carrying the current [`Feed`], or `null` when there's nothing to
/// list (plugin off, or no permission).
pub const NOTIFICATIONS_EVENT: &str = "pill://notifications";

/// Most notifications sent to the UI; the count still covers all of them.
const MAX_LISTED: usize = 100;
const TITLE_CHARS: usize = 120;
/// The card clamps the body to two lines; this only bounds the payload.
const BODY_CHARS: usize = 300;
/// How far a screenshot's write time may be from its notification's
/// creation time to count as the screenshot it announces.
const SCREENSHOT_WINDOW_MS: i64 = 60_000;
/// App icon edge in pixels: shown at 20 CSS px, so this stays sharp at 200%.
const ICON_SIZE: u32 = 40;
/// Windows ticks (100 ns since 1601-01-01) at the Unix epoch.
const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
/// How long a dismiss waits for the worker before giving up.
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// One notification in Action Center. Mirrors TS `NotificationItem`.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Action Center's id; what dismissing names.
    pub id: u32,
    /// The sending app's display name.
    pub app: String,
    pub title: String,
    pub body: String,
    /// Creation time in Unix milliseconds.
    pub created_ms: i64,
    /// The app's icon as a small PNG `data:` URL.
    pub icon: Option<String>,
    /// For Snipping Tool notifications, a JPEG `data:` URL thumbnail of the
    /// screenshot they announce.
    pub screenshot: Option<String>,
}

/// What the Today card lists. Mirrors TS `NotificationFeed`.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
    /// Every notification in Action Center, including ones not listed.
    pub total: u32,
    /// Newest first, at most [`MAX_LISTED`].
    pub notes: Vec<Note>,
}

/// The signal and feed for the notifications currently in Action Center.
///
/// @param notes Every notification read, in any order.
/// @returns The notification signal (empty when there are none) and the feed.
pub fn present(mut notes: Vec<Note>) -> (Vec<PluginSignal>, Feed) {
    notes.sort_by_key(|n| std::cmp::Reverse(n.created_ms));
    let total = notes.len() as u32;
    notes.truncate(MAX_LISTED);
    let signals = if total == 0 {
        Vec::new()
    } else {
        vec![PluginSignal {
            id: "count".into(),
            kind: Some("notification"),
            icon: Icon::Bell,
            tone: Tone::Neutral,
            count: total,
            label: if total == 1 { "1 notification".into() } else { format!("{total} notifications") },
        }]
    };
    (signals, Feed { total, notes })
}

/// Converts a Windows `DateTime` (100 ns ticks since 1601) to Unix
/// milliseconds.
///
/// @param ticks `DateTime::UniversalTime`.
pub fn ticks_to_unix_ms(ticks: i64) -> i64 {
    (ticks - UNIX_EPOCH_TICKS) / 10_000
}

/// Whether a notification comes from Snipping Tool, whose toasts announce
/// a screenshot it just saved ("Screenshot copied to clipboard").
///
/// @param aumid The sender's app user model id.
/// @param app The sender's display name, for builds that report no id.
pub fn is_screenshot_app(aumid: &str, app: &str) -> bool {
    aumid.to_ascii_lowercase().starts_with("microsoft.screensketch_") || app.eq_ignore_ascii_case("Snipping Tool")
}

/// The screenshot a notification announces: of the images written within
/// [`SCREENSHOT_WINDOW_MS`] of its creation, the closest in time.
///
/// @param created_ms The notification's creation time, Unix ms.
/// @param images Candidate files with their write times, Unix ms.
/// @returns The matching file, if any.
pub fn match_screenshot(created_ms: i64, images: &[(PathBuf, i64)]) -> Option<&Path> {
    images
        .iter()
        .map(|(path, written)| (path, (written - created_ms).abs()))
        .filter(|&(_, gap)| gap <= SCREENSHOT_WINDOW_MS)
        .min_by_key(|&(_, gap)| gap)
        .map(|(path, _)| path.as_path())
}

/// The key an app's icon is cached under: its app user model id, or its
/// display name for senders that report none. Case-insensitive, like both.
///
/// @param aumid The sender's app user model id.
/// @param app The sender's display name.
pub fn icon_key(aumid: &str, app: &str) -> String {
    let aumid = aumid.trim();
    if aumid.is_empty() {
        format!("name:{}", app.trim().to_lowercase())
    } else {
        format!("id:{}", aumid.to_lowercase())
    }
}

/// Turns an app logo (PNG or JPEG) into a small PNG `data:` URL, keeping
/// its transparency and aspect ratio.
///
/// @param bytes Encoded image.
/// @returns `None` when the image can't be decoded or is fully transparent.
pub fn icon_data_url(bytes: &[u8]) -> Option<String> {
    let mut img = image::load_from_memory(bytes).ok()?;
    if img.width() == 0 || img.height() == 0 {
        return None;
    }
    // NOTE: Store app logos are drawn with wide transparent margins for the
    // Start menu's plates; without them the glyph fills the card's icon slot.
    if let Some((x, y, w, h)) = opaque_bounds(&img.to_rgba8()) {
        img = img.crop_imm(x, y, w, h);
    }
    let small =if img.width() > ICON_SIZE || img.height() > ICON_SIZE { img.thumbnail(ICON_SIZE, ICON_SIZE) } else { img }.to_rgba8();
    // NOTE: some unpackaged apps hand back a blank placeholder; the card's
    // fallback tile reads better than an invisible icon.
    if small.pixels().all(|p| p.0[3] == 0) {
        return None;
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(small).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png)))
}

/// The smallest square-ish box around the visible pixels, with a one-pixel
/// margin, or `None` when nothing is visible or there's nothing to trim.
fn opaque_bounds(img: &image::RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p.0[3] > 8 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 == u32::MAX {
        return None;
    }
    let (x0, y0) = (x0.saturating_sub(1), y0.saturating_sub(1));
    let (x1, y1) = ((x1 + 1).min(img.width() - 1), (y1 + 1).min(img.height() - 1));
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    if w == img.width() && h == img.height() {
        return None;
    }
    // Keep the glyph's proportions centred in a square so wide and tall
    // marks don't stretch the slot differently.
    let side = w.max(h).min(img.width().min(img.height()));
    let cx = (x0 + w / 2).saturating_sub(side / 2).min(img.width() - side);
    let cy = (y0 + h / 2).saturating_sub(side / 2).min(img.height() - side);
    if side >= img.width() && side >= img.height() {
        return None;
    }
    Some((cx, cy, side, side))
}

// -----------------------------------------------------------------------------
// The worker's shared state
// -----------------------------------------------------------------------------

type Reply = Sender<Result<(), String>>;

/// Matched screenshot files by notification id.
type Shots = Vec<(u32, PathBuf)>;

/// Checks on a permission prompt: `None` while it is still open, then
/// `Ok` once the user answered (the access status tells how), or why the
/// prompt failed.
type AccessPoll = Box<dyn FnMut() -> Option<Result<(), String>> + Send>;

#[cfg_attr(not(windows), allow(dead_code))]
enum Cmd {
    /// A permission prompt opened on the main thread.
    AccessAsked(AccessPoll),
    Dismiss(u32, Reply),
    Clear(Reply),
    Stop,
}

/// The running worker and what it last published. A runtime that was
/// stopped (an older `epoch`) can't publish anymore.
struct Hub {
    epoch: u64,
    tx: Option<Sender<Cmd>>,
    feed: Option<Feed>,
    /// Matched screenshot files by notification id, for opening them.
    shots: Shots,
}

static HUB: Mutex<Hub> = Mutex::new(Hub { epoch: 0, tx: None, feed: None, shots: Vec::new() });

/// The notifications currently listed.
///
/// @returns `None` when the plugin is off or can't read Action Center.
pub fn feed() -> Option<Feed> {
    HUB.lock().unwrap().feed.clone()
}

/// Removes one notification from Action Center, then refreshes the feed.
/// Blocks until Windows answers.
///
/// @param id The notification's [`Note::id`].
/// @returns A message for the user if Windows refused.
pub fn dismiss(id: u32) -> Result<(), String> {
    ask(|reply| Cmd::Dismiss(id, reply))
}

/// Removes every notification from Action Center, then refreshes the feed.
/// Blocks until Windows answers.
///
/// @returns A message for the user if Windows refused.
pub fn clear_all() -> Result<(), String> {
    ask(Cmd::Clear)
}

/// `HRESULT_FROM_WIN32(ERROR_NOT_FOUND)`: the notification is already gone.
const NOT_FOUND: windows::core::HRESULT = windows::core::HRESULT(0x8007_0490_u32 as i32);

/// Removes one notification; one that's already gone counts as removed.
fn remove(listener: &windows::UI::Notifications::Management::UserNotificationListener, id: u32) -> windows::core::Result<()> {
    match listener.RemoveNotification(id) {
        Err(e) if e.code() == NOT_FOUND => Ok(()),
        other => other,
    }
}

/// Clears Action Center.
///
/// COMPAT: `ClearNotifications` fails with ERROR_NOT_FOUND when any listed
/// notification has expired or was removed by its app in the meantime, and
/// then clears nothing. Removing them one by one gets past that.
fn clear(listener: &windows::UI::Notifications::Management::UserNotificationListener) -> windows::core::Result<()> {
    if listener.ClearNotifications().is_ok() {
        return Ok(());
    }
    let list = listener.GetNotificationsAsync(windows::UI::Notifications::NotificationKinds::Toast)?.get()?;
    let mut first_error = None;
    for note in list {
        if let Err(e) = note.Id().and_then(|id| remove(listener, id)) {
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// The screenshot file matched to a listed notification.
///
/// @param id The notification's [`Note::id`].
pub fn screenshot_path(id: u32) -> Option<PathBuf> {
    HUB.lock().unwrap().shots.iter().find(|(n, _)| *n == id).map(|(_, p)| p.clone())
}

fn ask(cmd: impl FnOnce(Reply) -> Cmd) -> Result<(), String> {
    let tx = HUB.lock().unwrap().tx.clone().ok_or("Windows notifications are turned off")?;
    let (reply, answer) = mpsc::channel();
    tx.send(cmd(reply)).map_err(|_| "Windows notifications have stopped".to_string())?;
    answer.recv_timeout(REPLY_TIMEOUT).map_err(|_| "Windows didn't answer in time".to_string())?
}

/// Registers a new worker and returns its epoch.
#[cfg_attr(not(windows), allow(dead_code))]
fn attach(tx: Sender<Cmd>) -> u64 {
    let mut hub = HUB.lock().unwrap();
    hub.epoch += 1;
    hub.tx = Some(tx);
    hub.feed = None;
    hub.shots.clear();
    hub.epoch
}

/// Unregisters the worker of `epoch` (if it's still current) and tells the
/// UI the list is gone.
#[cfg_attr(not(windows), allow(dead_code))]
fn detach(app: &AppHandle, epoch: u64) {
    {
        let mut hub = HUB.lock().unwrap();
        if hub.epoch != epoch {
            return;
        }
        hub.epoch += 1;
        hub.tx = None;
        hub.feed = None;
        hub.shots.clear();
    }
    let _ = app.emit(NOTIFICATIONS_EVENT, None::<Feed>);
}

/// Stores what the worker of `epoch` read and emits the feed if it changed.
#[cfg_attr(not(windows), allow(dead_code))]
fn publish(app: &AppHandle, epoch: u64, feed: Option<Feed>, shots: Shots) {
    {
        let mut hub = HUB.lock().unwrap();
        if hub.epoch != epoch {
            return;
        }
        hub.shots = shots;
        if hub.feed == feed {
            return;
        }
        hub.feed = feed.clone();
    }
    let _ = app.emit(NOTIFICATIONS_EVENT, feed);
}

#[cfg(windows)]
pub use imp::start;

/// Non-Windows builds have no notification listener.
#[cfg(not(windows))]
pub fn start(_sink: Sink) -> Result<Box<dyn Runtime>, String> {
    Err("Windows notifications need Windows".into())
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::plugins::{Fix, Status, StatusState};
    use crate::system_settings::SystemPage;
    use std::collections::HashMap;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Instant, UNIX_EPOCH};
    use windows::core::GUID;
    use windows::ApplicationModel::AppDisplayInfo;
    use windows::Foundation::Size;
    use windows::Storage::Streams::DataReader;
    use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus as Access};
    use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED};
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, FOLDERID_Pictures, FOLDERID_Screenshots, KF_FLAG_DEFAULT};

    /// PERF: reading Action Center is one cross-process call; every few
    /// seconds keeps the badge current without measurable cost.
    const POLL: Duration = Duration::from_secs(4);
    /// How often an open permission prompt is checked for an answer.
    const PROMPT_POLL: Duration = Duration::from_millis(250);
    /// How long a permission prompt may stay open before Pill stops waiting.
    const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);
    /// Action id the "Allow" fix sends.
    const REQUEST_ACCESS: &str = "request-access";
    /// Largest logo stream read, in bytes.
    const MAX_LOGO_BYTES: u64 = 2 * 1024 * 1024;
    /// Apps whose icons are remembered before the cache starts over.
    const MAX_ICONS: usize = 64;

    struct Notifications {
        tx: Sender<Cmd>,
        app: AppHandle,
        epoch: u64,
    }

    impl Runtime for Notifications {
        fn action(&mut self, id: &str) -> Result<(), String> {
            if id != REQUEST_ACCESS {
                return Err(format!("Unknown notifications action {id}"));
            }
            let tx = self.tx.clone();
            // NOTE: RequestAccessAsync must be called on the UI thread, which
            // owns the window the prompt belongs to. Only starting it happens
            // there; the worker waits for the answer, so the UI never blocks.
            self.app
                .run_on_main_thread(move || {
                    let _ = tx.send(Cmd::AccessAsked(ask_access()));
                })
                .map_err(|e| format!("Couldn't show the permission prompt ({e})"))
        }

        fn stop(&mut self) {
            detach(&self.app, self.epoch);
            let _ = self.tx.send(Cmd::Stop);
        }
    }

    /// Starts the notifications worker.
    ///
    /// @param sink Where output and status go.
    pub fn start(sink: Sink) -> Result<Box<dyn Runtime>, String> {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let app = sink.app().clone();
        let epoch = attach(tx.clone());
        let worker_app = app.clone();
        std::thread::Builder::new()
            .name("pill-notifications".into())
            .spawn(move || {
                // SAFETY: balanced by CoUninitialize below on this thread.
                let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                run(&sink, &worker_app, epoch, rx);
                if com {
                    unsafe { CoUninitialize() };
                }
            })
            .map_err(|e| {
                detach(&app, epoch);
                e.to_string()
            })?;
        Ok(Box::new(Notifications { tx, app, epoch }))
    }

    /// Opens Windows' notification access prompt; call on the main thread.
    fn ask_access() -> AccessPoll {
        let op = match UserNotificationListener::Current().and_then(|l| l.RequestAccessAsync()) {
            Ok(op) => op,
            Err(e) => {
                let mut failed = Some(e.to_string());
                return Box::new(move || failed.take().map(Err));
            }
        };
        let asked = Instant::now();
        Box::new(move || {
            // NOTE: the windows crate doesn't export AsyncStatus at this version;
            // its values are 0 started, 1 completed, 2 canceled, 3 error.
            match op.Status().map(|s| s.0) {
                Ok(0) if asked.elapsed() < PROMPT_TIMEOUT => None,
                Ok(0) => {
                    let _ = op.Cancel();
                    Some(Err("the prompt got no answer".into()))
                }
                Ok(_) => Some(op.GetResults().map(|_| ()).map_err(|e| e.to_string())),
                Err(e) => Some(Err(e.to_string())),
            }
        })
    }

    fn prompt_failed(why: &str) -> Status {
        Status::with(
            StatusState::NeedsPermission,
            format!("Windows didn't show the permission prompt ({why}). Allow notification access in privacy settings instead."),
            Some(Fix::Settings { page: SystemPage::Notifications, label: "Open privacy settings".into() }),
        )
    }

    fn denied() -> Status {
        Status::with(
            StatusState::NeedsPermission,
            "Windows is blocking notification access for Pill. Turn on notification access in privacy settings.",
            Some(Fix::Settings { page: SystemPage::Notifications, label: "Open privacy settings".into() }),
        )
    }

    fn not_asked() -> Status {
        Status::with(
            StatusState::NeedsPermission,
            "Pill needs your permission to read notifications.",
            Some(Fix::Action { id: REQUEST_ACCESS.into(), label: "Allow".into() }),
        )
    }

    fn run(sink: &Sink, app: &AppHandle, epoch: u64, rx: mpsc::Receiver<Cmd>) {
        let listener = match UserNotificationListener::Current() {
            Ok(l) => l,
            Err(e) => {
                sink.status(Status::with(
                    StatusState::Unsupported,
                    format!("This version of Windows doesn't let apps read notifications ({e})"),
                    None,
                ));
                return;
            }
        };

        let mut cache = Cache::default();
        let mut last_error: Option<String> = None;
        let mut pending: Option<(Reply, Result<(), String>)> = None;
        let mut asking: Option<AccessPoll> = None;
        let mut prompt_error: Option<String> = None;
        loop {
            match listener.GetAccessStatus() {
                Ok(Access::Allowed) => match read(&listener, &mut cache) {
                    Ok((notes, shots)) => {
                        let (signals, feed) = present(notes);
                        sink.set_signals(signals);
                        publish(app, epoch, Some(feed), shots);
                        sink.status(Status::plain(StatusState::Running));
                        last_error = None;
                    }
                    Err(e) => {
                        let msg = format!("Couldn't read notifications ({e})");
                        if last_error.as_deref() != Some(&msg) {
                            log::warn!("{msg}");
                        }
                        sink.status(Status::with(StatusState::Error, msg.clone(), Some(Fix::Restart)));
                        last_error = Some(msg);
                    }
                },
                Ok(Access::Denied) => {
                    sink.set_signals(Vec::new());
                    publish(app, epoch, None, Vec::new());
                    sink.status(denied());
                }
                Ok(_) => {
                    sink.set_signals(Vec::new());
                    publish(app, epoch, None, Vec::new());
                    sink.status(prompt_error.as_deref().map_or_else(not_asked, prompt_failed));
                }
                Err(e) => {
                    sink.status(Status::with(StatusState::Unsupported, format!("Windows didn't report notification access ({e})"), None));
                }
            }
            // Answered only now, so the refreshed feed reaches the UI before
            // the dismiss resolves.
            if let Some((reply, result)) = pending.take() {
                let _ = reply.send(result);
            }
            if let Some(answer) = asking.as_mut().and_then(|poll| poll()) {
                asking = None;
                prompt_error = answer.err();
                continue;
            }
            match rx.recv_timeout(if asking.is_some() { PROMPT_POLL } else { POLL }) {
                Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
                Ok(Cmd::Dismiss(id, reply)) => {
                    let result = remove(&listener, id).map_err(|e| format!("Windows didn't dismiss the notification ({e})"));
                    pending = Some((reply, result));
                }
                Ok(Cmd::Clear(reply)) => {
                    let result = clear(&listener).map_err(|e| format!("Windows didn't clear notifications ({e})"));
                    pending = Some((reply, result));
                }
                Ok(Cmd::AccessAsked(poll)) => {
                    asking = Some(poll);
                    prompt_error = None;
                }
            }
        }
        if let Some((reply, result)) = pending {
            let _ = reply.send(result);
        }
    }

    /// Icons and screenshot matches kept across polls.
    #[derive(Default)]
    struct Cache {
        /// Icon `data:` URL (or `None` when the app has none) by [`icon_key`].
        icons: HashMap<String, Option<String>>,
        /// Settled screenshot matches by notification id.
        shots: HashMap<u32, Option<PathBuf>>,
    }

    impl Cache {
        fn icon(&mut self, key: String, display: Option<&AppDisplayInfo>) -> Option<String> {
            if let Some(icon) = self.icons.get(&key) {
                return icon.clone();
            }
            if self.icons.len() >= MAX_ICONS {
                self.icons.clear();
            }
            let icon = display.and_then(logo);
            self.icons.insert(key, icon.clone());
            icon
        }

        /// PERF: the Screenshots folder can hold thousands of files, so it's
        /// listed at most once per poll, and only while a recent Snipping
        /// Tool notification is still unmatched.
        fn screenshot(&mut self, id: u32, created_ms: i64, now_ms: i64, images: &mut Option<Vec<(PathBuf, i64)>>) -> Option<PathBuf> {
            if let Some(found) = self.shots.get(&id) {
                return found.clone();
            }
            let images = images.get_or_insert_with(list_screenshots);
            let found = match_screenshot(created_ms, images).map(Path::to_path_buf);
            // NOTE: the file can land a moment after the toast, so a miss
            // only settles once the window has passed.
            if found.is_some() || now_ms - created_ms > SCREENSHOT_WINDOW_MS {
                self.shots.insert(id, found.clone());
            }
            found
        }
    }

    fn read(listener: &UserNotificationListener, cache: &mut Cache) -> windows::core::Result<(Vec<Note>, Shots)> {
        let list = listener.GetNotificationsAsync(NotificationKinds::Toast)?.get()?;
        let generic = KnownNotificationBindings::ToastGeneric()?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut images: Option<Vec<(PathBuf, i64)>> = None;
        let mut notes = Vec::with_capacity(list.Size()? as usize);
        let mut shots = Vec::new();
        for i in 0..list.Size()? {
            let n: UserNotification = list.GetAt(i)?;
            let id = n.Id()?;
            let info = n.AppInfo().ok();
            let display = info.as_ref().and_then(|a| a.DisplayInfo().ok());
            let app = display.as_ref().and_then(|d| d.DisplayName().ok()).map(|h| h.to_string()).unwrap_or_default();
            let aumid = info.as_ref().and_then(|a| a.AppUserModelId().ok()).map(|h| h.to_string()).unwrap_or_default();
            let texts: Vec<String> = n
                .Notification()
                .and_then(|x| x.Visual())
                .and_then(|v| v.GetBinding(&generic))
                .and_then(|b| b.GetTextElements())
                .map(|els| (0..els.Size().unwrap_or(0)).filter_map(|j| els.GetAt(j).and_then(|t| t.Text()).ok()).map(|h| h.to_string()).collect())
                .unwrap_or_default();
            let created_ms = n.CreationTime().map(|t| ticks_to_unix_ms(t.UniversalTime)).unwrap_or(0);

            let icon = cache.icon(icon_key(&aumid, &app), display.as_ref());
            let mut screenshot = None;
            if is_screenshot_app(&aumid, &app) {
                if let Some(path) = cache.screenshot(id, created_ms, now_ms, &mut images) {
                    screenshot = crate::glass_image::thumbnail(&path).ok();
                    if screenshot.is_some() {
                        shots.push((id, path));
                    }
                }
            }
            notes.push(Note {
                id,
                app: if app.trim().is_empty() { "App".into() } else { clean_text(&app, 40) },
                title: clean_text(texts.first().map(String::as_str).unwrap_or_default(), TITLE_CHARS),
                body: clean_text(&texts.get(1..).unwrap_or_default().join(" "), BODY_CHARS),
                created_ms,
                icon,
                screenshot,
            });
        }
        let listed: Vec<u32> = notes.iter().map(|n| n.id).collect();
        cache.shots.retain(|id, _| listed.contains(id));
        Ok((notes, shots))
    }

    fn logo(display: &AppDisplayInfo) -> Option<String> {
        let size = Size { Width: 64.0, Height: 64.0 };
        let stream = display.GetLogo(size).ok()?.OpenReadAsync().and_then(|op| op.get()).ok()?;
        let len = stream.Size().ok()?;
        if len == 0 || len > MAX_LOGO_BYTES {
            return None;
        }
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        let loaded = reader.LoadAsync(len as u32).and_then(|op| op.get()).ok()?;
        let mut bytes = vec![0u8; loaded as usize];
        reader.ReadBytes(&mut bytes).ok()?;
        icon_data_url(&bytes)
    }

    /// Images in the Screenshots folder with their write times, Unix ms.
    fn list_screenshots() -> Vec<(PathBuf, i64)> {
        let Some(dir) = known_folder(&FOLDERID_Screenshots)
            .filter(|d| d.is_dir())
            .or_else(|| known_folder(&FOLDERID_Pictures).map(|p| p.join("Screenshots")).filter(|d| d.is_dir()))
        else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        entries
            .filter_map(Result::ok)
            .filter(|e| crate::glass_image::is_image_path(&e.path()))
            .filter_map(|e| {
                let meta = e.metadata().ok().filter(|m| m.is_file())?;
                let written = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64;
                Some((e.path(), written))
            })
            .collect()
    }

    fn known_folder(id: &GUID) -> Option<PathBuf> {
        // SAFETY: the returned buffer is copied, then freed exactly once.
        unsafe {
            let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
            let path = raw.to_string().ok().map(PathBuf::from);
            CoTaskMemFree(Some(raw.0 as *const _));
            path
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Probes the real listener from this unpackaged test process. The
        /// listener itself must be reachable; access may be allowed, denied
        /// or not yet asked depending on the machine, so only the call
        /// results are checked, never the content. Nothing is dismissed.
        #[test]
        fn listener_is_reachable_without_package_identity() {
            let listener = UserNotificationListener::Current().expect("UserNotificationListener::Current");
            let access = listener.GetAccessStatus().expect("GetAccessStatus");
            if access == Access::Allowed {
                assert!(read(&listener, &mut Cache::default()).is_ok());
            }
        }

        #[test]
        fn finds_the_pictures_folder() {
            assert!(known_folder(&FOLDERID_Pictures).is_some_and(|p| p.is_absolute()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u32, created_ms: i64) -> Note {
        Note { id, app: "Mail".into(), title: format!("Message {id}"), body: "Hi".into(), created_ms, ..Note::default() }
    }

    #[test]
    fn counts_and_lists_newest_first() {
        let (signals, feed) = present((0..7).map(|i| note(i, i64::from(i) * 1000)).collect());
        assert_eq!(signals[0].count, 7);
        assert_eq!(signals[0].kind, Some("notification"));
        assert_eq!(signals[0].label, "7 notifications");
        assert_eq!(feed.total, 7);
        assert_eq!(feed.notes.iter().map(|n| n.id).collect::<Vec<_>>(), vec![6, 5, 4, 3, 2, 1, 0]);
    }

    #[test]
    fn lists_at_most_the_cap_but_counts_everything() {
        let (signals, feed) = present((0..(MAX_LISTED as u32 + 20)).map(|i| note(i, i64::from(i))).collect());
        assert_eq!(feed.notes.len(), MAX_LISTED);
        assert_eq!(feed.total as usize, MAX_LISTED + 20);
        assert_eq!(signals[0].count as usize, MAX_LISTED + 20);
    }

    #[test]
    fn empty_action_center_has_no_signal_and_an_empty_feed() {
        assert_eq!(present(Vec::new()), (Vec::new(), Feed::default()));
    }

    #[test]
    fn feed_serializes_for_the_ui() {
        let (_, feed) = present(vec![Note { icon: Some("data:x".into()), ..note(9, 1_700_000_000_000) }]);
        let v = serde_json::to_value(&feed).unwrap();
        assert_eq!(v["total"], 1);
        assert_eq!(v["notes"][0]["id"], 9);
        assert_eq!(v["notes"][0]["createdMs"], 1_700_000_000_000i64);
        assert_eq!(v["notes"][0]["icon"], "data:x");
        assert!(v["notes"][0]["screenshot"].is_null());
    }

    #[test]
    fn converts_windows_ticks_to_unix_ms() {
        assert_eq!(ticks_to_unix_ms(UNIX_EPOCH_TICKS), 0);
        // 2024-01-01T00:00:00Z
        assert_eq!(ticks_to_unix_ms(133_485_408_000_000_000), 1_704_067_200_000);
        assert_eq!(ticks_to_unix_ms(UNIX_EPOCH_TICKS + 12_345_678), 1_234);
    }

    #[test]
    fn recognizes_snipping_tool() {
        assert!(is_screenshot_app("Microsoft.ScreenSketch_8wekyb3d8bbwe!App", "Snipping Tool"));
        assert!(is_screenshot_app("microsoft.screensketch_8wekyb3d8bbwe!App", "Captura de pantalla"));
        assert!(is_screenshot_app("", "snipping tool"));
        assert!(!is_screenshot_app("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "Terminal"));
        assert!(!is_screenshot_app("", "Snipping Tool helper"));
    }

    #[test]
    fn matches_the_screenshot_closest_to_the_notification() {
        let at = 1_700_000_000_000;
        let files = vec![
            (PathBuf::from("old.png"), at - 5 * 60_000),
            (PathBuf::from("just-before.png"), at - 2_000),
            (PathBuf::from("after.png"), at + 30_000),
        ];
        assert_eq!(match_screenshot(at, &files), Some(Path::new("just-before.png")));
        assert_eq!(match_screenshot(at + 29_000, &files), Some(Path::new("after.png")));
    }

    #[test]
    fn screenshot_window_is_a_minute_either_way() {
        let at = 1_700_000_000_000;
        let edge = vec![(PathBuf::from("edge.png"), at - SCREENSHOT_WINDOW_MS)];
        assert_eq!(match_screenshot(at, &edge), Some(Path::new("edge.png")));
        let late = vec![(PathBuf::from("late.png"), at + SCREENSHOT_WINDOW_MS + 1)];
        assert_eq!(match_screenshot(at, &late), None);
        assert_eq!(match_screenshot(at, &[]), None);
    }

    #[test]
    fn icon_keys_prefer_the_app_id_and_ignore_case() {
        assert_eq!(icon_key("Microsoft.Outlook_8wekyb3d8bbwe!App", "Outlook"), "id:microsoft.outlook_8wekyb3d8bbwe!app");
        assert_eq!(icon_key("MICROSOFT.OUTLOOK_8wekyb3d8bbwe!App", "Mail"), icon_key("Microsoft.Outlook_8wekyb3d8bbwe!App", "Outlook"));
        assert_eq!(icon_key("  ", " Slack "), "name:slack");
        assert_ne!(icon_key("", "Slack"), icon_key("slack", "Slack"));
    }

    #[test]
    fn icons_become_small_transparent_pngs() {
        let encode = |img: image::RgbaImage| {
            let mut png = Vec::new();
            image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
            png
        };
        let decode = |url: &str| {
            let b64 = url.strip_prefix("data:image/png;base64,").expect("PNG data URL");
            image::load_from_memory(&base64::engine::general_purpose::STANDARD.decode(b64).unwrap()).unwrap()
        };
        let wide = decode(&icon_data_url(&encode(image::RgbaImage::from_pixel(200, 100, image::Rgba([10, 120, 220, 128])))).unwrap());
        assert_eq!((wide.width(), wide.height()), (ICON_SIZE, ICON_SIZE / 2));
        assert_eq!(wide.to_rgba8().get_pixel(0, 0).0[3], 128);

        let tiny = decode(&icon_data_url(&encode(image::RgbaImage::from_pixel(16, 16, image::Rgba([1, 2, 3, 255])))).unwrap());
        assert_eq!(tiny.width(), 16);

        assert!(icon_data_url(&encode(image::RgbaImage::new(32, 32))).is_none());
        assert!(icon_data_url(b"not an image").is_none());
    }

    #[test]
    fn padded_logos_are_cropped_to_their_glyph() {
        let mut img = image::RgbaImage::new(64, 64);
        for y in 24..40 {
            for x in 20..44 {
                img.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        let (x, y, w, h) = opaque_bounds(&img).unwrap();
        assert_eq!((w, h), (26, 26));
        assert!(x <= 19 && x + w >= 44 && y <= 23 && y + h >= 40);
        assert_eq!(opaque_bounds(&image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 1, 1, 255]))), None);
        assert_eq!(opaque_bounds(&image::RgbaImage::new(8, 8)), None);
    }
}
