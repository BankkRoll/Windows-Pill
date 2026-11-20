//! Now playing: the Windows media sessions (Spotify, browsers, any player
//! that uses the system media controls).
//!
//! Responsibilities:
//! - A worker thread owns the WinRT session manager, subscribes to its
//!   session events and every session's change events, and republishes on
//!   change.
//! - Picks the session the card controls: the user's pick while that session
//!   exists, otherwise the one Windows calls current.
//! - Publishes the collapsed live item and the Today widget through the
//!   plugin protocol, plus [`NowPlaying`] (timeline, shuffle/repeat, session
//!   list) as [`MEDIA_EVENT`], which the generic widget model can't express.
//! - Decodes artwork once per track, keeping its shape, and app icons once
//!   per app.

use super::protocol::{clean_text, LiveItem, MediaWidget, Widget};
use super::volume::{self, AppVolume};
use super::{Runtime, Sink};
use base64::Engine;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

/// Built-in plugin id.
pub const ID: &str = "now-playing";

/// Event carrying the current [`NowPlaying`], or `null` when nothing plays.
pub const MEDIA_EVENT: &str = "pill://media";

/// Artwork height in pixels; the card shows it at 48 CSS px, so this stays
/// sharp at 200% scaling.
const ART_HEIGHT: u32 = 96;
/// Widest artwork kept, as width over height; wider images are cropped to it.
const ART_MAX_ASPECT: f32 = 16.0 / 9.0;
/// Largest artwork or logo stream read, in bytes.
const MAX_ART_BYTES: u64 = 8 * 1024 * 1024;
/// App icon edge length in pixels (shown at 16 CSS px).
const ICON_SIZE: u32 = 32;
/// Reads of one track's artwork before giving up on it; players often attach
/// artwork a moment after the title changes.
const ART_ATTEMPTS: u8 = 4;
/// Tracks whose artwork stays decoded, so switching sessions back and forth
/// doesn't decode again.
const ART_CACHE: usize = 16;

/// Ticks (100 ns) between 1601-01-01 and the Unix epoch.
const FILETIME_UNIX_OFFSET: i64 = 116_444_736_000_000_000;
const TICKS_PER_MS: i64 = 10_000;

// -----------------------------------------------------------------------------
// Artwork and icons
// -----------------------------------------------------------------------------

/// Decoded track artwork.
#[derive(Debug, Clone, PartialEq)]
pub struct Artwork {
    /// Small `data:image/jpeg` URL.
    pub url: String,
    /// Width over height, between 1 and 16:9, rounded to two decimals.
    pub aspect: f32,
}

/// Turns raw artwork bytes (JPEG or PNG) into a small JPEG that keeps the
/// image's shape: portrait art is cropped square, very wide art to 16:9.
///
/// @param bytes Encoded image.
/// @returns `None` when the image can't be decoded.
pub fn artwork(bytes: &[u8]) -> Option<Artwork> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return None;
    }
    let aspect = (w as f32 / h as f32).clamp(1.0, ART_MAX_ASPECT);
    let (cw, ch) = if w as f32 / h as f32 > aspect { ((h as f32 * aspect).round() as u32, h) } else { (w, (w as f32 / aspect).round() as u32) };
    let (cw, ch) = (cw.clamp(1, w), ch.clamp(1, h));
    let cropped = img.crop_imm((w - cw) / 2, (h - ch) / 2, cw, ch);
    let out_h = ART_HEIGHT.min(ch);
    let out_w = ((out_h as f32 * aspect).round() as u32).max(1);
    let small = cropped.resize_exact(out_w, out_h, image::imageops::FilterType::Triangle).to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85).encode_image(&small).ok()?;
    Some(Artwork {
        url: format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&jpeg)),
        aspect: (aspect * 100.0).round() / 100.0,
    })
}

/// Shrinks an app logo or icon to a small square PNG data URL, keeping
/// transparency.
///
/// @param img Decoded logo.
pub fn icon_data_url(img: &image::DynamicImage) -> Option<String> {
    if img.width() == 0 || img.height() == 0 {
        return None;
    }
    let small = img.resize(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Triangle).to_rgba8();
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(small).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png)))
}

/// A readable player name from a session's app user model id.
///
/// @param aumid e.g. `Spotify.exe`, `Chrome` or `SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify`.
/// @example app_label("msedge.exe") // "Edge"
pub fn app_label(aumid: &str) -> String {
    let last = aumid_stem(aumid);
    let known = match last.to_ascii_lowercase().as_str() {
        "msedge" | "microsoftedge" => Some("Edge"),
        "chrome" => Some("Chrome"),
        "firefox" | "308046b0af4a39cb" => Some("Firefox"),
        "zunemusic" => Some("Media Player"),
        "brave" => Some("Brave"),
        "opera" => Some("Opera"),
        "vlc" => Some("VLC"),
        "" => Some("Media"),
        _ => None,
    };
    if let Some(k) = known {
        return k.to_string();
    }
    let mut chars = last.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The last meaningful segment of an app user model id, without `.exe`.
fn aumid_stem(aumid: &str) -> &str {
    let tail = aumid.rsplit('!').next().unwrap_or(aumid);
    let tail = tail.rsplit(['\\', '/']).next().unwrap_or(tail);
    let tail = tail.strip_suffix(".exe").or_else(|| tail.strip_suffix(".EXE")).unwrap_or(tail);
    tail.rsplit('.').next().unwrap_or(tail)
}

/// Executable names that may own a desktop (unpackaged) app's session, most
/// likely first, lowercased. Store apps (`Package!App`) have none.
///
/// @param aumid Session's app user model id.
/// @example exe_candidates("MSEdge") // ["msedge.exe", "edge.exe"]
pub fn exe_candidates(aumid: &str) -> Vec<String> {
    if aumid.contains('!') {
        return Vec::new();
    }
    let tail = aumid.rsplit(['\\', '/']).next().unwrap_or(aumid).to_ascii_lowercase();
    if tail.ends_with(".exe") {
        return vec![tail];
    }
    let mut out = Vec::new();
    let stem = aumid_stem(aumid).to_ascii_lowercase();
    if !stem.is_empty() {
        out.push(format!("{stem}.exe"));
    }
    let label = format!("{}.exe", app_label(aumid).to_ascii_lowercase().replace(' ', ""));
    if !out.contains(&label) {
        out.push(label);
    }
    out
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

struct ArtEntry {
    key: String,
    art: Option<Artwork>,
    tries: u8,
}

/// Artwork per track, most recently used last.
///
/// NOTE: Spotify and browsers often update the title before the thumbnail,
/// so the first read for a new track can return the previous track's image.
/// The cache remembers each session's last image and treats an identical
/// image under a different (or no) album as not ready yet, until the retries
/// run out, since back-to-back videos can legitimately share a thumbnail.
#[derive(Default)]
pub struct ArtCache {
    entries: Vec<ArtEntry>,
    /// Session id → (album, image hash) of the artwork last accepted.
    last: HashMap<u32, (String, u64)>,
}

impl ArtCache {
    /// Artwork for a track, reading it only while it isn't cached yet.
    ///
    /// @param session Session id the track plays in.
    /// @param key Track identity, see [`Track::key`].
    /// @param album Track album, for spotting a stale image.
    /// @param read Reads the thumbnail's raw bytes.
    pub fn get(&mut self, session: u32, key: &str, album: &str, read: impl FnOnce() -> Option<Vec<u8>>) -> Option<Artwork> {
        let mut entry = match self.entries.iter().position(|e| e.key == key) {
            Some(i) => self.entries.remove(i),
            None => ArtEntry { key: key.to_string(), art: None, tries: 0 },
        };
        if entry.art.is_none() && entry.tries < ART_ATTEMPTS {
            entry.tries += 1;
            if let Some(bytes) = read() {
                let hash = hash_bytes(&bytes);
                let stale = self.last.get(&session).is_some_and(|(a, h)| *h == hash && (a != album || album.is_empty()));
                if !stale || entry.tries >= ART_ATTEMPTS {
                    entry.art = artwork(&bytes);
                    if entry.art.is_some() {
                        self.last.insert(session, (album.to_string(), hash));
                    }
                }
            }
        }
        let art = entry.art.clone();
        self.entries.push(entry);
        if self.entries.len() > ART_CACHE {
            self.entries.remove(0);
        }
        art
    }

    /// Whether a track's artwork is still worth reading again.
    ///
    /// @param key Track identity.
    pub fn pending(&self, key: &str) -> bool {
        self.entries.iter().find(|e| e.key == key).is_none_or(|e| e.art.is_none() && e.tries < ART_ATTEMPTS)
    }

    /// Forgets a session that ended.
    pub fn forget_session(&mut self, session: u32) {
        self.last.remove(&session);
    }
}

// -----------------------------------------------------------------------------
// Timeline, modes and session choice
// -----------------------------------------------------------------------------

/// A session's timeline as Windows reports it, in 100 ns ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RawTimeline {
    pub start: i64,
    pub end: i64,
    pub position: i64,
    pub min_seek: i64,
    pub max_seek: i64,
    /// When `position` was sampled, as a FILETIME.
    pub updated: i64,
}

/// Playback position anchored in time, so the UI can advance it locally.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    /// Position at `updated_at`.
    pub position_ms: u64,
    pub duration_ms: u64,
    /// Unix time in milliseconds at which `position_ms` was true.
    pub updated_at: i64,
    /// Playback speed; 1 is normal.
    pub rate: f64,
}

/// Anchors a raw timeline for the UI.
///
/// Windows reports the position as of `updated`. Some players leave that
/// timestamp empty, or stop refreshing it while playing so that advancing
/// from it would run far past the end; then the position is taken as current.
///
/// @param raw Timeline from the session.
/// @param rate Playback rate (players may omit it; anything not positive means 1).
/// @param playing Whether the session is playing.
/// @param now_ms Current Unix time in milliseconds.
/// @returns `None` when the session has no usable timeline.
pub fn anchor(raw: &RawTimeline, rate: f64, playing: bool, now_ms: i64) -> Option<Timeline> {
    let duration = (raw.end - raw.start) / TICKS_PER_MS;
    if duration <= 0 {
        return None;
    }
    let position = ((raw.position - raw.start) / TICKS_PER_MS).clamp(0, duration);
    let rate = if rate.is_finite() && rate > 0.0 { rate } else { 1.0 };
    let reported = (raw.updated - FILETIME_UNIX_OFFSET) / TICKS_PER_MS;
    let plausible = raw.updated > FILETIME_UNIX_OFFSET && reported <= now_ms + 2_000;
    let overshoots = playing && position as f64 + (now_ms - reported) as f64 * rate > duration as f64 + 5_000.0;
    let updated_at = if plausible && !overshoots { reported } else { now_ms };
    Some(Timeline { position_ms: position as u64, duration_ms: duration as u64, updated_at, rate })
}

/// The position to request for a seek, in the session's ticks.
///
/// @param raw Timeline from the session.
/// @param ms Wanted position from the start of the track.
pub fn seek_target(raw: &RawTimeline, ms: u64) -> i64 {
    let offset = i64::try_from(ms).unwrap_or(i64::MAX).saturating_mul(TICKS_PER_MS);
    let wanted = raw.start.saturating_add(offset);
    let lo = raw.start.max(raw.min_seek);
    let hi = if raw.max_seek > lo { raw.max_seek.min(raw.end) } else { raw.end };
    wanted.clamp(lo, hi.max(lo))
}

/// A session's repeat mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    None,
    List,
    Track,
}

impl Repeat {
    /// The mode the repeat button switches to: off, all, one, off.
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::List,
            Self::List => Self::Track,
            Self::Track => Self::None,
        }
    }
}

/// Which session the card shows.
///
/// @param sessions Playing or paused sessions as `(id, playing)`, in Windows' order.
/// @param picked The user's pick, if any.
/// @param current The session Windows calls current.
/// @returns The pick when it's listed, else Windows' current one, else the
/// first playing one, else the first; `None` when nothing is listed.
pub fn choose(sessions: &[(u32, bool)], picked: Option<u32>, current: Option<u32>) -> Option<u32> {
    let listed = |id: Option<u32>| id.filter(|i| sessions.iter().any(|(s, _)| s == i));
    listed(picked)
        .or_else(|| listed(current))
        .or_else(|| sessions.iter().find(|(_, p)| *p).map(|(s, _)| *s))
        .or_else(|| sessions.first().map(|(s, _)| *s))
}

/// Gives each found session a stable id. A session keeps its id while
/// Windows hands out the same object; failing that (some players re-create
/// their session object) a lone session from the same app inherits the id.
///
/// @param known Tracked sessions as `(id, object, app id)`.
/// @param found Sessions Windows lists now as `(object, app id)`.
/// @param next Last id handed out; advanced for every new session.
/// @returns One id per found session, in order.
pub fn assign_ids<S: PartialEq>(known: &[(u32, S, String)], found: &[(S, String)], next: &mut u32) -> Vec<u32> {
    let mut taken = vec![false; known.len()];
    let mut ids: Vec<Option<u32>> = found
        .iter()
        .map(|(obj, _)| {
            let i = (0..known.len()).find(|&i| !taken[i] && known[i].1 == *obj)?;
            taken[i] = true;
            Some(known[i].0)
        })
        .collect();
    for (slot, (_, app)) in ids.iter_mut().zip(found) {
        if slot.is_some() {
            continue;
        }
        let same_app: Vec<usize> = (0..known.len()).filter(|&i| !taken[i] && known[i].2 == *app).collect();
        let unmatched_same_app = found.iter().filter(|(_, a)| a == app).count();
        if let [i] = same_app[..] {
            if unmatched_same_app == 1 {
                taken[i] = true;
                *slot = Some(known[i].0);
            }
        }
    }
    ids.into_iter()
        .map(|id| {
            id.unwrap_or_else(|| {
                *next += 1;
                *next
            })
        })
        .collect()
}

// -----------------------------------------------------------------------------
// Output
// -----------------------------------------------------------------------------

/// What one refresh read from a session.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    pub app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// Playing or paused, as opposed to stopped or closed.
    pub active: bool,
    pub can_play_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
    /// `None` when the player can't shuffle.
    pub shuffle: Option<bool>,
    /// `None` when the player can't repeat.
    pub repeat: Option<Repeat>,
    /// `None` when the player has no timeline or doesn't allow seeking.
    pub timeline: Option<RawTimeline>,
    pub rate: f64,
}

impl Track {
    /// Identifies the track for the artwork cache.
    pub fn key(&self) -> String {
        format!("{}\u{1f}{}\u{1f}{}\u{1f}{}", self.app, self.title, self.artist, self.album)
    }

    fn shows(&self) -> bool {
        self.active && !self.title.is_empty()
    }
}

/// Output for a track: the collapsed live item and the Today widget. Both
/// disappear when nothing is playing or paused.
///
/// @param track Session the card shows, or `None` when there's none.
/// @param artwork Cached artwork URL for this track.
pub fn present(track: Option<&Track>, artwork: Option<String>) -> (Option<LiveItem>, Option<Widget>) {
    let Some(t) = track.filter(|t| t.shows()) else {
        return (None, None);
    };
    let live = LiveItem::Media { artwork: artwork.clone(), playing: t.playing, title: t.title.clone() };
    let widget = Widget::Media(MediaWidget {
        app: t.app.clone(),
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        artwork,
        playing: t.playing,
        can_play_pause: t.can_play_pause,
        can_next: t.can_next,
        can_previous: t.can_previous,
    });
    (Some(live), Some(widget))
}

/// One player in the session switcher.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSession {
    pub id: u32,
    /// Player name, e.g. "Spotify".
    pub app: String,
    /// Small `data:image/png` URL of the app's icon, when Windows has one.
    pub icon: Option<String>,
    pub title: String,
    /// Artist, or the channel for a browser tab; empty when unknown.
    pub artist: String,
    pub playing: bool,
}

/// What the Now playing card needs beyond the widget: the timeline, the
/// shuffle and repeat modes, and the sessions it can switch between.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    /// Session the card shows and controls.
    pub session: u32,
    /// Every playing or paused session, in Windows' order.
    pub sessions: Vec<MediaSession>,
    /// Artwork width over height; 1 when there's none.
    pub artwork_aspect: f32,
    /// `None` hides the seek bar.
    pub timeline: Option<Timeline>,
    /// `None` when the player can't shuffle.
    pub shuffle: Option<bool>,
    /// `None` when the player can't repeat.
    pub repeat: Option<Repeat>,
    /// The player's own mixer volume; `None` while it has no audio session.
    pub volume: Option<AppVolume>,
}

/// One session as read during a refresh.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRead {
    pub id: u32,
    pub icon: Option<String>,
    pub track: Track,
}

/// Builds [`NowPlaying`] for the chosen session.
///
/// @param sessions Every session read this refresh.
/// @param chosen Id from [`choose`].
/// @param aspect Artwork aspect of the chosen track, if it has artwork.
/// @param timeline Anchored timeline of the chosen track.
/// @returns `None` when the chosen session shows nothing.
pub fn detail(
    sessions: &[SessionRead],
    chosen: Option<u32>,
    aspect: Option<f32>,
    timeline: Option<Timeline>,
    volume: Option<AppVolume>,
) -> Option<NowPlaying> {
    let shown = sessions.iter().find(|s| Some(s.id) == chosen).filter(|s| s.track.shows())?;
    Some(NowPlaying {
        session: shown.id,
        sessions: sessions
            .iter()
            .filter(|s| s.track.shows())
            .map(|s| MediaSession {
                id: s.id,
                app: s.track.app.clone(),
                icon: s.icon.clone(),
                title: s.track.title.clone(),
                artist: s.track.artist.clone(),
                playing: s.track.playing,
            })
            .collect(),
        artwork_aspect: aspect.unwrap_or(1.0),
        timeline,
        shuffle: shown.track.shuffle,
        repeat: shown.track.repeat,
        volume,
    })
}

/// Latest [`NowPlaying`] with the runtime generation that published it, for
/// windows that open after it was emitted.
static LATEST: Mutex<Option<(u64, NowPlaying)>> = Mutex::new(None);

/// The current [`NowPlaying`], or `None` when nothing plays or the plugin is off.
pub fn latest() -> Option<NowPlaying> {
    LATEST.lock().unwrap().as_ref().map(|(_, n)| n.clone())
}

/// Emits `now` and remembers it, unless a newer runtime has taken over.
#[cfg_attr(not(windows), allow(dead_code))]
fn publish_detail(sink: &Sink, now: Option<NowPlaying>) {
    if !sink.emit(MEDIA_EVENT, now.clone()) {
        return;
    }
    let mut latest = LATEST.lock().unwrap();
    if latest.as_ref().is_none_or(|(g, _)| *g <= sink.generation) {
        *latest = now.map(|n| (sink.generation, n));
    }
}

/// Forgets the published state once the runtime that published it stops.
#[cfg_attr(not(windows), allow(dead_code))]
fn clear_detail(generation: u64) {
    let mut latest = LATEST.lock().unwrap();
    if latest.as_ref().is_some_and(|(g, _)| *g == generation) {
        *latest = None;
    }
}

// -----------------------------------------------------------------------------
// Actions
// -----------------------------------------------------------------------------

/// What the card can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    PlayPause,
    Next,
    Previous,
    /// Turn shuffle on or off, whichever it isn't.
    Shuffle,
    /// Switch to the next repeat mode, see [`Repeat::next`].
    Repeat,
    /// Jump to this many milliseconds from the start of the track.
    Seek(u64),
    /// Show and control this session.
    Session(u32),
    /// Set the player's mixer volume, in percent.
    Volume(u8),
    /// Mute the player, or unmute it.
    Mute,
}

impl Control {
    /// Parses an action id: `play-pause`, `next`, `previous`, `shuffle`,
    /// `repeat`, `mute`, `seek:<ms>`, `volume:<0-100>` or `session:<id>`.
    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "play-pause" => Some(Self::PlayPause),
            "next" => Some(Self::Next),
            "previous" => Some(Self::Previous),
            "shuffle" => Some(Self::Shuffle),
            "repeat" => Some(Self::Repeat),
            "mute" => Some(Self::Mute),
            _ => match id.split_once(':')? {
                ("seek", ms) => ms.parse().ok().map(Self::Seek),
                ("session", n) => n.parse().ok().map(Self::Session),
                ("volume", n) => n.parse::<u8>().ok().filter(|v| *v <= 100).map(Self::Volume),
                _ => None,
            },
        }
    }
}

#[cfg(windows)]
pub use imp::start;

/// Non-Windows builds have no system media sessions.
#[cfg(not(windows))]
pub fn start(_sink: Sink) -> Result<Box<dyn Runtime>, String> {
    Err("Now playing needs Windows".into())
}

// -----------------------------------------------------------------------------
// Windows
// -----------------------------------------------------------------------------

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::plugins::{Status, StatusState};
    use std::sync::mpsc::{self, RecvTimeoutError, Sender};
    use std::time::{Duration, Instant};
    use windows::core::{Interface, HSTRING};
    use windows::ApplicationModel::AppInfo;
    use windows::Foundation::{Size, TypedEventHandler};
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackControls as Controls,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
    };
    use windows::Media::MediaPlaybackAutoRepeatMode as AutoRepeat;
    use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    // NOTE: Session events are reliable for Spotify and the Windows players,
    // but Chromium-based browsers sometimes skip PlaybackInfoChanged when a
    // tab pauses itself. A slow poll catches those; each tick is a few cheap
    // cross-process calls and only republishes on change.
    const FALLBACK_POLL: Duration = Duration::from_secs(3);
    /// How soon to look again while a track's artwork hasn't arrived.
    const ART_RETRY: Duration = Duration::from_millis(600);
    /// Events arrive in bursts (title, then artwork, then state); wait this
    /// long for a burst to settle before reading.
    const SETTLE: Duration = Duration::from_millis(60);
    /// Longest a steady stream of events may hold a refresh back.
    const MAX_SETTLE: Duration = Duration::from_millis(250);
    /// Longest one call into a player's session may take before it is
    /// cancelled.
    const CALL_TIMEOUT: Duration = Duration::from_secs(2);
    /// Longest Windows may take to hand over the session manager.
    const MANAGER_TIMEOUT: Duration = Duration::from_secs(10);

    /// Waits for a WinRT async operation, cancelling it after `$timeout`.
    ///
    /// NOTE: a hung player (a frozen browser tab) may never complete a
    /// session call, and `.get()` would then block this worker, and Now
    /// playing with it, for good. The windows crate doesn't export the async
    /// operation types at this version, so this can't be a generic function
    /// that registers a completion handler; it polls the status instead.
    macro_rules! wait {
        ($op:expr, $timeout:expr) => {
            $op.and_then(|op| {
                if let Err(e) = wait_until_done(|| op.Status().map(|s| s.0), $timeout) {
                    let _ = op.Cancel();
                    return Err(e);
                }
                op.GetResults()
            })
        };
    }

    /// Polls `status` (an `AsyncStatus` value) until it leaves `Started`.
    ///
    /// @returns An `ERROR_TIMEOUT` error once `timeout` passes first.
    fn wait_until_done(status: impl Fn() -> windows::core::Result<i32>, timeout: Duration) -> windows::core::Result<()> {
        const STARTED: i32 = 0;
        let deadline = Instant::now() + timeout;
        let mut pause = Duration::from_millis(1);
        while status()? == STARTED {
            if Instant::now() >= deadline {
                return Err(windows::core::Error::new(windows::Win32::Foundation::ERROR_TIMEOUT.to_hresult(), "the player didn't answer in time"));
            }
            std::thread::sleep(pause);
            pause = (pause * 2).min(Duration::from_millis(20));
        }
        Ok(())
    }

    enum Cmd {
        Refresh,
        Control(Control),
        Stop,
    }

    struct Media {
        tx: Sender<Cmd>,
    }

    impl Runtime for Media {
        fn action(&mut self, id: &str) -> Result<(), String> {
            let c = Control::parse(id).ok_or_else(|| format!("Unknown media action {id}"))?;
            self.tx.send(Cmd::Control(c)).map_err(|_| "Now playing has stopped".to_string())
        }

        fn stop(&mut self) {
            let _ = self.tx.send(Cmd::Stop);
        }
    }

    /// Starts the media worker.
    ///
    /// @param sink Where output and status go.
    pub fn start(sink: Sink) -> Result<Box<dyn Runtime>, String> {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let events = tx.clone();
        std::thread::Builder::new()
            .name("pill-media".into())
            .spawn(move || {
                // SAFETY: balanced by CoUninitialize below on this thread.
                let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                run(&sink, events, rx);
                clear_detail(sink.generation);
                if com {
                    unsafe { CoUninitialize() };
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Box::new(Media { tx }))
    }

    fn notify<S: windows::core::RuntimeType + 'static, A: windows::core::RuntimeType + 'static>(tx: &Sender<Cmd>) -> TypedEventHandler<S, A> {
        let tx = tx.clone();
        TypedEventHandler::new(move |_, _| {
            let _ = tx.send(Cmd::Refresh);
            Ok(())
        })
    }

    /// A session with our event subscriptions on it.
    struct Tracked {
        id: u32,
        session: Session,
        aumid: String,
        tokens: [Option<i64>; 3],
    }

    impl Tracked {
        fn new(id: u32, session: Session, aumid: String, events: &Sender<Cmd>) -> Self {
            let tokens = [
                session.MediaPropertiesChanged(&notify(events)).ok(),
                session.PlaybackInfoChanged(&notify(events)).ok(),
                session.TimelinePropertiesChanged(&notify(events)).ok(),
            ];
            Self { id, session, aumid, tokens }
        }

        fn release(self) {
            if let Some(t) = self.tokens[0] {
                let _ = self.session.RemoveMediaPropertiesChanged(t);
            }
            if let Some(t) = self.tokens[1] {
                let _ = self.session.RemovePlaybackInfoChanged(t);
            }
            if let Some(t) = self.tokens[2] {
                let _ = self.session.RemoveTimelinePropertiesChanged(t);
            }
        }
    }

    /// An app's display name and icon.
    #[derive(Clone)]
    struct Identity {
        name: String,
        icon: Option<String>,
    }

    struct Worker<'a> {
        sink: &'a Sink,
        manager: Manager,
        events: Sender<Cmd>,
        tracked: Vec<Tracked>,
        next_id: u32,
        picked: Option<u32>,
        shown: Option<u32>,
        art: ArtCache,
        /// Per app user model id.
        apps: HashMap<String, Identity>,
        /// Last raw timeline (with session and playing state) and its anchor,
        /// so an unchanged timeline keeps its anchor and doesn't republish.
        timeline: Option<(u32, RawTimeline, bool, Timeline)>,
        /// Whether the shown track's artwork may still arrive.
        art_pending: bool,
        sent: Option<Option<NowPlaying>>,
    }

    fn run(sink: &Sink, events: Sender<Cmd>, rx: mpsc::Receiver<Cmd>) {
        let manager = match wait!(Manager::RequestAsync(), MANAGER_TIMEOUT) {
            Ok(m) => m,
            Err(e) => {
                sink.status(Status::with(StatusState::Unsupported, format!("Windows media controls aren't available ({e})"), None));
                return;
            }
        };
        let manager_tokens = (manager.CurrentSessionChanged(&notify(&events)).ok(), manager.SessionsChanged(&notify(&events)).ok());
        sink.status(Status::plain(StatusState::Running));

        let mut w = Worker {
            sink,
            manager,
            events,
            tracked: Vec::new(),
            next_id: 0,
            picked: None,
            shown: None,
            art: ArtCache::default(),
            apps: HashMap::new(),
            timeline: None,
            art_pending: false,
            sent: None,
        };
        w.publish();
        loop {
            let wait = if w.art_pending { ART_RETRY } else { FALLBACK_POLL };
            let mut cmds = match rx.recv_timeout(wait) {
                Ok(c) => vec![c],
                Err(RecvTimeoutError::Timeout) => vec![Cmd::Refresh],
                Err(RecvTimeoutError::Disconnected) => break,
            };
            if matches!(cmds[0], Cmd::Refresh) {
                let deadline = Instant::now() + MAX_SETTLE;
                while let Ok(c) = rx.recv_timeout(SETTLE.min(deadline.saturating_duration_since(Instant::now()))) {
                    cmds.push(c);
                }
            }
            if cmds.iter().any(|c| matches!(c, Cmd::Stop)) {
                break;
            }
            for c in cmds {
                if let Cmd::Control(c) = c {
                    w.control(c);
                }
            }
            w.publish();
        }
        for t in w.tracked.drain(..) {
            t.release();
        }
        if let Some(t) = manager_tokens.0 {
            let _ = w.manager.RemoveCurrentSessionChanged(t);
        }
        if let Some(t) = manager_tokens.1 {
            let _ = w.manager.RemoveSessionsChanged(t);
        }
    }

    impl Worker<'_> {
        /// Matches Windows' session list against the tracked one, subscribing
        /// to new sessions and releasing gone ones.
        fn reconcile(&mut self) {
            let found: Vec<(Session, String)> = match self.manager.GetSessions() {
                Ok(list) => list
                    .into_iter()
                    .map(|s| {
                        let aumid = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
                        (s, aumid)
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            let known: Vec<(u32, Session, String)> = self.tracked.iter().map(|t| (t.id, t.session.clone(), t.aumid.clone())).collect();
            let ids = assign_ids(&known, &found, &mut self.next_id);
            let mut old = std::mem::take(&mut self.tracked);
            for (id, (session, aumid)) in ids.into_iter().zip(found) {
                match old.iter().position(|t| t.id == id && t.session == session) {
                    Some(i) => self.tracked.push(old.remove(i)),
                    None => self.tracked.push(Tracked::new(id, session, aumid, &self.events)),
                }
            }
            for gone in old {
                if !self.tracked.iter().any(|t| t.id == gone.id) {
                    self.art.forget_session(gone.id);
                }
                gone.release();
            }
            if self.picked.is_some_and(|p| !self.tracked.iter().any(|t| t.id == p)) {
                self.picked = None;
            }
        }

        fn identity(&mut self, aumid: &str) -> Identity {
            if let Some(found) = self.apps.get(aumid) {
                return found.clone();
            }
            let found = packaged_identity(aumid).unwrap_or_else(|| Identity { name: app_label(aumid), icon: desktop_icon(aumid) });
            self.apps.insert(aumid.to_string(), found.clone());
            found
        }

        fn current(&self) -> Option<u32> {
            let c = self.manager.GetCurrentSession().ok()?;
            let by_object = self.tracked.iter().find(|t| t.session == c);
            let found = by_object.or_else(|| {
                let aumid = c.SourceAppUserModelId().ok()?.to_string();
                self.tracked.iter().find(|t| t.aumid == aumid)
            });
            found.map(|t| t.id)
        }

        fn publish(&mut self) {
            self.reconcile();
            let current = self.current();
            let mut reads = Vec::with_capacity(self.tracked.len());
            for i in 0..self.tracked.len() {
                let aumid = self.tracked[i].aumid.clone();
                let who = self.identity(&aumid);
                // PERF: every session is read on each refresh so the switcher
                // can list titles; there are rarely more than three.
                if let Some(track) = read_track(&self.tracked[i].session, who.name) {
                    reads.push(SessionRead { id: self.tracked[i].id, icon: who.icon, track });
                }
            }
            let listed: Vec<(u32, bool)> = reads.iter().filter(|r| r.track.shows()).map(|r| (r.id, r.track.playing)).collect();
            let chosen = choose(&listed, self.picked, current);
            self.shown = chosen;
            let shown = reads.iter().find(|r| Some(r.id) == chosen);

            self.art_pending = false;
            let art = shown.and_then(|r| {
                let session = self.tracked.iter().find(|t| t.id == r.id)?.session.clone();
                let key = r.track.key();
                let art = self.art.get(r.id, &key, &r.track.album, || read_thumbnail(&session));
                self.art_pending = art.is_none() && self.art.pending(&key);
                art
            });
            let timeline = shown.and_then(|r| {
                let raw = r.track.timeline?;
                if let Some((id, prev, playing, anchored)) = self.timeline {
                    if id == r.id && prev == raw && playing == r.track.playing {
                        return Some(anchored);
                    }
                }
                let anchored = anchor(&raw, r.track.rate, r.track.playing, chrono::Utc::now().timestamp_millis())?;
                self.timeline = Some((r.id, raw, r.track.playing, anchored));
                Some(anchored)
            });

            let (live, widget) = present(shown.map(|r| &r.track), art.as_ref().map(|a| a.url.clone()));
            self.sink.set_live(live);
            self.sink.set_widget(widget);
            let volume = shown.and_then(|r| self.tracked.iter().find(|t| t.id == r.id)).and_then(|t| volume::read(&exe_candidates(&t.aumid)));
            let next = detail(&reads, chosen, art.map(|a| a.aspect), timeline, volume);
            if self.sent.as_ref() != Some(&next) {
                publish_detail(self.sink, next.clone());
                self.sent = Some(next);
            }
        }

        fn control(&mut self, c: Control) {
            if let Control::Session(id) = c {
                if self.tracked.iter().any(|t| t.id == id) {
                    self.picked = Some(id);
                }
                return;
            }
            let Some((s, aumid)) = self.tracked.iter().find(|t| Some(t.id) == self.shown).map(|t| (t.session.clone(), t.aumid.clone())) else {
                return;
            };
            if let Control::Volume(_) | Control::Mute = c {
                let names = exe_candidates(&aumid);
                let result = match c {
                    Control::Volume(percent) => volume::set_level(&names, f32::from(percent) / 100.0),
                    _ => volume::set_muted(&names, !volume::read(&names).is_some_and(|v| v.muted)),
                };
                if let Err(e) = result {
                    log::warn!("media control {c:?}: {e}");
                }
                return;
            }
            let info = s.GetPlaybackInfo().ok();
            let op = match c {
                Control::PlayPause => s.TryTogglePlayPauseAsync(),
                Control::Next => s.TrySkipNextAsync(),
                Control::Previous => s.TrySkipPreviousAsync(),
                Control::Shuffle => {
                    let on = info.as_ref().and_then(|i| i.IsShuffleActive().ok()).and_then(|r| r.Value().ok()).unwrap_or(false);
                    s.TryChangeShuffleActiveAsync(!on)
                }
                Control::Repeat => {
                    let mode = info.as_ref().and_then(|i| i.AutoRepeatMode().ok()).and_then(|r| r.Value().ok()).map(from_auto_repeat);
                    s.TryChangeAutoRepeatModeAsync(to_auto_repeat(mode.unwrap_or(Repeat::None).next()))
                }
                Control::Seek(ms) => match read_timeline(&s) {
                    Some(raw) => s.TryChangePlaybackPositionAsync(seek_target(&raw, ms)),
                    None => return,
                },
                Control::Session(_) | Control::Volume(_) | Control::Mute => return,
            };
            match wait!(op, CALL_TIMEOUT) {
                Ok(true) => {}
                Ok(false) => log::info!("media control {c:?}: the player declined"),
                Err(e) => log::warn!("media control {c:?}: {e}"),
            }
        }
    }

    fn from_auto_repeat(m: AutoRepeat) -> Repeat {
        match m {
            AutoRepeat::Track => Repeat::Track,
            AutoRepeat::List => Repeat::List,
            _ => Repeat::None,
        }
    }

    fn to_auto_repeat(r: Repeat) -> AutoRepeat {
        match r {
            Repeat::None => AutoRepeat::None,
            Repeat::List => AutoRepeat::List,
            Repeat::Track => AutoRepeat::Track,
        }
    }

    fn read_timeline(s: &Session) -> Option<RawTimeline> {
        let t = s.GetTimelineProperties().ok()?;
        Some(RawTimeline {
            start: t.StartTime().ok()?.Duration,
            end: t.EndTime().ok()?.Duration,
            position: t.Position().ok()?.Duration,
            min_seek: t.MinSeekTime().map(|d| d.Duration).unwrap_or(0),
            max_seek: t.MaxSeekTime().map(|d| d.Duration).unwrap_or(0),
            updated: t.LastUpdatedTime().map(|d| d.UniversalTime).unwrap_or(0),
        })
    }

    fn read_track(s: &Session, app: String) -> Option<Track> {
        let info = s.GetPlaybackInfo().ok()?;
        let status = info.PlaybackStatus().ok()?;
        let controls = info.Controls().ok();
        let props = wait!(s.TryGetMediaPropertiesAsync(), CALL_TIMEOUT).ok()?;
        let text = |r: windows::core::Result<HSTRING>| r.map(|h| h.to_string()).unwrap_or_default();
        let flag = |f: fn(&Controls) -> windows::core::Result<bool>| controls.as_ref().and_then(|c| f(c).ok()).unwrap_or(false);
        let artist = text(props.Artist());
        let artist = if artist.is_empty() { text(props.AlbumArtist()) } else { artist };
        let shuffle = flag(|c| c.IsShuffleEnabled()).then(|| info.IsShuffleActive().ok().and_then(|r| r.Value().ok()).unwrap_or(false));
        let repeat = flag(|c| c.IsRepeatEnabled())
            .then(|| info.AutoRepeatMode().ok().and_then(|r| r.Value().ok()).map(from_auto_repeat).unwrap_or(Repeat::None));
        let timeline = if flag(|c| c.IsPlaybackPositionEnabled()) { read_timeline(s).filter(|t| t.end > t.start) } else { None };
        Some(Track {
            app,
            title: clean_text(&text(props.Title()), 200),
            artist: clean_text(&artist, 200),
            album: clean_text(&text(props.AlbumTitle()), 200),
            playing: status == PlaybackStatus::Playing,
            active: status == PlaybackStatus::Playing || status == PlaybackStatus::Paused,
            can_play_pause: flag(|c| c.IsPlayPauseToggleEnabled()) || flag(|c| c.IsPlayEnabled()) || flag(|c| c.IsPauseEnabled()),
            can_next: flag(|c| c.IsNextEnabled()),
            can_previous: flag(|c| c.IsPreviousEnabled()),
            shuffle,
            repeat,
            timeline,
            rate: info.PlaybackRate().ok().and_then(|r| r.Value().ok()).unwrap_or(1.0),
        })
    }

    fn read_stream(reference: &IRandomAccessStreamReference) -> Option<Vec<u8>> {
        let stream = wait!(reference.OpenReadAsync(), CALL_TIMEOUT).ok()?;
        let size = stream.Size().ok()?;
        if size == 0 || size > MAX_ART_BYTES {
            return None;
        }
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        let loaded = wait!(reader.LoadAsync(size as u32), CALL_TIMEOUT).ok()?;
        let mut bytes = vec![0u8; loaded as usize];
        reader.ReadBytes(&mut bytes).ok()?;
        Some(bytes)
    }

    fn read_thumbnail(s: &Session) -> Option<Vec<u8>> {
        let props = wait!(s.TryGetMediaPropertiesAsync(), CALL_TIMEOUT).ok()?;
        read_stream(&props.Thumbnail().ok()?)
    }

    /// Name and logo of a Store (packaged) app.
    fn packaged_identity(aumid: &str) -> Option<Identity> {
        if !aumid.contains('!') {
            return None;
        }
        let display = AppInfo::GetFromAppUserModelId(&HSTRING::from(aumid)).and_then(|a| a.DisplayInfo()).ok()?;
        let name = display.DisplayName().map(|h| h.to_string()).ok().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| app_label(aumid));
        let size = Size { Width: ICON_SIZE as f32, Height: ICON_SIZE as f32 };
        let icon = display
            .GetLogo(size)
            .and_then(|r| r.cast::<IRandomAccessStreamReference>())
            .ok()
            .and_then(|r| read_stream(&r))
            .and_then(|b| image::load_from_memory(&b).ok())
            .and_then(|img| icon_data_url(&img));
        Some(Identity { name, icon })
    }

    /// Icon of a desktop app's running executable.
    fn desktop_icon(aumid: &str) -> Option<String> {
        let names = exe_candidates(aumid);
        if names.is_empty() {
            return None;
        }
        let img = win32::exe_icon(&win32::running_exe(&names)?)?;
        icon_data_url(&image::DynamicImage::ImageRgba8(img))
    }

    mod win32 {
        use windows::core::{BOOL, PCWSTR};
        use windows::Win32::Foundation::{HWND, LPARAM};
        use windows::Win32::Graphics::Gdi::{
            CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
            HBITMAP, HDC,
        };
        use windows::Win32::UI::Shell::SHDefExtractIconW;
        use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, EnumWindows, GetIconInfo, GetWindowThreadProcessId, HICON, ICONINFO};

        unsafe extern "system" fn collect_pid(hwnd: HWND, lparam: LPARAM) -> BOOL {
            // SAFETY: lparam is the &mut Vec<u32> running_exe passes, alive for the whole EnumWindows call.
            let pids = unsafe { &mut *(lparam.0 as *mut Vec<u32>) };
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid != 0 && !pids.contains(&pid) {
                pids.push(pid);
            }
            true.into()
        }

        /// Full path of a running process whose executable file name is one
        /// of `names` (lowercased), preferring earlier names.
        ///
        /// NOTE: Players that own a media session always have a top-level
        /// window (often hidden), so walking windows finds them without the
        /// process snapshot APIs.
        pub fn running_exe(names: &[String]) -> Option<String> {
            let mut pids: Vec<u32> = Vec::new();
            unsafe {
                let _ = EnumWindows(Some(collect_pid), LPARAM(&mut pids as *mut Vec<u32> as isize));
            }
            let paths: Vec<String> = pids.into_iter().filter_map(crate::plugins::volume::process_path).collect();
            names.iter().find_map(|name| paths.iter().find(|p| p.rsplit(['\\', '/']).next().is_some_and(|f| f.eq_ignore_ascii_case(name))).cloned())
        }

        fn bitmap_pixels(dc: HDC, bmp: HBITMAP, w: i32, h: i32) -> Option<Vec<u8>> {
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down rows
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let lines = unsafe { GetDIBits(dc, bmp, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS) };
            (lines == h).then_some(buf)
        }

        fn icon_rgba(icon: HICON) -> Option<image::RgbaImage> {
            let mut ii = ICONINFO::default();
            unsafe { GetIconInfo(icon, &mut ii) }.ok()?;
            let mut bm = BITMAP::default();
            let got = unsafe { GetObjectW(ii.hbmColor.into(), std::mem::size_of::<BITMAP>() as i32, Some((&mut bm as *mut BITMAP).cast())) };
            let (w, h) = (bm.bmWidth, bm.bmHeight);
            let out = if got == 0 || ii.hbmColor.is_invalid() || !(1..=256).contains(&w) || !(1..=256).contains(&h) {
                None
            } else {
                let dc = unsafe { CreateCompatibleDC(None) };
                let color = bitmap_pixels(dc, ii.hbmColor, w, h);
                let mask = bitmap_pixels(dc, ii.hbmMask, w, h);
                unsafe {
                    let _ = DeleteDC(dc);
                }
                color.and_then(|mut px| {
                    // COMPAT: old 24-bit icons carry no alpha; their
                    // transparency lives in the AND mask (white = clear).
                    let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
                    for (i, p) in px.chunks_exact_mut(4).enumerate() {
                        p.swap(0, 2);
                        if !has_alpha {
                            let clear = mask.as_ref().is_some_and(|m| m[i * 4] != 0);
                            p[3] = if clear { 0 } else { 255 };
                        }
                    }
                    image::RgbaImage::from_raw(w as u32, h as u32, px)
                })
            };
            unsafe {
                let _ = DeleteObject(ii.hbmColor.into());
                let _ = DeleteObject(ii.hbmMask.into());
            }
            out
        }

        /// The executable's main icon at 32 px.
        pub fn exe_icon(path: &str) -> Option<image::RgbaImage> {
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            let mut large = HICON::default();
            unsafe { SHDefExtractIconW(PCWSTR(wide.as_ptr()), 0, 0, Some(&mut large), None, super::ICON_SIZE) }.ok().ok()?;
            if large.is_invalid() {
                return None;
            }
            let img = icon_rgba(large);
            unsafe {
                let _ = DestroyIcon(large);
            }
            img
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The polling behind the wait macro, without a player.
        #[test]
        fn waits_end_when_done_and_time_out_otherwise() {
            assert!(wait_until_done(|| Ok(1), Duration::from_millis(1)).is_ok());
            let started = Instant::now();
            let err = wait_until_done(|| Ok(0), Duration::from_millis(30)).unwrap_err();
            assert_eq!(err.code(), windows::Win32::Foundation::ERROR_TIMEOUT.to_hresult());
            assert!(started.elapsed() >= Duration::from_millis(30));
            assert!(wait_until_done(|| Err(windows::core::Error::empty()), Duration::from_secs(1)).is_err());
        }

        /// Talks to the real session manager: it must be creatable from an
        /// unpackaged process, with or without anything playing.
        #[test]
        fn session_manager_is_available() {
            let manager = wait!(Manager::RequestAsync(), MANAGER_TIMEOUT);
            assert!(manager.is_ok(), "{manager:?}");
        }

        /// The icon pipeline, against an executable every Windows install has.
        #[test]
        fn extracts_an_exe_icon() {
            let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
            let img = win32::exe_icon(&format!(r"{windir}\explorer.exe")).expect("explorer.exe icon");
            assert!(img.width() > 0 && img.pixels().any(|p| p[3] > 0));
            assert!(icon_data_url(&image::DynamicImage::ImageRgba8(img)).unwrap().starts_with("data:image/png;base64,"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    fn decode(url: &str) -> image::DynamicImage {
        let bytes = base64::engine::general_purpose::STANDARD.decode(&url[url.find(',').unwrap() + 1..]).unwrap();
        image::load_from_memory(&bytes).unwrap()
    }

    fn dims(url: &str) -> (u32, u32) {
        let img = decode(url);
        (img.width(), img.height())
    }

    #[test]
    fn labels_players() {
        assert_eq!(app_label("Spotify.exe"), "Spotify");
        assert_eq!(app_label("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "Spotify");
        assert_eq!(app_label("Chrome"), "Chrome");
        assert_eq!(app_label("MSEdge"), "Edge");
        assert_eq!(app_label("308046B0AF4A39CB"), "Firefox");
        assert_eq!(app_label("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "Media Player");
        assert_eq!(app_label(r"C:\Apps\foobar2000.exe"), "Foobar2000");
        assert_eq!(app_label(""), "Media");
    }

    #[test]
    fn guesses_executables_for_desktop_players() {
        assert_eq!(exe_candidates("Spotify.exe"), vec!["spotify.exe"]);
        assert_eq!(exe_candidates("Chrome"), vec!["chrome.exe"]);
        assert_eq!(exe_candidates("MSEdge"), vec!["msedge.exe", "edge.exe"]);
        assert_eq!(exe_candidates("308046B0AF4A39CB"), vec!["308046b0af4a39cb.exe", "firefox.exe"]);
        assert!(exe_candidates("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic").is_empty());
    }

    #[test]
    fn artwork_keeps_its_shape_within_limits() {
        let square = artwork(&png(300, 300, [200, 30, 30])).unwrap();
        assert!(square.url.starts_with("data:image/jpeg;base64,"));
        assert_eq!(square.aspect, 1.0);
        assert_eq!(dims(&square.url), (ART_HEIGHT, ART_HEIGHT));

        let video = artwork(&png(480, 270, [0, 0, 0])).unwrap();
        assert_eq!(video.aspect, 1.78);
        assert_eq!(dims(&video.url), (171, ART_HEIGHT));

        assert_eq!(artwork(&png(900, 100, [0, 0, 0])).unwrap().aspect, 1.78);
        let portrait = artwork(&png(100, 300, [0, 0, 0])).unwrap();
        assert_eq!(portrait.aspect, 1.0);
        assert_eq!(dims(&portrait.url), (96, 96));

        assert_eq!(dims(&artwork(&png(40, 40, [0, 0, 0])).unwrap().url), (40, 40));
        assert!(artwork(b"not an image").is_none());
    }

    #[test]
    fn icons_are_small_transparent_pngs() {
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(64, 64, image::Rgba([1, 2, 3, 0])));
        let back = decode(&icon_data_url(&img).unwrap());
        assert_eq!((back.width(), back.height()), (ICON_SIZE, ICON_SIZE));
        assert_eq!(back.to_rgba8().get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn artwork_is_read_once_per_track() {
        let mut cache = ArtCache::default();
        let mut reads = 0;
        let a = png(10, 10, [1, 1, 1]);
        for _ in 0..3 {
            let art = cache.get(1, "a", "Album", || {
                reads += 1;
                Some(a.clone())
            });
            assert!(art.is_some());
        }
        assert_eq!(reads, 1);
        assert!(!cache.pending("a"));
    }

    #[test]
    fn missing_artwork_is_retried_a_few_times() {
        let mut cache = ArtCache::default();
        let mut reads = 0;
        for _ in 0..ART_ATTEMPTS + 2 {
            let art = cache.get(1, "a", "", || {
                reads += 1;
                None
            });
            assert!(art.is_none());
        }
        assert_eq!(reads, ART_ATTEMPTS);
        assert!(!cache.pending("a"));
    }

    #[test]
    fn the_previous_tracks_image_is_held_back_for_a_new_album() {
        let mut cache = ArtCache::default();
        let old = png(10, 10, [9, 9, 9]);
        let new = png(10, 10, [200, 9, 9]);
        let first = cache.get(1, "one", "First", || Some(old.clone())).unwrap();
        // The title changed but the thumbnail hasn't yet.
        assert!(cache.get(1, "two", "Second", || Some(old.clone())).is_none());
        assert!(cache.pending("two"));
        let second = cache.get(1, "two", "Second", || Some(new.clone())).unwrap();
        assert_ne!(first.url, second.url);
        // Same album, same cover: accepted straight away.
        assert!(cache.get(1, "three", "Second", || Some(new.clone())).is_some());
        // Another session's history doesn't count.
        assert!(cache.get(2, "four", "Other", || Some(new.clone())).is_some());
    }

    #[test]
    fn a_shared_image_is_accepted_once_retries_run_out() {
        let mut cache = ArtCache::default();
        let same = png(10, 10, [5, 5, 5]);
        cache.get(1, "one", "", || Some(same.clone())).unwrap();
        let tries: Vec<bool> = (0..ART_ATTEMPTS).map(|_| cache.get(1, "two", "", || Some(same.clone())).is_some()).collect();
        assert_eq!(tries.iter().filter(|t| **t).count(), 1);
        assert!(tries[usize::from(ART_ATTEMPTS) - 1]);
    }

    #[test]
    fn the_artwork_cache_is_bounded() {
        let mut cache = ArtCache::default();
        let img = png(4, 4, [0, 0, 0]);
        for i in 0..ART_CACHE + 4 {
            cache.get(i as u32, &i.to_string(), "", || Some(img.clone()));
        }
        assert_eq!(cache.entries.len(), ART_CACHE);
        assert!(cache.pending("0"));
        assert!(!cache.pending(&(ART_CACHE + 3).to_string()));
    }

    const NOW: i64 = 1_700_000_000_000;
    const NOW_FILETIME: i64 = FILETIME_UNIX_OFFSET + NOW * TICKS_PER_MS;

    fn raw(position_ms: i64, duration_ms: i64, updated: i64) -> RawTimeline {
        RawTimeline { start: 0, end: duration_ms * TICKS_PER_MS, position: position_ms * TICKS_PER_MS, min_seek: 0, max_seek: 0, updated }
    }

    #[test]
    fn timelines_anchor_at_the_reported_time() {
        let t = anchor(&raw(30_000, 200_000, NOW_FILETIME - 5_000 * TICKS_PER_MS), 1.0, true, NOW).unwrap();
        assert_eq!(t, Timeline { position_ms: 30_000, duration_ms: 200_000, updated_at: NOW - 5_000, rate: 1.0 });
        let offset = RawTimeline { start: 10_000 * TICKS_PER_MS, end: 70_000 * TICKS_PER_MS, position: 25_000 * TICKS_PER_MS, ..raw(0, 0, NOW_FILETIME) };
        let t = anchor(&offset, 0.0, false, NOW).unwrap();
        assert_eq!((t.position_ms, t.duration_ms, t.rate), (15_000, 60_000, 1.0));
        assert_eq!(anchor(&offset, f64::NAN, false, NOW).unwrap().rate, 1.0);
        assert_eq!(anchor(&offset, 1.5, false, NOW).unwrap().rate, 1.5);
    }

    #[test]
    fn implausible_timestamps_anchor_at_now() {
        assert_eq!(anchor(&raw(1_000, 60_000, 0), 1.0, true, NOW).unwrap().updated_at, NOW);
        assert_eq!(anchor(&raw(1_000, 60_000, NOW_FILETIME + 60_000 * TICKS_PER_MS), 1.0, true, NOW).unwrap().updated_at, NOW);
        // Playing, but advancing from an hour-old stamp would run far past the end.
        let stale = raw(10_000, 60_000, NOW_FILETIME - 3_600_000 * TICKS_PER_MS);
        assert_eq!(anchor(&stale, 1.0, true, NOW).unwrap().updated_at, NOW);
        // Paused, the old stamp is harmless.
        assert_eq!(anchor(&stale, 1.0, false, NOW).unwrap().updated_at, NOW - 3_600_000);
    }

    #[test]
    fn timelines_without_length_are_dropped_and_positions_clamped() {
        assert!(anchor(&raw(0, 0, NOW_FILETIME), 1.0, true, NOW).is_none());
        assert_eq!(anchor(&raw(90_000, 60_000, NOW_FILETIME), 1.0, false, NOW).unwrap().position_ms, 60_000);
        assert_eq!(anchor(&raw(-5_000, 60_000, NOW_FILETIME), 1.0, false, NOW).unwrap().position_ms, 0);
    }

    #[test]
    fn seeks_stay_inside_the_seekable_range() {
        let r = RawTimeline { start: 1_000, end: 600_001_000, position: 0, min_seek: 0, max_seek: 0, updated: 0 };
        assert_eq!(seek_target(&r, 30_000), 1_000 + 30_000 * TICKS_PER_MS);
        assert_eq!(seek_target(&r, 999_999), r.end);
        assert_eq!(seek_target(&r, u64::MAX), r.end);
        let windowed = RawTimeline { min_seek: 100_000_000, max_seek: 200_000_000, ..r };
        assert_eq!(seek_target(&windowed, 0), 100_000_000);
        assert_eq!(seek_target(&windowed, 59_000), 200_000_000);
    }

    #[test]
    fn repeat_cycles_off_all_one() {
        assert_eq!(Repeat::None.next(), Repeat::List);
        assert_eq!(Repeat::List.next(), Repeat::Track);
        assert_eq!(Repeat::Track.next(), Repeat::None);
        assert_eq!(serde_json::to_value(Repeat::List).unwrap(), "list");
    }

    #[test]
    fn a_pick_sticks_while_its_session_is_listed() {
        let sessions = [(1, false), (2, true), (3, false)];
        assert_eq!(choose(&sessions, Some(3), Some(1)), Some(3));
        assert_eq!(choose(&sessions, Some(9), Some(1)), Some(1));
        assert_eq!(choose(&sessions, None, None), Some(2));
        assert_eq!(choose(&sessions, None, Some(9)), Some(2));
        assert_eq!(choose(&[(4, false), (5, false)], None, None), Some(4));
        assert_eq!(choose(&[], Some(1), Some(1)), None);
    }

    #[test]
    fn sessions_keep_their_ids() {
        let mut next = 0;
        let first = assign_ids::<i32>(&[], &[(10, "Spotify.exe".into()), (20, "Chrome".into())], &mut next);
        assert_eq!(first, vec![1, 2]);
        let known = vec![(1, 10, "Spotify.exe".to_string()), (2, 20, "Chrome".to_string())];
        // Same objects in a new order, plus a newcomer.
        assert_eq!(assign_ids(&known, &[(20, "Chrome".into()), (30, "VLC".into()), (10, "Spotify.exe".into())], &mut next), vec![2, 3, 1]);
        // Chrome re-created its session object: the lone Chrome keeps id 2.
        assert_eq!(assign_ids(&known, &[(10, "Spotify.exe".into()), (21, "Chrome".into())], &mut next), vec![1, 2]);
        // Two new Chrome sessions are ambiguous, so both get fresh ids.
        assert_eq!(assign_ids(&known, &[(21, "Chrome".into()), (22, "Chrome".into())], &mut next), vec![4, 5]);
    }

    fn track(title: &str, playing: bool) -> Track {
        Track { app: "Spotify".into(), title: title.into(), active: true, playing, shuffle: Some(true), repeat: Some(Repeat::Track), ..Track::default() }
    }

    #[test]
    fn nothing_shows_without_an_active_track() {
        let mut t = Track { title: "Song".into(), active: false, ..Track::default() };
        assert_eq!(present(Some(&t), None), (None, None));
        t.active = true;
        t.playing = true;
        let (live, widget) = present(Some(&t), Some("data:x".into()));
        assert!(matches!(live, Some(LiveItem::Media { playing: true, .. })));
        assert!(matches!(widget, Some(Widget::Media(MediaWidget { artwork: Some(_), .. }))));
        t.title.clear();
        assert_eq!(present(Some(&t), None), (None, None));
        assert_eq!(present(None, None), (None, None));
    }

    #[test]
    fn detail_lists_active_sessions_and_the_shown_ones_modes() {
        let reads = vec![
            SessionRead { id: 1, icon: Some("data:i".into()), track: track("Song", true) },
            SessionRead { id: 2, icon: None, track: Track { shuffle: None, repeat: None, ..track("Video", false) } },
            SessionRead { id: 3, icon: None, track: Track { active: false, ..track("Old", false) } },
        ];
        let d = detail(&reads, Some(2), None, None, None).unwrap();
        assert_eq!(d.session, 2);
        assert_eq!(d.sessions.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!((d.shuffle, d.repeat, d.artwork_aspect), (None, None, 1.0));
        let d = detail(&reads, Some(1), Some(1.78), None, Some(AppVolume { level: 0.5, muted: false })).unwrap();
        assert_eq!((d.shuffle, d.repeat, d.artwork_aspect), (Some(true), Some(Repeat::Track), 1.78));
        assert!(detail(&reads, Some(3), None, None, None).is_none());
        assert!(detail(&reads, None, None, None, None).is_none());
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["sessions"][0]["icon"], "data:i");
        assert_eq!(v["repeat"], "track");
        assert!(v["timeline"].is_null());
    }

    #[test]
    fn controls_parse_from_action_ids() {
        assert_eq!(Control::parse("play-pause"), Some(Control::PlayPause));
        assert_eq!(Control::parse("next"), Some(Control::Next));
        assert_eq!(Control::parse("previous"), Some(Control::Previous));
        assert_eq!(Control::parse("shuffle"), Some(Control::Shuffle));
        assert_eq!(Control::parse("repeat"), Some(Control::Repeat));
        assert_eq!(Control::parse("seek:61500"), Some(Control::Seek(61_500)));
        assert_eq!(Control::parse("session:2"), Some(Control::Session(2)));
        assert_eq!(Control::parse("volume:35"), Some(Control::Volume(35)));
        assert_eq!(Control::parse("volume:101"), None);
        assert_eq!(Control::parse("mute"), Some(Control::Mute));
        assert_eq!(Control::parse("seek:-1"), None);
        assert_eq!(Control::parse("seek:"), None);
        assert_eq!(Control::parse("stop"), None);
    }
}
