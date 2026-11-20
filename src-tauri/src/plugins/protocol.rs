//! The JSON lines protocol spoken by external plugins (see `plugins/README.md`).
//!
//! Responsibilities:
//! - Parse one stdout line from a plugin into a validated [`Message`]:
//!   size caps, fixed icon and tone vocabularies, id formats and text limits.
//! - Define the output a plugin can show ([`PluginSignal`], [`LiveItem`],
//!   [`Widget`], [`FeedItem`]), which built-in plugins reuse, plus tab pages
//!   (parsed in [`super::page`]).
//! - Serialize the messages Pill writes back on the plugin's stdin, including
//!   the project list for plugins granted [`Capability::Projects`].
//!
//! SECURITY: plugin output is untrusted. Only plain text, numbers and names
//! from fixed lists ever reach the UI, which renders text as text; nothing a
//! plugin sends is interpreted as HTML or code. The one exception is a feed
//! item's link: it must be a plain `https://` URL, and it only opens in the
//! browser when the user clicks it.

use super::page::{self, NavItem, PageContent, PageUpdate};
use serde::{Deserialize, Serialize};

/// Protocol version announced in the `hello` message.
pub const PROTOCOL_VERSION: u32 = 1;

/// Longest stdout line Pill accepts, in bytes (newline excluded). `page`
/// lines may be longer, up to [`page::MAX_PAGE_LINE_BYTES`].
pub const MAX_LINE_BYTES: usize = 8 * 1024;
/// Signals one plugin may show at a time.
pub const MAX_SIGNALS: usize = 4;
/// Rows in one card widget.
pub const MAX_ROWS: usize = 8;
/// Buttons in one card widget.
pub const MAX_ACTIONS: usize = 4;
/// Characters of a live item's text.
pub const MAX_LIVE_TEXT: usize = 24;
/// Characters of a signal's tooltip label.
pub const MAX_LABEL: usize = 60;
/// Characters of a widget title.
pub const MAX_TITLE: usize = 40;
/// Characters of a widget row's text or detail.
pub const MAX_ROW_TEXT: usize = 80;
/// Characters of a widget button label.
pub const MAX_ACTION_LABEL: usize = 20;
/// Characters of an `error` message.
pub const MAX_ERROR: usize = 200;
/// Highest badge count; larger values are clamped.
pub const MAX_COUNT: u32 = 9_999;
/// Feed items one plugin may show at a time.
pub const MAX_FEED_ITEMS: usize = 100;
/// Characters of a feed item's title.
pub const MAX_FEED_TITLE: usize = 120;
/// Characters of a feed item's body.
pub const MAX_FEED_BODY: usize = 400;
/// Characters of a feed item's detail line.
pub const MAX_FEED_DETAIL: usize = 120;
/// Plugin-defined buttons on one feed item.
pub const MAX_ITEM_ACTIONS: usize = 2;
/// Longest feed item id, in bytes.
pub const MAX_ITEM_ID: usize = 128;
/// Longest feed item link, in bytes.
pub const MAX_URL: usize = 2048;
/// Characters of a suggested task's title.
pub const MAX_TASK_TITLE: usize = 120;
/// Characters of a suggested task's notes.
pub const MAX_TASK_NOTES: usize = 4_000;

/// Icons a plugin may name. The UI maps each to a Fluent icon; anything else
/// is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Icon {
    Alert,
    Bell,
    Bot,
    Branch,
    Bug,
    Calendar,
    Chat,
    Check,
    Clock,
    Cloud,
    Code,
    Download,
    Error,
    Flash,
    Globe,
    Heart,
    Info,
    Mail,
    Music,
    Pause,
    Person,
    Play,
    Rocket,
    Shield,
    Star,
    Timer,
    Warning,
    Wrench,
}

/// Every [`Icon`] name, in declaration order. The UI test checks its own
/// icon map and `plugins/README.md` against this list.
#[cfg(test)]
pub const ICON_NAMES: &[&str] = &[
    "alert", "bell", "bot", "branch", "bug", "calendar", "chat", "check", "clock", "cloud", "code", "download",
    "error", "flash", "globe", "heart", "info", "mail", "music", "pause", "person", "play", "rocket", "shield",
    "star", "timer", "warning", "wrench",
];

/// Colors a plugin may use; the UI maps them to its status palette.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    #[default]
    Neutral,
    Accent,
    Green,
    Amber,
    Red,
    Purple,
}

/// One entry in the collapsed notch's status cluster.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSignal {
    /// Plugin-chosen id; a later signal with the same id replaces it.
    pub id: String,
    /// Merges into one of Pill's own signal kinds instead of drawing its own
    /// icon. Only built-in plugins set it (e.g. `notification`).
    pub kind: Option<&'static str>,
    pub icon: Icon,
    pub tone: Tone,
    /// Badge number; signals are only kept while this is above zero.
    pub count: u32,
    /// Tooltip text.
    pub label: String,
}

/// What a plugin shows inside the collapsed notch. Pill shows one at a time.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LiveItem {
    /// A small icon and a few words.
    Text { icon: Option<Icon>, text: String, tone: Tone },
    /// Now playing: artwork with a playing hint.
    #[serde(rename_all = "camelCase")]
    Media {
        /// Track artwork as a small `data:image/jpeg` URL.
        artwork: Option<String>,
        playing: bool,
        /// Track title, for the tooltip.
        title: String,
    },
}

/// A widget row: an optional icon, a line of text and optional detail.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetRow {
    pub icon: Option<Icon>,
    pub text: String,
    pub detail: Option<String>,
    pub tone: Tone,
}

/// A widget button. Clicking it sends `{"type":"action","id":…}` back.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetAction {
    pub id: String,
    pub label: String,
    pub icon: Option<Icon>,
}

/// The now playing widget's data.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaWidget {
    /// Friendly name of the player, e.g. "Spotify".
    pub app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Track artwork as a small `data:image/jpeg` URL.
    pub artwork: Option<String>,
    pub playing: bool,
    pub can_play_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

/// A card on the Today tab.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Widget {
    /// Title, rows and buttons: the only widget external plugins can send.
    Card { title: String, rows: Vec<WidgetRow>, actions: Vec<WidgetAction> },
    /// Built-in now playing card with transport controls.
    Media(MediaWidget),
}

/// How urgent a feed item is, from least to most.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Info,
    Low,
    Medium,
    High,
    Critical,
}

/// A task a feed item suggests. Pill creates it in the item's project when
/// the user picks "Fix with an agent".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedTask {
    pub title: String,
    /// Multi-line notes. Kept out of the UI state; Pill fences them as
    /// untrusted text when it writes the task.
    #[serde(skip_serializing)]
    pub notes: String,
}

/// One entry in Pill's notification feed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    /// Plugin-chosen id. A later item with the same id replaces it, and a
    /// dismissed id stays hidden for good.
    pub id: String,
    pub severity: Severity,
    pub icon: Option<Icon>,
    pub title: String,
    pub body: String,
    /// One short line of facts, e.g. `lodash 4.17.20 → 4.17.21`.
    pub detail: Option<String>,
    /// An `https://` link the user can open.
    pub url: Option<String>,
    /// The Pill project the item is about.
    pub project_id: Option<String>,
    pub task: Option<FeedTask>,
    /// Plugin-defined buttons; a click sends `{"type":"action","id":…,"item":…}`.
    pub actions: Vec<WidgetAction>,
}

/// A change to a plugin's feed.
#[derive(Debug, Clone, PartialEq)]
pub enum FeedChange {
    /// Add or replace an item.
    Put(FeedItem),
    /// Remove one item by id.
    Remove(String),
    /// Remove every item.
    Clear,
}

/// A validated message from a plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Add or replace a signal; `count: 0` removes it.
    Signal(PluginSignal),
    /// Show a live item, or clear it with `None`.
    Live(Option<LiveItem>),
    /// Show a widget, or clear it with `None`.
    Widget(Option<Widget>),
    /// Report a problem to show in Settings, or clear it with `None`.
    Error(Option<String>),
    /// Change the plugin's feed items.
    Feed(FeedChange),
    /// A page for the plugin's tab, or why it couldn't make one.
    Page(PageUpdate),
    /// Badges and order for the tab's project sidebar, and what was dropped.
    Nav(Vec<NavItem>, Vec<String>),
}

/// Which parts of the protocol a plugin may use, as declared in its
/// manifest. `signals`, `live`, `widget`, `feed` and `tab` gate what it may
/// send; `projects` grants what Pill sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Signals,
    Live,
    Widget,
    /// Items in Pill's notification feed.
    Feed,
    /// A tab in the panel's header whose pages the plugin sends on request.
    Tab,
    /// Receive every project's id, name and folder.
    Projects,
}

impl Capability {
    /// Whether the capability tells the plugin something about the user's
    /// machine, so approving the plugin must name it explicitly.
    ///
    /// NOTE: `tab` doesn't. A tab only shows what the plugin sends, like a
    /// widget; the `view` messages it brings say which of the plugin's own
    /// pages is on screen, and name a project only to plugins that already
    /// hold the project list through `projects`.
    pub fn grants_access(self) -> bool {
        matches!(self, Capability::Projects)
    }

    /// Whether the capability lets the plugin offer tasks: feed items and
    /// page buttons can carry a task that Pill adds to a project, where an
    /// agent may run it, so approving the plugin must say so.
    pub fn suggests_tasks(self) -> bool {
        matches!(self, Capability::Feed | Capability::Tab)
    }
}

impl Message {
    /// The capability this message needs, if any. Errors are always allowed.
    pub fn capability(&self) -> Option<Capability> {
        match self {
            Message::Signal(_) => Some(Capability::Signals),
            Message::Live(_) => Some(Capability::Live),
            Message::Widget(_) => Some(Capability::Widget),
            Message::Feed(_) => Some(Capability::Feed),
            Message::Page(_) | Message::Nav(..) => Some(Capability::Tab),
            Message::Error(_) => None,
        }
    }
}

// Raw wire shapes. Unknown fields are ignored so newer plugins keep working
// with an older Pill; unknown message types are rejected.

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Raw {
    Signal(RawSignal),
    Live(RawLive),
    Widget(RawWidget),
    Error(RawError),
    Feed(Box<RawFeed>),
    Page(RawPage),
    Nav(RawNav),
}

#[derive(Deserialize)]
struct RawNav {
    items: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawPage {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    page: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct RawSignal {
    id: String,
    icon: Icon,
    #[serde(default)]
    tone: Tone,
    count: u64,
    #[serde(default)]
    label: String,
}

#[derive(Deserialize)]
struct RawLive {
    #[serde(default)]
    clear: bool,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    icon: Option<Icon>,
    #[serde(default)]
    tone: Tone,
}

#[derive(Deserialize)]
struct RawWidget {
    #[serde(default)]
    clear: bool,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    rows: Vec<RawRow>,
    #[serde(default)]
    actions: Vec<RawAction>,
}

#[derive(Deserialize)]
struct RawRow {
    text: String,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    icon: Option<Icon>,
    #[serde(default)]
    tone: Tone,
}

#[derive(Deserialize)]
struct RawAction {
    id: String,
    label: String,
    #[serde(default)]
    icon: Option<Icon>,
}

#[derive(Deserialize)]
struct RawError {
    message: Option<String>,
}

#[derive(Deserialize)]
struct RawFeed {
    #[serde(default)]
    clear: bool,
    #[serde(default)]
    remove: bool,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    severity: Severity,
    #[serde(default)]
    icon: Option<Icon>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    task: Option<RawTask>,
    #[serde(default)]
    actions: Vec<RawAction>,
}

#[derive(Deserialize)]
struct RawTask {
    title: String,
    #[serde(default)]
    notes: String,
}

/// Whether `c` is an invisible formatting character (Unicode category `Cf`):
/// bidi marks and overrides, zero-width spaces, the byte order mark, word
/// joiners, interlinear annotations and tag characters.
///
/// SECURITY: bidi controls can make text read differently than it is, and
/// zero-width characters hide inside names that look identical.
///
/// NOTE: U+200D (zero-width joiner) stays, because emoji sequences such as
/// 👩‍💻 need it, and it can't reorder text.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'
            | '\u{200C}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// Removes control characters (including newlines) and invisible format
/// characters such as bidi overrides and zero-width spaces, collapses runs
/// of whitespace, trims and cuts to `max` characters with an ellipsis.
///
/// @param s Untrusted text.
/// @param max Maximum characters kept, including the ellipsis.
/// @returns Display-safe single-line text.
pub fn clean_text(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len().min(max.saturating_mul(4)));
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if c.is_control() || is_invisible_format(c) {
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(c);
    }
    if out.chars().count() > max {
        let cut: String = out.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    } else {
        out
    }
}

/// Like [`clean_text`] but keeps line breaks, for multi-line notes: each line
/// is cleaned, runs of blank lines collapse to one, and the result is cut to
/// `max` characters.
///
/// @param s Untrusted text.
/// @param max Maximum characters kept, including the ellipsis.
pub fn clean_block(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in s.replace("\r\n", "\n").split(['\n', '\r']).map(|l| clean_text(l, usize::MAX)) {
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank { "\n\n" } else { "\n" });
        }
        blank = false;
        out.push_str(&line);
    }
    if out.chars().count() > max {
        let cut: String = out.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    } else {
        out
    }
}

/// Whether `id` is a valid feed item id: 1–128 bytes of ASCII letters,
/// digits and `. _ - : @ / +`.
pub fn is_item_id(id: &str) -> bool {
    (1..=MAX_ITEM_ID).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b".-_:@/+".contains(&b))
}

/// Whether `id` looks like a Pill project id (letters, digits, `_`, `-`).
pub fn is_project_id(id: &str) -> bool {
    (1..=96).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Accepts only a plain `https://` URL: visible ASCII, a host, no user
/// info, at most [`MAX_URL`] bytes.
///
/// SECURITY: other schemes can launch local programs through Windows'
/// protocol handlers, and `https://github.com@evil.example/` reads like
/// GitHub but goes elsewhere, so user info is refused too.
///
/// @param raw Untrusted link.
/// @returns The trimmed link, or why it was refused.
pub fn clean_url(raw: &str) -> Result<String, String> {
    let url = raw.trim();
    if url.len() > MAX_URL {
        return Err(format!("url is longer than {MAX_URL} bytes"));
    }
    let rest = url.strip_prefix("https://").ok_or("url must start with https://")?;
    if !url.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("url may only contain visible ASCII characters".into());
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains(['@', '\\']) {
        return Err("url needs a plain host name".into());
    }
    Ok(url.to_string())
}

/// Whether `id` is 1–32 characters of `a-z`, `0-9`, `-` or `_`.
pub fn is_token(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Parses and validates one line of plugin output.
///
/// @param line One stdout line without its newline.
/// @returns The message, or why it was rejected.
pub fn parse_line(line: &str) -> Result<Message, String> {
    if line.len() > page::MAX_PAGE_LINE_BYTES {
        return Err(format!("line is {} bytes; the limit is {}", line.len(), page::MAX_PAGE_LINE_BYTES));
    }
    // NOTE: a sidebar lists up to MAX_NAV_ITEMS projects, which outgrows the
    // ordinary line limit long before it reaches its own cap.
    if line.len() > MAX_LINE_BYTES && !matches!(peek_type(line).as_deref(), Some("page" | "nav")) {
        return Err(format!("line is {} bytes; the limit is {MAX_LINE_BYTES}", line.len()));
    }
    let raw: Raw = serde_json::from_str(line.trim()).map_err(|e| format!("not a valid message: {e}"))?;
    match raw {
        Raw::Signal(s) => {
            if !is_token(&s.id) {
                return Err(format!("signal id {:?} must be 1-32 characters of a-z, 0-9, - or _", s.id));
            }
            Ok(Message::Signal(PluginSignal {
                id: s.id,
                kind: None,
                icon: s.icon,
                tone: s.tone,
                count: u32::try_from(s.count).unwrap_or(MAX_COUNT).min(MAX_COUNT),
                label: clean_text(&s.label, MAX_LABEL),
            }))
        }
        Raw::Live(l) if l.clear => Ok(Message::Live(None)),
        Raw::Live(l) => {
            let text = clean_text(l.text.as_deref().unwrap_or_default(), MAX_LIVE_TEXT);
            if text.is_empty() && l.icon.is_none() {
                return Err("live needs text or an icon (or \"clear\": true)".into());
            }
            Ok(Message::Live(Some(LiveItem::Text { icon: l.icon, text, tone: l.tone })))
        }
        Raw::Widget(w) if w.clear => Ok(Message::Widget(None)),
        Raw::Widget(w) => {
            let title = clean_text(w.title.as_deref().unwrap_or_default(), MAX_TITLE);
            if title.is_empty() {
                return Err("widget needs a title (or \"clear\": true)".into());
            }
            if w.rows.len() > MAX_ROWS {
                return Err(format!("widget has {} rows; the limit is {MAX_ROWS}", w.rows.len()));
            }
            if w.actions.len() > MAX_ACTIONS {
                return Err(format!("widget has {} actions; the limit is {MAX_ACTIONS}", w.actions.len()));
            }
            let rows = w
                .rows
                .into_iter()
                .map(|r| WidgetRow {
                    icon: r.icon,
                    text: clean_text(&r.text, MAX_ROW_TEXT),
                    detail: r.detail.map(|d| clean_text(&d, MAX_ROW_TEXT)).filter(|d| !d.is_empty()),
                    tone: r.tone,
                })
                .collect();
            let actions = parse_actions(w.actions)?;
            Ok(Message::Widget(Some(Widget::Card { title, rows, actions })))
        }
        Raw::Error(e) => Ok(Message::Error(e.message.map(|m| clean_text(&m, MAX_ERROR)).filter(|m| !m.is_empty()))),
        Raw::Feed(f) => parse_feed(*f).map(Message::Feed),
        Raw::Page(p) => parse_page(p).map(Message::Page),
        Raw::Nav(n) => {
            let (items, warnings) = page::parse_nav(&n.items);
            Ok(Message::Nav(items, warnings))
        }
    }
}

/// The `type` of a JSON object line, read without building the rest of it.
///
/// PERF: other fields are skipped by the deserializer instead of becoming
/// values, so refusing a long line of the wrong type costs one scan.
fn peek_type(line: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Type {
        #[serde(rename = "type")]
        kind: String,
    }
    serde_json::from_str::<Type>(line.trim()).ok().map(|t| t.kind)
}

fn parse_page(p: RawPage) -> Result<PageUpdate, String> {
    if let Some(id) = p.project.as_deref().filter(|id| !is_project_id(id)) {
        return Err(format!("project {id:?} isn't a project id"));
    }
    let (content, warnings) = match (p.page, p.error) {
        (Some(_), Some(_)) => return Err("a page message has either \"page\" or \"error\", not both".into()),
        (None, Some(e)) => (PageContent::Error(clean_text(&e, page::MAX_PAGE_ERROR)), Vec::new()),
        (Some(v), None) => {
            let (parsed, warnings) = page::parse_page(&v, p.project.is_some())?;
            (PageContent::Page(parsed), warnings)
        }
        (None, None) => return Err("a page message needs \"page\" or \"error\"".into()),
    };
    Ok(PageUpdate { project: p.project, content, warnings })
}

fn parse_actions(raw: Vec<RawAction>) -> Result<Vec<WidgetAction>, String> {
    let mut actions: Vec<WidgetAction> = Vec::with_capacity(raw.len());
    for a in raw {
        if !is_token(&a.id) {
            return Err(format!("action id {:?} must be 1-32 characters of a-z, 0-9, - or _", a.id));
        }
        if actions.iter().any(|x| x.id == a.id) {
            return Err(format!("action id {:?} appears twice", a.id));
        }
        let label = clean_text(&a.label, MAX_ACTION_LABEL);
        if label.is_empty() {
            return Err(format!("action {:?} needs a label", a.id));
        }
        actions.push(WidgetAction { id: a.id, label, icon: a.icon });
    }
    Ok(actions)
}

fn parse_feed(f: RawFeed) -> Result<FeedChange, String> {
    if f.clear {
        return Ok(FeedChange::Clear);
    }
    let id = f.id.ok_or("feed needs an id (or \"clear\": true)")?;
    if !is_item_id(&id) {
        return Err(format!("feed id {id:?} must be 1-{MAX_ITEM_ID} characters of A-Z, a-z, 0-9 or . _ - : @ / +"));
    }
    if f.remove {
        return Ok(FeedChange::Remove(id));
    }
    let title = clean_text(f.title.as_deref().unwrap_or_default(), MAX_FEED_TITLE);
    if title.is_empty() {
        return Err(format!("feed item {id:?} needs a title"));
    }
    if f.actions.len() > MAX_ITEM_ACTIONS {
        return Err(format!("feed item has {} actions; the limit is {MAX_ITEM_ACTIONS}", f.actions.len()));
    }
    let project_id = match f.project {
        Some(p) if !is_project_id(&p) => return Err(format!("project {p:?} isn't a project id")),
        p => p,
    };
    let task = match f.task {
        Some(_) if project_id.is_none() => return Err("a feed item with a task needs a project".into()),
        Some(t) => {
            let title = clean_text(&t.title, MAX_TASK_TITLE);
            if title.is_empty() {
                return Err("a feed item's task needs a title".into());
            }
            Some(FeedTask { title, notes: clean_block(&t.notes, MAX_TASK_NOTES) })
        }
        None => None,
    };
    Ok(FeedChange::Put(FeedItem {
        id,
        severity: f.severity,
        icon: f.icon,
        title,
        body: clean_text(f.body.as_deref().unwrap_or_default(), MAX_FEED_BODY),
        detail: f.detail.map(|d| clean_text(&d, MAX_FEED_DETAIL)).filter(|d| !d.is_empty()),
        url: f.url.as_deref().map(clean_url).transpose()?,
        project_id,
        task,
        actions: parse_actions(f.actions)?,
    }))
}

/// A project as plugins with [`Capability::Projects`] see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    /// The project's folder.
    pub path: std::path::PathBuf,
}

/// A message Pill writes to a plugin's stdin.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HostMessage<'a> {
    /// First line after start.
    Hello { protocol: u32, pill: &'a str },
    /// The user clicked a widget button, a feed item's button (`item`), or
    /// a button on one of the plugin's pages (`page`, plus `item` for a
    /// list row's button).
    Action {
        id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        item: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        page: Option<PageRef<'a>>,
    },
    /// Every project; sent after `hello` and whenever the list changes.
    Projects { projects: &'a [ProjectInfo] },
    /// The plugin's tab started showing the page for `project` (`None`:
    /// the overview), or stopped showing it (`closed`).
    View {
        project: Option<&'a str>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        closed: bool,
    },
}

/// Which page a page button was on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PageRef<'a> {
    /// The page's project; `None` (`null`) for the overview page.
    pub project: Option<&'a str>,
}

impl HostMessage<'_> {
    /// One JSON line including the trailing newline.
    pub fn to_line(&self) -> String {
        let mut s = serde_json::to_string(self).expect("host messages always serialize");
        s.push('\n');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_signal_and_clamps_the_count() {
        let m = parse_line(r#"{"type":"signal","id":"builds","icon":"warning","tone":"amber","count":123456,"label":"Builds failing"}"#).unwrap();
        let Message::Signal(s) = m else { panic!("expected a signal") };
        assert_eq!(s.id, "builds");
        assert_eq!(s.icon, Icon::Warning);
        assert_eq!(s.tone, Tone::Amber);
        assert_eq!(s.count, MAX_COUNT);
        assert_eq!(s.kind, None);
    }

    #[test]
    fn rejects_unknown_icons_types_and_bad_ids() {
        assert!(parse_line(r#"{"type":"signal","id":"x","icon":"skull","count":1}"#).is_err());
        assert!(parse_line(r#"{"type":"html","body":"<b>hi</b>"}"#).is_err());
        assert!(parse_line(r#"{"type":"signal","id":"Bad Id","icon":"bell","count":1}"#).is_err());
        assert!(parse_line(r#"{"type":"signal","id":"x","icon":"bell","count":-1}"#).is_err());
        assert!(parse_line("not json").is_err());
        assert!(parse_line(r#"["type","signal"]"#).is_err());
    }

    #[test]
    fn rejects_oversized_lines() {
        let line = format!(r#"{{"type":"error","message":"{}"}}"#, "x".repeat(MAX_LINE_BYTES));
        assert!(parse_line(&line).unwrap_err().contains("limit"));
        let text = "x".repeat(page::MAX_TEXT);
        let blocks = vec![format!(r#"{{"type":"text","text":"{text}"}}"#); 6].join(",");
        let long_page = format!(r#"{{"type":"page","project":null,"page":{{"blocks":[{blocks}]}}}}"#);
        assert!(long_page.len() > MAX_LINE_BYTES);
        assert!(parse_line(&long_page).is_ok(), "pages may use the larger limit");
        let huge = format!(r#"{{"type":"page","error":"{}"}}"#, "x".repeat(page::MAX_PAGE_LINE_BYTES));
        assert!(parse_line(&huge).unwrap_err().contains("limit"));
    }

    #[test]
    fn page_messages_are_validated() {
        let m = parse_line(r#"{"type":"page","project":"p_1","page":{"blocks":[{"type":"text","text":"Hi"},{"type":"bogus"}]}}"#).unwrap();
        let Message::Page(update) = m else { panic!("expected a page") };
        assert_eq!(update.project.as_deref(), Some("p_1"));
        assert!(matches!(&update.content, PageContent::Page(p) if p.blocks.len() == 1));
        assert_eq!(update.warnings.len(), 1);
        assert_eq!(Message::Page(update).capability(), Some(Capability::Tab));
        let err = parse_line(r#"{"type":"page","project":null,"error":"OSV.dev is down\nretrying"}"#).unwrap();
        assert!(matches!(err, Message::Page(PageUpdate { project: None, content: PageContent::Error(ref e), .. }) if e == "OSV.dev is down retrying"));
        assert!(parse_line(r#"{"type":"page","project":"../x","page":{"blocks":[]}}"#).is_err());
        assert!(parse_line(r#"{"type":"page","project":null}"#).is_err());
        assert!(parse_line(r#"{"type":"page","page":{"blocks":[]},"error":"x"}"#).is_err());
        let nav = parse_line(r#"{"type":"nav","items":[{"project":"p_1","badge":{"text":"2 high","tone":"amber"}}]}"#).unwrap();
        assert!(matches!(&nav, Message::Nav(items, w) if items.len() == 1 && w.is_empty()));
        assert_eq!(nav.capability(), Some(Capability::Tab));
        assert!(parse_line(r#"{"type":"nav"}"#).is_err());
    }

    #[test]
    fn live_items_truncate_and_clear() {
        let m = parse_line(r#"{"type":"live","icon":"rocket","text":"Deploying the whole monorepo to production"}"#).unwrap();
        let Message::Live(Some(LiveItem::Text { text, icon, .. })) = m else { panic!() };
        assert_eq!(text.chars().count(), MAX_LIVE_TEXT);
        assert!(text.ends_with('…'));
        assert_eq!(icon, Some(Icon::Rocket));
        assert_eq!(parse_line(r#"{"type":"live","clear":true}"#).unwrap(), Message::Live(None));
        assert!(parse_line(r#"{"type":"live"}"#).is_err());
    }

    #[test]
    fn widgets_validate_rows_and_actions() {
        let ok = parse_line(
            r#"{"type":"widget","title":"Builds","rows":[{"text":"main","detail":"passed","icon":"check","tone":"green"}],"actions":[{"id":"refresh","label":"Refresh"}]}"#,
        )
        .unwrap();
        let Message::Widget(Some(Widget::Card { rows, actions, .. })) = ok else { panic!() };
        assert_eq!(rows[0].detail.as_deref(), Some("passed"));
        assert_eq!(actions[0].id, "refresh");

        let too_many = format!(r#"{{"type":"widget","title":"T","rows":[{}]}}"#, [r#"{"text":"r"}"#; MAX_ROWS + 1].join(","));
        assert!(parse_line(&too_many).is_err());
        assert!(parse_line(r#"{"type":"widget","title":"T","actions":[{"id":"a","label":"A"},{"id":"a","label":"B"}]}"#).is_err());
        assert!(parse_line(r#"{"type":"widget","rows":[]}"#).is_err());
        assert_eq!(parse_line(r#"{"type":"widget","clear":true}"#).unwrap(), Message::Widget(None));
    }

    #[test]
    fn text_is_stripped_of_controls_and_bidi() {
        assert_eq!(clean_text("  a\n\tb\u{202E}c\u{0007}  ", 50), "a bc");
        assert_eq!(clean_text("<b>bold</b>", 50), "<b>bold</b>");
        assert_eq!(clean_text("\u{FEFF}pay\u{200B}pal\u{061C}\u{2060}\u{E0041}\u{00AD}", 50), "paypal");
        assert_eq!(clean_text("👩\u{200D}💻 dev", 50), "👩\u{200D}💻 dev", "emoji sequences keep their joiner");
    }

    #[test]
    fn unknown_fields_are_ignored() {
        assert!(parse_line(r#"{"type":"error","message":"Signed out","future":true}"#).is_ok());
        assert_eq!(parse_line(r#"{"type":"error","message":null}"#).unwrap(), Message::Error(None));
    }

    #[test]
    fn parses_a_feed_item_with_a_task() {
        let m = parse_line(
            r#"{"type":"feed","id":"GHSA-35jh-r3h4-6jhm:p_1","severity":"high","icon":"shield","title":"lodash: command injection","body":"Template\nis unsafe","detail":"lodash 4.17.20 → 4.17.21","url":"https://osv.dev/vulnerability/GHSA-35jh-r3h4-6jhm","project":"p_1","task":{"title":"Update lodash to 4.17.21","notes":"Line one\r\nLine two\n\n\n\nLast\u202e"},"actions":[{"id":"ignore","label":"Ignore package"}],"future":1}"#,
        )
        .unwrap();
        let Message::Feed(FeedChange::Put(item)) = m else { panic!("expected a feed item") };
        assert_eq!(item.severity, Severity::High);
        assert_eq!(item.body, "Template is unsafe");
        assert_eq!(item.project_id.as_deref(), Some("p_1"));
        assert_eq!(item.task.as_ref().unwrap().notes, "Line one\nLine two\n\nLast");
        assert_eq!(item.actions[0].id, "ignore");
        assert_eq!(Message::Feed(FeedChange::Put(item)).capability(), Some(Capability::Feed));
        let v = serde_json::to_value(parse_line(r#"{"type":"feed","id":"a","title":"T","project":"p_1","task":{"title":"Fix","notes":"secret-ish"}}"#).map(|m| match m {
            Message::Feed(FeedChange::Put(i)) => i,
            _ => panic!(),
        }).unwrap())
        .unwrap();
        assert_eq!(v["task"], serde_json::json!({ "title": "Fix" }), "notes stay out of the UI state");
        assert_eq!(v["severity"], "info");
    }

    #[test]
    fn feed_removals_and_clears() {
        assert_eq!(parse_line(r#"{"type":"feed","clear":true}"#).unwrap(), Message::Feed(FeedChange::Clear));
        assert_eq!(parse_line(r#"{"type":"feed","id":"x","remove":true}"#).unwrap(), Message::Feed(FeedChange::Remove("x".into())));
        assert!(parse_line(r#"{"type":"feed","remove":true}"#).is_err());
    }

    #[test]
    fn feed_items_are_validated() {
        let bad = [
            r#"{"type":"feed","id":"has space","title":"T"}"#,
            r#"{"type":"feed","id":"a"}"#,
            r#"{"type":"feed","id":"a","title":"T","severity":"apocalyptic"}"#,
            r#"{"type":"feed","id":"a","title":"T","url":"http://example.com"}"#,
            r#"{"type":"feed","id":"a","title":"T","url":"file:///C:/Windows/System32/calc.exe"}"#,
            r#"{"type":"feed","id":"a","title":"T","url":"https://github.com@evil.example/x"}"#,
            r#"{"type":"feed","id":"a","title":"T","url":"https:// spaced.example"}"#,
            r#"{"type":"feed","id":"a","title":"T","url":"https:///path"}"#,
            r#"{"type":"feed","id":"a","title":"T","project":"../etc"}"#,
            r#"{"type":"feed","id":"a","title":"T","task":{"title":"Fix"}}"#,
            r#"{"type":"feed","id":"a","title":"T","project":"p_1","task":{"title":"  "}}"#,
            r#"{"type":"feed","id":"a","title":"T","actions":[{"id":"a","label":"A"},{"id":"b","label":"B"},{"id":"c","label":"C"}]}"#,
        ];
        for line in bad {
            assert!(parse_line(line).is_err(), "{line}");
        }
        assert!(parse_line(&format!(r#"{{"type":"feed","id":"{}","title":"T"}}"#, "a".repeat(MAX_ITEM_ID + 1))).is_err());
        assert!(parse_line(r#"{"type":"feed","id":"npm:@babel/core@7.0.0+x","title":"T","url":"https://osv.dev/vulnerability/X?a=1#b"}"#).is_ok());
    }

    #[test]
    fn severities_order_by_urgency() {
        assert!(Severity::Critical > Severity::High && Severity::High > Severity::Medium && Severity::Low > Severity::Info);
    }

    #[test]
    fn only_projects_grants_access() {
        assert!(Capability::Projects.grants_access());
        for cap in [Capability::Signals, Capability::Live, Capability::Widget, Capability::Feed, Capability::Tab] {
            assert!(!cap.grants_access());
        }
        for cap in [Capability::Feed, Capability::Tab] {
            assert!(cap.suggests_tasks());
        }
        for cap in [Capability::Signals, Capability::Live, Capability::Widget, Capability::Projects] {
            assert!(!cap.suggests_tasks());
        }
    }

    #[test]
    fn clean_block_keeps_paragraphs_and_cuts() {
        assert_eq!(clean_block("a\r\nb\n\n\n\tc  d\u{0007}", 100), "a\nb\n\nc d");
        assert_eq!(clean_block("\n\nx\n\n", 100), "x");
        assert_eq!(clean_block("abcdef", 4), "abc…");
    }

    #[test]
    fn icon_names_match_the_enum() {
        for name in ICON_NAMES {
            let icon: Icon = serde_json::from_value(serde_json::Value::String((*name).into())).unwrap();
            assert_eq!(serde_json::to_value(icon).unwrap(), *name);
        }
    }

    #[test]
    fn host_messages_are_single_lines() {
        assert_eq!(HostMessage::Action { id: "refresh", item: None, page: None }.to_line(), "{\"type\":\"action\",\"id\":\"refresh\"}\n");
        assert_eq!(
            HostMessage::Action { id: "snooze", item: Some("GHSA-1:p_1"), page: None }.to_line(),
            "{\"type\":\"action\",\"id\":\"snooze\",\"item\":\"GHSA-1:p_1\"}\n"
        );
        let projects = [ProjectInfo { id: "p_1".into(), name: "Shop".into(), path: r"C:\code\shop".into() }];
        assert_eq!(
            HostMessage::Projects { projects: &projects }.to_line(),
            "{\"type\":\"projects\",\"projects\":[{\"id\":\"p_1\",\"name\":\"Shop\",\"path\":\"C:\\\\code\\\\shop\"}]}\n"
        );
        assert_eq!(
            HostMessage::Hello { protocol: PROTOCOL_VERSION, pill: "0.1.0" }.to_line(),
            "{\"type\":\"hello\",\"protocol\":1,\"pill\":\"0.1.0\"}\n"
        );
        assert_eq!(HostMessage::View { project: Some("p_1"), closed: false }.to_line(), "{\"type\":\"view\",\"project\":\"p_1\"}\n");
        assert_eq!(HostMessage::View { project: None, closed: true }.to_line(), "{\"type\":\"view\",\"project\":null,\"closed\":true}\n");
        assert_eq!(
            HostMessage::Action { id: "ignore", item: Some("npm/lodash"), page: Some(PageRef { project: None }) }.to_line(),
            "{\"type\":\"action\",\"id\":\"ignore\",\"item\":\"npm/lodash\",\"page\":{\"project\":null}}\n"
        );
    }

    #[test]
    fn a_long_sidebar_may_exceed_the_ordinary_line_limit() {
        let items: Vec<String> = (0..200)
            .map(|i| format!(r#"{{"project":"p_{i:04}","badge":{{"text":"{i} critical, {i} high","tone":"red"}}}}"#))
            .collect();
        let line = format!(r#"{{"type":"nav","items":[{}]}}"#, items.join(","));
        assert!(line.len() > MAX_LINE_BYTES);
        assert!(matches!(parse_line(&line), Ok(Message::Nav(..))));
        let chatty = format!(r#"{{"type":"live","text":"{}"}}"#, "x".repeat(MAX_LINE_BYTES));
        assert!(parse_line(&chatty).unwrap_err().contains("the limit is"));
        let disguised = format!(r#"{{"inner":{{"type":"page"}},"type":"live","text":"{}"}}"#, "x".repeat(MAX_LINE_BYTES));
        assert!(parse_line(&disguised).unwrap_err().contains("the limit is"), "only the top-level type counts");
        assert_eq!(peek_type(r#" {"items":[{"type":"x"}],"type":"nav"} "#).as_deref(), Some("nav"));
    }
}
