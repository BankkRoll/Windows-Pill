//! Plugins: small sources of signals, a live item and a widget.
//!
//! Responsibilities:
//! - Keep the registry: built-in plugins compiled into Pill plus external
//!   plugins found in `%APPDATA%\Pill\plugins` (see [`manifest`]).
//! - Start and stop each plugin's runtime to match the user's settings.
//!   External plugins only run once the user has approved their exact
//!   command line, folder and access (see [`Manifest::approval_in`]); if
//!   any of it changes, they stop until the user approves the new one.
//! - Hold each plugin's latest output and status and send the whole list to
//!   the UI as `pill://plugins` when anything changes, at most once per
//!   [`EMIT_DEBOUNCE`] for output from running plugins.
//! - Route widget button clicks back to the plugin that drew them.
//! - Keep each plugin's feed items (minus the ones the user dismissed) and
//!   send the project list to plugins granted the `projects` capability.
//! - For plugins with a tab: track which windows show which of its pages,
//!   tell the plugin (`view`), cache the pages it sends per project and
//!   publish them as `pill://plugin-page`, and route page buttons back.

pub mod external;
pub mod feed;
pub mod manifest;
pub mod media;
pub mod notifications;
pub mod page;
pub mod protocol;
pub mod volume;

use crate::config::PluginSettings;
use crate::system_settings::SystemPage;
use manifest::Manifest;
use page::{Button, ButtonKind, NavItem, Page, PageContent, Row, TabSpec};
use protocol::{Capability, FeedChange, FeedItem, LiveItem, Message, PluginSignal, ProjectInfo, Widget};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Event carrying every plugin's [`PluginState`] after any change.
pub const PLUGINS_EVENT: &str = "pill://plugins";
/// Event carrying a [`PageEvent`] whenever a plugin sends a page that
/// differs from the one cached for its project.
pub const PAGE_EVENT: &str = "pill://plugin-page";
/// How long plugin output is gathered before [`PLUGINS_EVENT`] goes out.
///
/// PERF: every event carries every plugin's whole state, so a plugin
/// sending its 50 lines a second would otherwise make the UI re-render
/// the full list 50 times a second.
pub const EMIT_DEBOUNCE: Duration = Duration::from_millis(100);

/// Source of runtime generations, shared by every plugin.
///
/// NOTE: global rather than per entry, so an entry recreated by a rescan
/// never reuses a generation an old runtime of the same id still holds;
/// lines that runtime had buffered are then always dropped.
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

fn next_generation() -> u64 {
    NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)
}

/// The last page a plugin sent for one project (or its overview).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageState {
    /// `None` when the plugin answered with an error instead.
    pub page: Option<Page>,
    /// Why the plugin couldn't make the page.
    pub error: Option<String>,
    /// When it arrived, in Unix milliseconds.
    pub received_at: u64,
}

/// A page update for the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageEvent {
    /// The plugin that sent the page.
    pub plugin_id: String,
    /// The project the page is about; `None` for the overview page.
    pub project: Option<String>,
    /// The page as cached, or the error the plugin sent instead.
    pub state: PageState,
}

/// A window showing one of a plugin's pages.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Viewer {
    /// The window's label.
    label: String,
    project: Option<String>,
}

/// Where a plugin comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Builtin,
    External,
}

/// Coarse state shown next to each plugin in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StatusState {
    /// Turned off.
    Off,
    /// Turned on and starting up.
    Starting,
    Running,
    /// Windows needs the user's permission first.
    NeedsPermission,
    /// The OS doesn't offer what the plugin needs.
    Unsupported,
    /// Stopped by a failure.
    Error,
    /// External plugin whose command changed since the user approved it.
    Changed,
    /// External plugin whose manifest is broken.
    Invalid,
}

/// A one-click way out of a status, shown as a button in Settings.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fix {
    /// Open a Windows Settings page.
    Settings { page: SystemPage, label: String },
    /// Send an action to the plugin's runtime (e.g. request access).
    Action { id: String, label: String },
    /// Stop and start the plugin again.
    Restart,
}

/// A plugin's status line.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: StatusState,
    /// Short explanation for anything other than plain running or off.
    pub message: Option<String>,
    pub fix: Option<Fix>,
}

impl Status {
    /// A status with no message or fix.
    pub fn plain(state: StatusState) -> Self {
        Self { state, message: None, fix: None }
    }

    /// A status with a message and an optional fix.
    pub fn with(state: StatusState, message: impl Into<String>, fix: Option<Fix>) -> Self {
        Self { state, message: Some(message.into()), fix }
    }
}

/// Everything the UI knows about one plugin.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginState {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: Source,
    pub capabilities: Vec<Capability>,
    pub enabled: bool,
    pub status: Status,
    /// External plugins: the exact command line that runs.
    pub command: Option<String>,
    /// External plugins: what turning it on approves, the command line plus
    /// any access it asks for (see [`Manifest::approval`]).
    pub approval: Option<String>,
    /// External plugins: the plugin's folder.
    pub folder: Option<PathBuf>,
    pub signals: Vec<PluginSignal>,
    pub live: Option<LiveItem>,
    pub widget: Option<Widget>,
    /// Feed items the user hasn't dismissed, in the order the plugin sent them.
    pub feed: Vec<FeedItem>,
    /// The plugin's tab, when it has the `tab` capability.
    pub tab: Option<TabSpec>,
    /// Badges and order for the tab's project sidebar, as the plugin last
    /// sent them.
    pub nav: Vec<NavItem>,
    /// Tab plugins: whether the tab sits in the header (`false` when the user
    /// keeps it in the header's overflow menu). Always `true` without a tab.
    pub pinned: bool,
}

/// A running plugin. Both methods must return quickly: they're called with
/// the registry locked.
pub trait Runtime: Send {
    /// Handles a widget button or status fix.
    fn action(&mut self, id: &str) -> Result<(), String>;
    /// Stops the plugin. Output sent after this is ignored.
    fn stop(&mut self);
    /// Hands over the project list; only called for plugins with the
    /// `projects` capability.
    fn projects(&mut self, _projects: &[ProjectInfo]) {}
    /// Handles a feed item's button.
    fn item_action(&mut self, _item: &str, _id: &str) -> Result<(), String> {
        Err("This plugin has no item buttons".into())
    }
    /// The plugin's tab started (`open`) or stopped showing a page; only
    /// called for plugins with the `tab` capability.
    fn view(&mut self, _project: Option<&str>, _open: bool) {}
    /// Handles a button on one of the plugin's pages.
    fn page_action(&mut self, _project: Option<&str>, _item: Option<&str>, _id: &str) -> Result<(), String> {
        Err("This plugin has no page buttons".into())
    }
}

/// How a runtime publishes output and status. Updates from a runtime that
/// was stopped (an older generation) are dropped.
#[derive(Clone)]
pub struct Sink {
    plugins: Arc<Plugins>,
    id: String,
    generation: u64,
}

impl Sink {
    /// Sets the plugin's status.
    pub fn status(&self, status: Status) {
        self.plugins.update(&self.id, self.generation, |e| e.status = status);
    }

    /// Applies a protocol message, enforcing the plugin's capabilities, the
    /// per-plugin limits and the user's dismissals.
    ///
    /// @returns Why the message was refused, if it was.
    pub fn apply(&self, message: Message) -> Result<(), String> {
        let mut result = Ok(None);
        self.plugins.update(&self.id, self.generation, |e| {
            let dismissed = self.plugins.dismissed.lock().unwrap();
            let projects = self.plugins.projects.lock().unwrap();
            result = apply_message(e, message, &dismissed, &projects);
        });
        if let Some(event) = result? {
            let _ = self.plugins.app.emit(PAGE_EVENT, event);
        }
        Ok(())
    }

    /// Replaces every signal at once (built-ins).
    pub fn set_signals(&self, signals: Vec<PluginSignal>) {
        self.plugins.update(&self.id, self.generation, |e| e.signals = signals);
    }

    /// Replaces the live item (built-ins).
    pub fn set_live(&self, live: Option<LiveItem>) {
        self.plugins.update(&self.id, self.generation, |e| e.live = live);
    }

    /// Replaces the widget (built-ins).
    pub fn set_widget(&self, widget: Option<Widget>) {
        self.plugins.update(&self.id, self.generation, |e| e.widget = widget);
    }

    /// The app handle, for built-ins that publish their own typed events.
    pub fn app(&self) -> &AppHandle {
        &self.plugins.app
    }

    /// Emits a plugin-specific event while this runtime is current, for
    /// built-ins whose UI needs more than the widget model offers.
    ///
    /// @returns Whether the event went out.
    pub fn emit<T: Serialize + Clone>(&self, event: &str, payload: T) -> bool {
        let current = self.plugins.entries.lock().unwrap().iter().any(|e| e.id == self.id && e.generation == self.generation && e.runtime.is_some());
        if current {
            let _ = self.plugins.app.emit(event, payload);
        }
        current
    }

    /// Drops all output, e.g. after the process died.
    pub fn clear(&self) {
        self.plugins.update(&self.id, self.generation, |e| {
            e.signals.clear();
            e.live = None;
            e.widget = None;
            e.feed.clear();
            e.nav.clear();
            e.pages.clear();
        });
    }
}

/// Applies one message to a plugin's entry. Messages for capabilities the
/// manifest didn't declare are refused, and dismissed feed items are
/// dropped quietly so they never come back.
///
/// @param projects The project list as plugins last saw it.
/// @returns The page update to publish, for `page` messages whose page
///   changed.
fn apply_message(e: &mut Entry, message: Message, dismissed: &feed::Dismissed, projects: &[ProjectInfo]) -> Result<Option<PageEvent>, String> {
    if let Some(cap) = message.capability() {
        if !e.capabilities.contains(&cap) {
            return Err(format!("{cap:?} isn't in this plugin's capabilities").to_lowercase());
        }
    }
    match message {
        Message::Signal(s) => {
            if let Some(i) = e.signals.iter().position(|x| x.id == s.id) {
                if s.count == 0 {
                    e.signals.remove(i);
                } else {
                    e.signals[i] = s;
                }
            } else if s.count > 0 {
                if e.signals.len() >= protocol::MAX_SIGNALS {
                    return Err(format!("at most {} signals at a time", protocol::MAX_SIGNALS));
                }
                e.signals.push(s);
            }
        }
        Message::Live(l) => e.live = l,
        Message::Widget(w) => e.widget = w,
        Message::Error(m) => e.status.message = m,
        Message::Feed(FeedChange::Clear) => e.feed.clear(),
        Message::Feed(FeedChange::Remove(id)) => e.feed.retain(|i| i.id != id),
        Message::Feed(FeedChange::Put(item)) => {
            if let Some(project) = item.project_id.as_deref() {
                // SECURITY: a project id is what lets a feed item's task land
                // in a project, so only plugins approved for `projects` may
                // name one, and only one that exists.
                if !e.capabilities.contains(&Capability::Projects) {
                    return Err("feed items for a project need the projects capability".into());
                }
                if !projects.iter().any(|p| p.id == project) {
                    return Err(format!("project {project:?} isn't one of Pill's projects"));
                }
            }
            if dismissed.contains(&e.id, &item.id) {
                return Ok(None);
            }
            if let Some(i) = e.feed.iter().position(|x| x.id == item.id) {
                e.feed[i] = item;
            } else if e.feed.len() >= protocol::MAX_FEED_ITEMS {
                return Err(format!("at most {} feed items at a time", protocol::MAX_FEED_ITEMS));
            } else {
                e.feed.push(item);
            }
        }
        Message::Page(update) => {
            if update.project.is_some() && !e.capabilities.contains(&Capability::Projects) {
                return Err("pages for a project need the projects capability".into());
            }
            if !update.warnings.is_empty() {
                let which = update.project.as_deref().map_or("the overview page".to_string(), |p| format!("the page for {p}"));
                e.status.message =
                    Some(protocol::clean_text(&format!("Dropped parts of {which}: {}", update.warnings.join("; ")), protocol::MAX_ERROR));
            }
            let (page, error) = match update.content {
                PageContent::Page(p) => (Some(p), None),
                PageContent::Error(m) => (None, Some(m)),
            };
            if e.pages.iter().any(|(p, s)| *p == update.project && s.page == page && s.error == error) {
                e.use_page(&update.project);
                return Ok(None);
            }
            let state = PageState { page, error, received_at: now_ms() };
            e.put_page(update.project.clone(), state.clone());
            return Ok(Some(PageEvent { plugin_id: e.id.clone(), project: update.project, state }));
        }
        Message::Nav(items, warnings) => {
            if items.iter().any(|i| i.project.is_some()) && !e.capabilities.contains(&Capability::Projects) {
                return Err("sidebar badges for projects need the projects capability".into());
            }
            if !warnings.is_empty() {
                e.status.message = Some(protocol::clean_text(&format!("Dropped parts of the sidebar: {}", warnings.join("; ")), protocol::MAX_ERROR));
            }
            e.nav = items;
        }
    }
    Ok(None)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Hands `projects` to every running plugin granted the `projects`
/// capability, and to no other.
fn send_projects(entries: &mut [Entry], projects: &[ProjectInfo]) {
    for e in entries.iter_mut().filter(|e| e.capabilities.contains(&Capability::Projects)) {
        if let Some(rt) = e.runtime.as_mut() {
            rt.projects(projects);
        }
    }
}

/// The project list plugins see, in a stable order so it is only resent
/// when a project is added, removed, renamed or moved.
///
/// @param snapshot The workspace snapshot.
pub fn project_list(snapshot: &crate::workspace::Snapshot) -> Vec<ProjectInfo> {
    let mut list: Vec<ProjectInfo> =
        snapshot.projects.iter().map(|p| ProjectInfo { id: p.id.clone(), name: p.name.clone(), path: p.path.clone() }).collect();
    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    list
}

/// A plugin compiled into Pill.
struct Builtin {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    capabilities: &'static [Capability],
    /// Whether it runs before the user has touched its toggle.
    default_on: bool,
}

const BUILTINS: &[Builtin] = &[
    Builtin {
        id: media::ID,
        name: "Now playing",
        description: "What's playing in Spotify, your browser or any player, with controls.",
        capabilities: &[Capability::Live, Capability::Widget],
        default_on: true,
    },
    Builtin {
        id: notifications::ID,
        name: "Windows notifications",
        description: "Counts notifications in Action Center, lists them on Today and dismisses them from there.",
        capabilities: &[Capability::Signals, Capability::Widget],
        default_on: false,
    },
];

/// Starts a built-in plugin.
///
/// NOTE: Kept out of [`BUILTINS`] so code (and tests) that only read plugin
/// metadata don't link the runtimes, which pull in the whole Tauri runtime.
fn start_builtin(id: &str, sink: Sink) -> Result<Box<dyn Runtime>, String> {
    match id {
        media::ID => media::start(sink),
        notifications::ID => notifications::start(sink),
        other => Err(format!("No built-in plugin {other}")),
    }
}

/// Ids external plugins can't take.
pub fn reserved_ids() -> Vec<&'static str> {
    BUILTINS.iter().map(|b| b.id).collect()
}

/// Whether a built-in plugin is on under `settings`.
///
/// @param settings The plugins section of the config.
/// @param id Built-in plugin id.
pub fn builtin_enabled(settings: &PluginSettings, id: &str) -> bool {
    let default = BUILTINS.iter().find(|b| b.id == id).is_some_and(|b| b.default_on);
    settings.builtin.get(id).copied().unwrap_or(default)
}

/// Whether an external plugin is approved exactly as it would run now.
///
/// @param settings The plugins section of the config.
/// @param id External plugin id.
/// @param approval What turning it on would approve now (see
///   [`Manifest::approval_in`]).
pub fn external_approved(settings: &PluginSettings, id: &str, approval: &str) -> bool {
    settings.external.get(id).is_some_and(|approved| approved == approval)
}

/// Emits [`PLUGINS_EVENT`] once per burst of updates: the first update
/// after a quiet spell waits [`EMIT_DEBOUNCE`], then one event carries
/// everything that changed meanwhile.
fn flush_events(plugins: Weak<Plugins>, rx: mpsc::Receiver<()>) {
    while rx.recv().is_ok() {
        std::thread::sleep(EMIT_DEBOUNCE);
        while rx.try_recv().is_ok() {}
        let Some(plugins) = plugins.upgrade() else { return };
        let states = plugins.states();
        plugins.emit(states);
    }
}

/// The user's order for external plugins, cleaned against the plugins that
/// exist: unknown and repeated ids drop out, and plugins the order doesn't
/// name yet (newly installed ones) follow in `known`'s order.
///
/// @param order Ids as saved or as the UI sent them.
/// @param known Every external plugin id, in discovery order.
/// @returns Each id of `known` exactly once.
/// @example
/// normalize_order(&["b".into(), "gone".into()], &["a".into(), "b".into()]) // ["b", "a"]
pub fn normalize_order(order: &[String], known: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(known.len());
    for id in order.iter().chain(known) {
        if known.contains(id) && !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

/// Sorts entries the way every plugin list shows them: built-ins first, as
/// compiled, then external plugins in the user's order. Plugins the order
/// doesn't name keep their discovery order at the end.
fn arrange(entries: &mut [Entry], order: &[String]) {
    entries.sort_by_key(|e| match e.kind {
        Kind::Builtin(_) => (0, 0),
        Kind::External { .. } => (1, order.iter().position(|id| *id == e.id).unwrap_or(usize::MAX)),
    });
}

enum Kind {
    Builtin(&'static Builtin),
    External { dir: PathBuf, manifest: Result<Manifest, String> },
}

struct Entry {
    kind: Kind,
    /// External plugins: the folder with links resolved, which the plugin
    /// runs from and the approval names.
    folder: Option<PathBuf>,
    /// External plugins with a valid manifest: what turning them on approves.
    approval: Option<String>,
    id: String,
    name: String,
    description: String,
    capabilities: Vec<Capability>,
    enabled: bool,
    status: Status,
    signals: Vec<PluginSignal>,
    live: Option<LiveItem>,
    widget: Option<Widget>,
    feed: Vec<FeedItem>,
    tab: Option<TabSpec>,
    nav: Vec<NavItem>,
    pinned: bool,
    /// Pages the plugin sent, least recently used first.
    pages: Vec<(Option<String>, PageState)>,
    /// Windows showing one of its pages. Kept while the plugin restarts, so
    /// the new process is told what is on screen.
    viewers: Vec<Viewer>,
    /// The current runtime's generation (see [`NEXT_GENERATION`]).
    generation: u64,
    runtime: Option<Box<dyn Runtime>>,
}

/// The parts of an entry its runtime can change, to tell whether an update
/// changed anything the UI shows.
#[derive(PartialEq)]
struct Output {
    status: Status,
    signals: Vec<PluginSignal>,
    live: Option<LiveItem>,
    widget: Option<Widget>,
    feed: Vec<FeedItem>,
    nav: Vec<NavItem>,
}

impl Output {
    fn of(e: &Entry) -> Self {
        Self {
            status: e.status.clone(),
            signals: e.signals.clone(),
            live: e.live.clone(),
            widget: e.widget.clone(),
            feed: e.feed.clone(),
            nav: e.nav.clone(),
        }
    }

    /// Compares without cloning `e` again.
    fn matches(&self, e: &Entry) -> bool {
        self.status == e.status
            && self.signals == e.signals
            && self.live == e.live
            && self.widget == e.widget
            && self.feed == e.feed
            && self.nav == e.nav
    }
}

impl Entry {
    fn new(kind: Kind) -> Self {
        let folder = match &kind {
            Kind::External { dir, .. } => Some(manifest::canonical_folder(dir)),
            Kind::Builtin(_) => None,
        };
        let approval = match (&kind, &folder) {
            (Kind::External { manifest: Ok(m), .. }, Some(f)) => Some(m.approval_in(f)),
            _ => None,
        };
        let (id, name, description, capabilities, tab) = match &kind {
            Kind::Builtin(b) => (b.id.to_string(), b.name.to_string(), b.description.to_string(), b.capabilities.to_vec(), None),
            Kind::External { manifest: Ok(m), .. } => {
                (m.id.clone(), m.name.clone(), m.description.clone(), m.capabilities.clone(), m.tab.clone())
            }
            Kind::External { dir, manifest: Err(_) } => {
                let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                (format!("folder:{folder}"), folder, String::new(), Vec::new(), None)
            }
        };
        Self {
            kind,
            folder,
            approval,
            id,
            name,
            description,
            capabilities,
            enabled: false,
            status: Status::plain(StatusState::Off),
            signals: Vec::new(),
            live: None,
            widget: None,
            feed: Vec::new(),
            tab,
            nav: Vec::new(),
            pinned: true,
            pages: Vec::new(),
            viewers: Vec::new(),
            generation: 0,
            runtime: None,
        }
    }

    fn command_line(&self) -> Option<String> {
        match &self.kind {
            Kind::External { manifest: Ok(m), .. } => Some(m.command_line()),
            _ => None,
        }
    }

    fn state(&self) -> PluginState {
        let source = match &self.kind {
            Kind::Builtin(_) => Source::Builtin,
            Kind::External { .. } => Source::External,
        };
        PluginState {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            source,
            capabilities: self.capabilities.clone(),
            enabled: self.enabled,
            status: self.status.clone(),
            command: self.command_line(),
            approval: self.approval.clone(),
            folder: self.folder.clone(),
            signals: self.signals.clone(),
            live: self.live.clone(),
            widget: self.widget.clone(),
            feed: self.feed.clone(),
            tab: self.tab.clone(),
            nav: self.nav.clone(),
            pinned: self.pinned,
        }
    }

    fn stop(&mut self) {
        if let Some(mut rt) = self.runtime.take() {
            rt.stop();
        }
        self.generation = next_generation();
        self.signals.clear();
        self.live = None;
        self.widget = None;
        self.feed.clear();
        self.nav.clear();
        self.pages.clear();
    }

    /// Caches a page, evicting the least recently used page nobody is
    /// looking at once more than [`page::MAX_CACHED_PAGES`] are kept.
    fn put_page(&mut self, project: Option<String>, state: PageState) {
        self.pages.retain(|(p, _)| *p != project);
        self.pages.push((project, state));
        while self.pages.len() > page::MAX_CACHED_PAGES {
            let unseen = self.pages.iter().position(|(p, _)| !self.viewers.iter().any(|v| v.project == *p));
            self.pages.remove(unseen.unwrap_or(0));
        }
    }

    /// The cached page for `project`, marked as just used.
    fn use_page(&mut self, project: &Option<String>) -> Option<&PageState> {
        let at = self.pages.iter().position(|(p, _)| p == project)?;
        let entry = self.pages.remove(at);
        self.pages.push(entry);
        self.pages.last().map(|(_, s)| s)
    }

    /// Records that window `label` shows the page for `project`, telling
    /// the plugin when no other window already showed it, and closing the
    /// window's previous page when nobody else shows that one.
    ///
    /// @returns The cached page, if the plugin sent one before.
    fn open_view(&mut self, label: &str, project: Option<String>) -> Option<PageState> {
        let previous = self.viewers.iter().position(|v| v.label == label).map(|i| self.viewers.remove(i));
        let shown = self.viewers.iter().any(|v| v.project == project) || previous.as_ref().is_some_and(|v| v.project == project);
        if let Some(old) = previous.filter(|old| old.project != project) {
            if !self.viewers.iter().any(|v| v.project == old.project) {
                if let Some(rt) = self.runtime.as_mut() {
                    rt.view(old.project.as_deref(), false);
                }
            }
        }
        if !shown {
            if let Some(rt) = self.runtime.as_mut() {
                rt.view(project.as_deref(), true);
            }
        }
        self.viewers.push(Viewer { label: label.to_string(), project: project.clone() });
        self.use_page(&project).cloned()
    }

    /// Records that window `label` no longer shows any of the plugin's
    /// pages, telling the plugin when nobody else shows that page.
    fn close_view(&mut self, label: &str) {
        let Some(at) = self.viewers.iter().position(|v| v.label == label) else { return };
        let old = self.viewers.remove(at);
        if !self.viewers.iter().any(|v| v.project == old.project) {
            if let Some(rt) = self.runtime.as_mut() {
                rt.view(old.project.as_deref(), false);
            }
        }
    }

    /// Tells a freshly started runtime which pages are on screen.
    fn resend_views(&mut self) {
        let Some(rt) = self.runtime.as_mut() else { return };
        let mut sent: Vec<&Option<String>> = Vec::new();
        for v in &self.viewers {
            if !sent.contains(&&v.project) {
                rt.view(v.project.as_deref(), true);
                sent.push(&v.project);
            }
        }
    }

    /// The button a page click names, from the cached page for `project`.
    fn page_button(&self, project: &Option<String>, item: Option<&str>, id: &str) -> Result<(Button, Option<Row>), String> {
        let page = self
            .pages
            .iter()
            .find(|(p, _)| p == project)
            .and_then(|(_, s)| s.page.as_ref())
            .ok_or_else(|| format!("{} isn't showing that page any more", self.name))?;
        let found = page.find_button(item, id).ok_or_else(|| format!("{} doesn't offer {id:?} on that page right now", self.name))?;
        Ok((found.button.clone(), found.row.cloned()))
    }
}

/// The plugin registry, shared as `Arc<Plugins>` in Tauri state.
pub struct Plugins {
    app: AppHandle,
    dir: PathBuf,
    entries: Mutex<Vec<Entry>>,
    /// Feed items the user dismissed. Lock after `entries`, never before.
    dismissed: Mutex<feed::Dismissed>,
    /// What plugins with the `projects` capability were last told. Lock
    /// after `entries` (and after `dismissed` when holding both), never
    /// before.
    projects: Mutex<Vec<ProjectInfo>>,
    /// Wakes the thread that emits [`PLUGINS_EVENT`] after
    /// [`EMIT_DEBOUNCE`].
    dirty: mpsc::Sender<()>,
}

impl Plugins {
    /// Creates the registry with every built-in and the external plugins
    /// currently in `dir`. Nothing runs until [`Plugins::sync`].
    ///
    /// @param app For emitting [`PLUGINS_EVENT`].
    /// @param dir The plugins folder.
    pub fn new(app: AppHandle, dir: PathBuf) -> Arc<Self> {
        let mut entries: Vec<Entry> = BUILTINS.iter().map(|b| Entry::new(Kind::Builtin(b))).collect();
        entries.extend(
            manifest::discover(&dir, &reserved_ids()).into_iter().map(|f| Entry::new(Kind::External { dir: f.dir, manifest: f.manifest })),
        );
        let dismissed = feed::Dismissed::load(dir.with_file_name("feed-dismissed.json"));
        let projects = {
            use tauri::Manager;
            app.try_state::<crate::state::AppState>().map(|s| project_list(&s.snapshot())).unwrap_or_default()
        };
        Arc::new_cyclic(|this: &Weak<Self>| {
            let (dirty, rx) = mpsc::channel();
            let this = this.clone();
            if let Err(e) = std::thread::Builder::new().name("plugin-events".into()).spawn(move || flush_events(this, rx)) {
                log::warn!("plugins: no event thread, so plugin output won't reach the UI: {e}");
            }
            Self { app, dir, entries: Mutex::new(entries), dismissed: Mutex::new(dismissed), projects: Mutex::new(projects), dirty }
        })
    }

    /// The plugins folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every plugin's current state.
    pub fn states(&self) -> Vec<PluginState> {
        self.entries.lock().unwrap().iter().map(Entry::state).collect()
    }

    fn emit(&self, states: Vec<PluginState>) {
        let _ = self.app.emit(PLUGINS_EVENT, states);
    }

    /// Applies `edit` to a plugin if `generation` is still current, and
    /// schedules [`PLUGINS_EVENT`] only when something visible changed.
    fn update(&self, id: &str, generation: u64, edit: impl FnOnce(&mut Entry)) {
        let mut entries = self.entries.lock().unwrap();
        let Some(e) = entries.iter_mut().find(|e| e.id == id && e.generation == generation && e.runtime.is_some()) else {
            return;
        };
        let before = Output::of(e);
        edit(e);
        if !before.matches(e) {
            let _ = self.dirty.send(());
        }
    }

    /// Starts and stops plugins to match `settings`.
    ///
    /// @param settings The plugins section of the config.
    pub fn sync(self: &Arc<Self>, settings: &PluginSettings) {
        let states = {
            let mut entries = self.entries.lock().unwrap();
            for e in entries.iter_mut() {
                self.sync_entry(e, settings);
            }
            arrange(&mut entries, &settings.order);
            entries.iter().map(Entry::state).collect()
        };
        self.emit(states);
    }

    fn sync_entry(self: &Arc<Self>, e: &mut Entry, settings: &PluginSettings) {
        let want = match &e.kind {
            Kind::Builtin(b) => builtin_enabled(settings, b.id),
            Kind::External { manifest: Ok(m), .. } => external_approved(settings, &m.id, e.approval.as_deref().unwrap_or_default()),
            Kind::External { manifest: Err(_), .. } => false,
        };
        e.enabled = want;
        e.pinned = !settings.menu_tabs.contains(&e.id);
        if want && e.runtime.is_none() {
            self.start_entry(e);
        } else if !want {
            if e.runtime.is_some() {
                e.stop();
            }
            e.status = match &e.kind {
                Kind::External { manifest: Err(err), .. } => Status::with(StatusState::Invalid, err.clone(), None),
                Kind::External { manifest: Ok(m), .. } if settings.external.contains_key(&m.id) => Status::with(
                    StatusState::Changed,
                    "The command, its folder or what it can access changed since you turned this on. Check it, then turn it on again.",
                    None,
                ),
                _ => Status::plain(StatusState::Off),
            };
        }
    }

    fn start_entry(self: &Arc<Self>, e: &mut Entry) {
        e.generation = next_generation();
        let sink = Sink { plugins: self.clone(), id: e.id.clone(), generation: e.generation };
        let started = match &e.kind {
            Kind::Builtin(b) => start_builtin(b.id, sink),
            Kind::External { dir, manifest: Ok(m) } => {
                external::start(sink, e.folder.as_deref().unwrap_or(dir), &self.dir.with_file_name("plugin-data").join(&m.id), m)
            }
            Kind::External { manifest: Err(err), .. } => Err(err.clone()),
        };
        match started {
            Ok(rt) => {
                e.runtime = Some(rt);
                send_projects(std::slice::from_mut(e), &self.projects.lock().unwrap());
                e.resend_views();
                e.status = Status::plain(StatusState::Starting);
            }
            Err(err) => {
                e.status = Status::with(StatusState::Error, err, Some(Fix::Restart));
            }
        }
    }

    /// Re-reads the plugins folder. Unchanged plugins keep running; removed
    /// ones stop; new ones appear turned off unless already approved.
    ///
    /// @param settings The plugins section of the config.
    pub fn rescan(self: &Arc<Self>, settings: &PluginSettings) {
        let found = manifest::discover(&self.dir, &reserved_ids());
        {
            let mut entries = self.entries.lock().unwrap();
            let mut next: Vec<Entry> = Vec::new();
            let mut old: Vec<Entry> = std::mem::take(&mut *entries);
            // Built-ins keep their place and runtime.
            let mut i = 0;
            while i < old.len() {
                if matches!(old[i].kind, Kind::Builtin(_)) {
                    next.push(old.remove(i));
                } else {
                    i += 1;
                }
            }
            for f in found {
                // NOTE: a junction retargeted since the last scan resolves to
                // another folder, which needs its own approval.
                let folder = manifest::canonical_folder(&f.dir);
                let same = old.iter().position(|e| match (&e.kind, &f.manifest) {
                    (Kind::External { dir, manifest: Ok(a) }, Ok(b)) => dir == &f.dir && a == b && e.folder.as_ref() == Some(&folder),
                    _ => false,
                });
                match same {
                    Some(at) => next.push(old.remove(at)),
                    None => next.push(Entry::new(Kind::External { dir: f.dir, manifest: f.manifest })),
                }
            }
            for mut gone in old {
                gone.stop();
            }
            *entries = next;
        }
        self.sync(settings);
    }

    /// Stops and starts one plugin.
    ///
    /// @param id Plugin id.
    pub fn restart(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let states = {
            let mut entries = self.entries.lock().unwrap();
            let e = entries.iter_mut().find(|e| e.id == id).ok_or_else(|| format!("No plugin {id}"))?;
            if !e.enabled {
                return Err("Turn the plugin on first".into());
            }
            e.stop();
            self.start_entry(e);
            entries.iter().map(Entry::state).collect()
        };
        self.emit(states);
        Ok(())
    }

    /// Sends a widget button or status fix to a running plugin. External
    /// plugins only receive ids their current widget offers.
    ///
    /// @param id Plugin id.
    /// @param action Action id.
    pub fn action(&self, id: &str, action: &str) -> Result<(), String> {
        let mut entries = self.entries.lock().unwrap();
        let e = entries.iter_mut().find(|e| e.id == id).ok_or_else(|| format!("No plugin {id}"))?;
        if matches!(e.kind, Kind::External { .. }) {
            let offered = matches!(&e.widget, Some(Widget::Card { actions, .. }) if actions.iter().any(|a| a.id == action));
            if !offered {
                return Err(format!("{} doesn't offer {action:?} right now", e.name));
            }
        }
        let rt = e.runtime.as_mut().ok_or_else(|| format!("{} isn't running", e.name))?;
        rt.action(action)
    }

    /// Sends the project list to every running plugin with the `projects`
    /// capability, if it changed since last time.
    ///
    /// @param projects Every project, as [`project_list`] orders them.
    pub fn set_projects(&self, projects: Vec<ProjectInfo>) {
        let mut entries = self.entries.lock().unwrap();
        let mut current = self.projects.lock().unwrap();
        if *current == projects {
            return;
        }
        *current = projects;
        send_projects(&mut entries, &current);
    }

    /// Hides feed items for good: they leave the feed now, and the same ids
    /// are ignored if the plugin sends them again.
    ///
    /// @param plugin Plugin id.
    /// @param items Item ids to dismiss.
    pub fn dismiss(&self, plugin: &str, items: Vec<String>) -> Result<(), String> {
        let saved = self.dismissed.lock().unwrap().add(plugin, items.iter().cloned());
        let states = {
            let mut entries = self.entries.lock().unwrap();
            if let Some(e) = entries.iter_mut().find(|e| e.id == plugin) {
                e.feed.retain(|i| !items.contains(&i.id));
            }
            entries.iter().map(Entry::state).collect()
        };
        self.emit(states);
        saved.map_err(|e| format!("Dismissed for now, but Pill couldn't save it, so it may come back after a restart: {e}"))
    }

    /// Dismisses every feed item of one plugin, or of every plugin.
    pub fn dismiss_all(&self, plugin: Option<&str>) -> Result<(), String> {
        let showing: Vec<(String, Vec<String>)> = self
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| !e.feed.is_empty() && plugin.is_none_or(|p| p == e.id))
            .map(|e| (e.id.clone(), e.feed.iter().map(|i| i.id.clone()).collect()))
            .collect();
        let mut result = Ok(());
        for (plugin, items) in showing {
            if let Err(e) = self.dismiss(&plugin, items) {
                result = Err(e);
            }
        }
        result
    }

    /// A feed item as the plugin last sent it, and the plugin's name.
    ///
    /// @param plugin Plugin id.
    /// @param item Item id.
    pub fn feed_item(&self, plugin: &str, item: &str) -> Result<(String, FeedItem), String> {
        let entries = self.entries.lock().unwrap();
        let e = entries.iter().find(|e| e.id == plugin).ok_or_else(|| format!("No plugin {plugin}"))?;
        let found = e.feed.iter().find(|i| i.id == item).ok_or("That item is no longer in the feed")?;
        Ok((e.name.clone(), found.clone()))
    }

    /// Sends a feed item's button to its plugin. Only buttons the item
    /// currently offers are sent.
    ///
    /// @param plugin Plugin id.
    /// @param item Item id.
    /// @param action Button id.
    pub fn item_action(&self, plugin: &str, item: &str, action: &str) -> Result<(), String> {
        let mut entries = self.entries.lock().unwrap();
        let e = entries.iter_mut().find(|e| e.id == plugin).ok_or_else(|| format!("No plugin {plugin}"))?;
        if !e.feed.iter().any(|i| i.id == item && i.actions.iter().any(|a| a.id == action)) {
            return Err(format!("{} doesn't offer {action:?} on that item right now", e.name));
        }
        let rt = e.runtime.as_mut().ok_or_else(|| format!("{} isn't running", e.name))?;
        rt.item_action(item, action)
    }

    /// Notes that a window shows one of a plugin's pages and returns the
    /// cached copy. Plugins without the `projects` capability only have an
    /// overview page, so `project` is ignored for them.
    ///
    /// @param plugin Plugin id.
    /// @param window The showing window's label.
    /// @param project The project picked in that window; `None` for all.
    pub fn open_view(&self, plugin: &str, window: &str, project: Option<String>) -> Result<Option<PageState>, String> {
        if project.as_deref().is_some_and(|p| !protocol::is_project_id(p)) {
            return Err("That isn't a project id".into());
        }
        let mut entries = self.entries.lock().unwrap();
        let e = entries.iter_mut().find(|e| e.id == plugin).ok_or_else(|| format!("No plugin {plugin}"))?;
        if e.tab.is_none() {
            return Err(format!("{} has no tab", e.name));
        }
        let project = project.filter(|_| e.capabilities.contains(&Capability::Projects));
        Ok(e.open_view(window, project))
    }

    /// Notes that a window stopped showing a plugin's page.
    ///
    /// @param plugin Plugin id.
    /// @param window The window's label.
    pub fn close_view(&self, plugin: &str, window: &str) {
        if let Some(e) = self.entries.lock().unwrap().iter_mut().find(|e| e.id == plugin) {
            e.close_view(window);
        }
    }

    /// Closes every page a window showed; called when the window goes away.
    ///
    /// @param window The window's label.
    pub fn close_window(&self, window: &str) {
        for e in self.entries.lock().unwrap().iter_mut() {
            e.close_view(window);
        }
    }

    /// Sends a page's plugin button to its plugin. Only buttons the cached
    /// page offers are sent; task and link buttons never reach the plugin.
    ///
    /// @param plugin Plugin id.
    /// @param project The page's project; `None` for the overview.
    /// @param item The row id, for a row's button.
    /// @param action Button id.
    pub fn page_action(&self, plugin: &str, project: Option<String>, item: Option<&str>, action: &str) -> Result<(), String> {
        let mut entries = self.entries.lock().unwrap();
        let e = entries.iter_mut().find(|e| e.id == plugin).ok_or_else(|| format!("No plugin {plugin}"))?;
        let (button, _) = e.page_button(&project, item, action)?;
        if !matches!(button.kind, ButtonKind::Plugin) {
            return Err(format!("{:?} isn't sent to the plugin", button.label));
        }
        let rt = e.runtime.as_mut().ok_or_else(|| format!("{} isn't running", e.name))?;
        rt.page_action(project.as_deref(), item, action)
    }

    /// A page button as the plugin last sent it, the row it sits on and the
    /// plugin's name, for task buttons.
    ///
    /// @param plugin Plugin id.
    /// @param project The page's project.
    /// @param item The row id, for a row's button.
    /// @param action Button id.
    pub fn page_button(&self, plugin: &str, project: Option<String>, item: Option<&str>, action: &str) -> Result<(String, Button, Option<Row>), String> {
        let entries = self.entries.lock().unwrap();
        let e = entries.iter().find(|e| e.id == plugin).ok_or_else(|| format!("No plugin {plugin}"))?;
        let (button, row) = e.page_button(&project, item, action)?;
        Ok((e.name.clone(), button, row))
    }

    /// Stops every plugin; called when Pill exits.
    pub fn shutdown(&self) {
        for e in self.entries.lock().unwrap().iter_mut() {
            e.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_follow_defaults_until_toggled() {
        let mut s = PluginSettings::default();
        assert!(builtin_enabled(&s, media::ID));
        assert!(!builtin_enabled(&s, notifications::ID));
        s.builtin.insert(media::ID.into(), false);
        s.builtin.insert(notifications::ID.into(), true);
        assert!(!builtin_enabled(&s, media::ID));
        assert!(builtin_enabled(&s, notifications::ID));
        assert!(!builtin_enabled(&s, "unknown"));
    }

    #[test]
    fn externals_need_the_exact_approved_command() {
        let mut s = PluginSettings::default();
        assert!(!external_approved(&s, "x", "node plugin.js"));
        s.external.insert("x".into(), "node plugin.js".into());
        assert!(external_approved(&s, "x", "node plugin.js"));
        assert!(!external_approved(&s, "x", "node evil.js"));
    }

    fn external(json: &str) -> Entry {
        let m = manifest::parse(json, &reserved_ids()).unwrap();
        Entry::new(Kind::External { dir: PathBuf::from("plugin"), manifest: Ok(m) })
    }

    fn feed_line(id: &str) -> Message {
        protocol::parse_line(&format!(r#"{{"type":"feed","id":"{id}","severity":"high","title":"Advisory {id}"}}"#)).unwrap()
    }

    #[derive(Default)]
    struct Recorder(Arc<Mutex<Vec<Vec<ProjectInfo>>>>);

    impl Runtime for Recorder {
        fn action(&mut self, _id: &str) -> Result<(), String> {
            Ok(())
        }
        fn stop(&mut self) {}
        fn projects(&mut self, projects: &[ProjectInfo]) {
            self.0.lock().unwrap().push(projects.to_vec());
        }
    }

    #[test]
    fn feed_items_name_only_known_projects_and_only_with_projects() {
        let dismissed = feed::Dismissed::load(std::env::temp_dir().join(format!("pill-none-feed-projects-{}.json", std::process::id())));
        let list = [ProjectInfo { id: "p_1".into(), name: "Shop".into(), path: PathBuf::from(r"C:\code\shop") }];
        let line = |project: &str| {
            protocol::parse_line(&format!(r#"{{"type":"feed","id":"a","title":"T","project":"{project}","task":{{"title":"Fix it"}}}}"#)).unwrap()
        };
        let mut fed = external(r#"{"id":"f","name":"F","command":"node","capabilities":["feed"]}"#);
        assert!(apply_message(&mut fed, line("p_1"), &dismissed, &list).unwrap_err().contains("projects capability"));
        assert!(apply_message(&mut fed, feed_line("plain"), &dismissed, &list).is_ok(), "items without a project need no projects");
        let mut granted = external(r#"{"id":"g","name":"G","command":"node","capabilities":["feed","projects"]}"#);
        assert!(apply_message(&mut granted, line("p_9"), &dismissed, &list).unwrap_err().contains("isn't one of Pill's projects"));
        apply_message(&mut granted, line("p_1"), &dismissed, &list).unwrap();
        assert_eq!(granted.feed[0].project_id.as_deref(), Some("p_1"));
    }

    #[test]
    fn unchanged_pages_are_not_published_again() {
        let dismissed = feed::Dismissed::load(std::env::temp_dir().join(format!("pill-none-same-page-{}.json", std::process::id())));
        let mut e = tabbed();
        assert!(apply_message(&mut e, page_line("\"p_1\"", "one"), &dismissed, &[]).unwrap().is_some());
        let first = e.pages[0].1.received_at;
        assert!(apply_message(&mut e, page_line("\"p_1\"", "one"), &dismissed, &[]).unwrap().is_none());
        assert_eq!(e.pages[0].1.received_at, first, "the cached copy is what the UI has");
        assert!(apply_message(&mut e, page_line("\"p_1\"", "two"), &dismissed, &[]).unwrap().is_some());
        assert!(apply_message(&mut e, page_line("null", "two"), &dismissed, &[]).unwrap().is_some(), "another project's page is new");
    }

    #[test]
    fn generations_are_never_reused() {
        let mut a = external(r#"{"id":"a","name":"A","command":"node"}"#);
        a.stop();
        let old = a.generation;
        let mut again = external(r#"{"id":"a","name":"A","command":"node"}"#);
        again.stop();
        assert!(again.generation > old, "a recreated entry doesn't take an old runtime's generation");
    }

    #[test]
    fn approvals_name_the_resolved_folder() {
        let root = std::env::temp_dir().join(format!("pill-approval-folder-{}", std::process::id()));
        std::fs::create_dir_all(root.join("one")).unwrap();
        std::fs::create_dir_all(root.join("two")).unwrap();
        let m = manifest::parse(r#"{"id":"x","name":"X","command":"node","args":["plugin.mjs"],"capabilities":["feed","projects"]}"#, &reserved_ids()).unwrap();
        let one = Entry::new(Kind::External { dir: root.join("one"), manifest: Ok(m.clone()) });
        let two = Entry::new(Kind::External { dir: root.join("two"), manifest: Ok(m) });
        let approved = one.approval.clone().unwrap();
        assert!(approved.starts_with("node plugin.mjs\n+projects\n+tasks\n@"));
        assert!(approved.ends_with("one"));
        let mut s = PluginSettings::default();
        s.external.insert("x".into(), approved.clone());
        assert!(external_approved(&s, "x", &approved));
        assert!(!external_approved(&s, "x", two.approval.as_deref().unwrap()), "another folder reusing the id asks again");
        assert_eq!(one.state().folder, one.folder);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn feed_messages_need_the_feed_capability() {
        let dismissed = feed::Dismissed::load(std::env::temp_dir().join(format!("pill-none-{}.json", std::process::id())));
        let mut plain = external(r#"{"id":"a","name":"A","command":"node"}"#);
        assert!(apply_message(&mut plain, feed_line("x"), &dismissed, &[]).unwrap_err().contains("feed"));
        assert!(plain.feed.is_empty());

        let mut fed = external(r#"{"id":"b","name":"B","command":"node","capabilities":["feed"]}"#);
        apply_message(&mut fed, feed_line("x"), &dismissed, &[]).unwrap();
        apply_message(&mut fed, feed_line("x"), &dismissed, &[]).unwrap();
        apply_message(&mut fed, feed_line("y"), &dismissed, &[]).unwrap();
        assert_eq!(fed.feed.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["x", "y"]);
        assert!(apply_message(&mut fed, protocol::parse_line(r#"{"type":"signal","id":"s","icon":"bell","count":1}"#).unwrap(), &dismissed, &[]).is_err());
        apply_message(&mut fed, Message::Feed(FeedChange::Remove("x".into())), &dismissed, &[]).unwrap();
        assert_eq!(fed.feed.len(), 1);
        apply_message(&mut fed, Message::Feed(FeedChange::Clear), &dismissed, &[]).unwrap();
        assert!(fed.feed.is_empty());
    }

    #[test]
    fn feed_items_are_capped_and_dismissals_stick() {
        let path = std::env::temp_dir().join(format!("pill-dismissed-{}", std::process::id())).join("feed-dismissed.json");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let mut dismissed = feed::Dismissed::load(path.clone());
        dismissed.add("b", ["gone".to_string()]).unwrap();
        let mut e = external(r#"{"id":"b","name":"B","command":"node","capabilities":["feed"]}"#);
        apply_message(&mut e, feed_line("gone"), &dismissed, &[]).unwrap();
        assert!(e.feed.is_empty(), "a dismissed id never comes back");
        for i in 0..protocol::MAX_FEED_ITEMS {
            apply_message(&mut e, feed_line(&format!("i{i}")), &dismissed, &[]).unwrap();
        }
        assert!(apply_message(&mut e, feed_line("one-more"), &dismissed, &[]).is_err());
        apply_message(&mut e, feed_line("i0"), &dismissed, &[]).expect("replacing an item is always fine");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Records `view` messages as `open:project` / `close:project`.
    #[derive(Default)]
    struct Views(Arc<Mutex<Vec<String>>>);

    impl Runtime for Views {
        fn action(&mut self, _id: &str) -> Result<(), String> {
            Ok(())
        }
        fn stop(&mut self) {}
        fn view(&mut self, project: Option<&str>, open: bool) {
            self.0.lock().unwrap().push(format!("{}:{}", if open { "open" } else { "close" }, project.unwrap_or("all")));
        }
    }

    fn tabbed() -> Entry {
        external(r#"{"id":"t","name":"T","command":"node","capabilities":["tab","projects"],"tab":{"title":"Security","icon":"shield"}}"#)
    }

    fn page_line(project: &str, text: &str) -> Message {
        protocol::parse_line(&format!(r#"{{"type":"page","project":{project},"page":{{"blocks":[{{"type":"text","text":"{text}"}}]}}}}"#)).unwrap()
    }

    #[test]
    fn views_are_shared_between_windows() {
        let rt = Views::default();
        let seen = rt.0.clone();
        let mut e = tabbed();
        e.runtime = Some(Box::new(rt));
        e.open_view("main", Some("p_1".into()));
        e.open_view("detached", Some("p_1".into()));
        e.open_view("main", Some("p_1".into()));
        e.open_view("main", None);
        e.close_view("detached");
        e.close_view("detached");
        e.close_view("main");
        assert_eq!(*seen.lock().unwrap(), ["open:p_1", "open:all", "close:p_1", "close:all"]);

        e.open_view("main", Some("p_2".into()));
        let restarted = Views::default();
        let again = restarted.0.clone();
        e.stop();
        e.runtime = Some(Box::new(restarted));
        e.resend_views();
        assert_eq!(*again.lock().unwrap(), ["open:p_2"], "a restarted plugin learns what is on screen");
    }

    #[test]
    fn pages_need_the_tab_capability_and_are_cached() {
        let dismissed = feed::Dismissed::load(std::env::temp_dir().join(format!("pill-none-pages-{}.json", std::process::id())));
        let mut plain = external(r#"{"id":"a","name":"A","command":"node","capabilities":["feed"]}"#);
        assert!(apply_message(&mut plain, page_line("null", "x"), &dismissed, &[]).unwrap_err().contains("tab"));
        let mut overview_only =
            external(r#"{"id":"o","name":"O","command":"node","capabilities":["tab"],"tab":{"title":"O","icon":"info"}}"#);
        assert!(apply_message(&mut overview_only, page_line("\"p_1\"", "x"), &dismissed, &[]).unwrap_err().contains("projects"));
        apply_message(&mut overview_only, page_line("null", "x"), &dismissed, &[]).unwrap();

        let mut e = tabbed();
        let event = apply_message(&mut e, page_line("\"p_1\"", "one"), &dismissed, &[]).unwrap().unwrap();
        assert_eq!(event.project.as_deref(), Some("p_1"));
        assert_eq!(e.open_view("main", Some("p_1".into())).unwrap().page, event.state.page);
        for i in 0..page::MAX_CACHED_PAGES + 3 {
            apply_message(&mut e, page_line(&format!("\"p_x{i}\""), "more"), &dismissed, &[]).unwrap();
        }
        assert_eq!(e.pages.len(), page::MAX_CACHED_PAGES);
        assert!(e.pages.iter().any(|(p, _)| p.as_deref() == Some("p_1")), "a page on screen stays cached");
        let nav = protocol::parse_line(r#"{"type":"nav","items":[{"project":"p_1","badge":{"text":"1 critical","tone":"red"}}]}"#).unwrap();
        assert!(apply_message(&mut overview_only, nav.clone(), &dismissed, &[]).unwrap_err().contains("projects"));
        apply_message(&mut e, nav, &dismissed, &[]).unwrap();
        assert_eq!(e.state().nav[0].badge.as_ref().unwrap().text, "1 critical");
        let warned = protocol::parse_line(r#"{"type":"page","project":null,"page":{"blocks":[{"type":"bogus"}]}}"#).unwrap();
        apply_message(&mut e, warned, &dismissed, &[]).unwrap();
        assert!(e.status.message.as_deref().unwrap().contains("overview page"));
    }

    #[test]
    fn page_buttons_must_be_on_the_cached_page() {
        let dismissed = feed::Dismissed::load(std::env::temp_dir().join(format!("pill-none-buttons-{}.json", std::process::id())));
        let mut e = tabbed();
        let line = r#"{"type":"page","project":"p_1","page":{"blocks":[{"type":"list","rows":[{"id":"npm/lodash","title":"lodash","actions":[{"id":"ignore","label":"Ignore"},{"id":"fix","label":"Fix","task":{"title":"Update lodash"}}]}]}]}}"#;
        apply_message(&mut e, protocol::parse_line(line).unwrap(), &dismissed, &[]).unwrap();
        let p1 = Some("p_1".to_string());
        assert!(matches!(e.page_button(&p1, Some("npm/lodash"), "ignore").unwrap().0.kind, ButtonKind::Plugin));
        let (fix, row) = e.page_button(&p1, Some("npm/lodash"), "fix").unwrap();
        assert!(matches!(fix.kind, ButtonKind::Task { .. }));
        assert_eq!(row.unwrap().title, "lodash");
        assert!(e.page_button(&p1, Some("npm/other"), "ignore").is_err());
        assert!(e.page_button(&None, Some("npm/lodash"), "ignore").is_err());
    }

    #[test]
    fn only_plugins_granted_projects_receive_them() {
        let granted = Recorder::default();
        let other = Recorder::default();
        let (seen_granted, seen_other) = (granted.0.clone(), other.0.clone());
        let mut entries = vec![
            external(r#"{"id":"reads","name":"R","command":"node","capabilities":["projects","feed"]}"#),
            external(r#"{"id":"plain","name":"P","command":"node"}"#),
            external(r#"{"id":"stopped","name":"S","command":"node","capabilities":["projects"]}"#),
        ];
        entries[0].runtime = Some(Box::new(granted));
        entries[1].runtime = Some(Box::new(other));
        let list = vec![ProjectInfo { id: "p_1".into(), name: "Shop".into(), path: PathBuf::from(r"C:\code\shop") }];
        send_projects(&mut entries, &list);
        assert_eq!(*seen_granted.lock().unwrap(), vec![list]);
        assert!(seen_other.lock().unwrap().is_empty());
    }

    #[test]
    fn project_list_order_is_stable() {
        let project = |id: &str, name: &str| crate::workspace::Project {
            id: id.into(),
            path: PathBuf::from(format!(r"C:\code\{name}")),
            name: name.into(),
            color: String::new(),
            base_branch: "main".into(),
            settings: Default::default(),
            tasks: vec![],
            notes: String::new(),
            has_remote: false,
            remote: None,
            last_active: 0,
            errors: vec![],
            tasks_locked: false,
            manual: false,
        };
        let snapshot = crate::workspace::Snapshot { projects: vec![project("p_2", "web"), project("p_1", "Api")], inbox: vec![], inbox_error: None };
        assert_eq!(project_list(&snapshot).iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["p_1", "p_2"]);
    }

    #[test]
    fn builtin_ids_are_valid_tokens() {
        for id in reserved_ids() {
            assert!(protocol::is_token(id), "{id}");
        }
    }

    #[test]
    fn plugin_state_serializes_for_the_ui() {
        let state = PluginState {
            id: "x".into(),
            name: "X".into(),
            description: String::new(),
            source: Source::External,
            capabilities: vec![Capability::Signals],
            enabled: true,
            status: Status::with(StatusState::NeedsPermission, "Allow it", Some(Fix::Settings { page: SystemPage::Notifications, label: "Open settings".into() })),
            command: Some("node a.js".into()),
            approval: Some("node a.js".into()),
            folder: None,
            signals: vec![],
            live: Some(LiveItem::Text { icon: None, text: "Hi".into(), tone: protocol::Tone::Accent }),
            widget: None,
            feed: vec![],
            tab: Some(TabSpec { title: "Security".into(), icon: protocol::Icon::Shield }),
            nav: vec![],
            pinned: false,
        };
        let v = serde_json::to_value(&state).unwrap();
        assert_eq!(v["tab"], serde_json::json!({ "title": "Security", "icon": "shield" }));
        assert_eq!(v["status"]["state"], "needsPermission");
        assert_eq!(v["status"]["fix"]["kind"], "settings");
        assert_eq!(v["status"]["fix"]["page"], "notifications");
        assert_eq!(v["live"]["kind"], "text");
        assert_eq!(v["source"], "external");
        assert_eq!(v["pinned"], false);
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn order_drops_gone_and_repeated_ids_and_appends_new_plugins() {
        let known = ids(&["a", "b", "c", "d"]);
        assert_eq!(normalize_order(&ids(&["c", "gone", "a", "c"]), &known), ids(&["c", "a", "b", "d"]));
        assert_eq!(normalize_order(&[], &known), known);
        assert_eq!(normalize_order(&ids(&["x"]), &[]), Vec::<String>::new());
    }

    #[test]
    fn entries_put_builtins_first_then_the_user_order() {
        let ext = |id: &str| external(&format!(r#"{{ "id": "{id}", "name": "{id}", "command": "node" }}"#));
        let builtin = |i: usize| Entry::new(Kind::Builtin(&BUILTINS[i]));
        let mut entries = vec![ext("a"), builtin(0), ext("b"), ext("new"), ext("c"), builtin(1)];
        arrange(&mut entries, &ids(&["c", "gone", "a", "b"]));
        let got: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(got, [BUILTINS[0].id, BUILTINS[1].id, "c", "a", "b", "new"]);
    }
}

/// Keeps the plugin docs honest: `plugins/README.md` (the specification) and
/// `docs/plugins.md` (the walkthrough). Every example in either must parse,
/// and the specification's tables must match the limits and vocabularies in
/// code.
#[cfg(test)]
mod doc_tests {
    use super::page;
    use super::protocol::{self, HostMessage, PageRef, ICON_NAMES};

    const SPEC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins/README.md"));
    const WALKTHROUGH: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/plugins.md"));

    fn blocks(doc: &str, info: &str) -> Vec<String> {
        let fence = format!("```{info}\n");
        doc.split(&fence).skip(1).map(|rest| rest.split("```").next().unwrap_or_default().to_string()).collect()
    }

    fn section<'a>(doc: &'a str, heading: &str) -> &'a str {
        let start = doc.find(heading).unwrap_or_else(|| panic!("missing {heading}"));
        let body = &doc[start + heading.len()..];
        &body[..body.find("\n#").unwrap_or(body.len())]
    }

    fn names(text: &str) -> Vec<String> {
        text.split('`').skip(1).step_by(2).map(str::to_string).collect()
    }

    /// A number as the docs write it: `131072` becomes `131,072`.
    fn thousands(n: impl ToString) -> String {
        let digits = n.to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    }

    fn assert_has(doc: &str, text: &str) {
        assert!(doc.contains(text), "the docs should say {text:?}");
    }

    /// Joins wrapped lines, so a phrase is found wherever Markdown breaks it.
    fn flatten(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Checks every line of the doc's `jsonl` blocks: Pill's own messages
    /// must be exactly what Pill writes, and plugin messages must parse.
    fn check_examples(doc: &str, at_least: usize) {
        let lines: Vec<String> = blocks(doc, "jsonl").iter().flat_map(|b| b.lines().map(str::to_string).collect::<Vec<_>>()).collect();
        assert!(lines.len() >= at_least);
        let hello = HostMessage::Hello { protocol: protocol::PROTOCOL_VERSION, pill: env!("CARGO_PKG_VERSION") }.to_line();
        for line in lines.iter().filter(|l| !l.trim().is_empty()) {
            let v: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}"));
            let expected = match v["type"].as_str() {
                Some("hello") => Some(hello.clone()),
                Some("action") => Some(
                    HostMessage::Action {
                        id: v["id"].as_str().unwrap(),
                        item: v["item"].as_str(),
                        page: v.get("page").map(|p| PageRef { project: p["project"].as_str() }),
                    }
                    .to_line(),
                ),
                Some("view") => Some(
                    HostMessage::View { project: v["project"].as_str(), closed: v["closed"].as_bool().unwrap_or(false) }.to_line(),
                ),
                Some("projects") => {
                    let projects: Vec<super::ProjectInfo> = v["projects"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| super::ProjectInfo {
                            id: p["id"].as_str().unwrap().into(),
                            name: p["name"].as_str().unwrap().into(),
                            path: p["path"].as_str().unwrap().into(),
                        })
                        .collect();
                    Some(HostMessage::Projects { projects: &projects }.to_line())
                }
                _ => None,
            };
            match expected {
                Some(host) => assert_eq!(format!("{line}\n"), host),
                None => {
                    protocol::parse_line(line).unwrap_or_else(|e| panic!("{line}: {e}"));
                }
            }
        }
    }

    #[test]
    fn every_example_line_is_valid() {
        check_examples(SPEC, 20);
        check_examples(WALKTHROUGH, 8);
    }

    #[test]
    fn the_example_manifests_are_valid_and_match_cve_watch() {
        let manifests = blocks(WALKTHROUGH, "json plugin.json");
        assert_eq!(manifests.len(), 2);
        let shipped = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins/cve-watch/plugin.json"));
        let cve = super::manifest::parse(&manifests[0], &super::reserved_ids()).unwrap();
        assert_eq!(cve, super::manifest::parse(shipped, &super::reserved_ids()).unwrap(), "the walkthrough shows the shipped manifest");
        assert_eq!(cve.approval(), "node plugin.mjs\n+projects\n+tasks");
        let pomodoro = super::manifest::parse(&manifests[1], &super::reserved_ids()).unwrap();
        assert_eq!(pomodoro.command_line(), "node plugin.js");

        let hello = blocks(SPEC, "json plugin.json");
        assert_eq!(hello.len(), 1);
        let hello = super::manifest::parse(&hello[0], &super::reserved_ids()).unwrap();
        assert_eq!(hello.approval(), "node plugin.mjs", "hello world asks for no access");
        assert_has(SPEC, &format!("```text\n{}\n@", cve.approval()));
    }

    #[test]
    fn icons_tones_and_severities_match_the_code() {
        assert_eq!(names(section(SPEC, "### Icons")), ICON_NAMES.to_vec());
        let tones = names(section(SPEC, "### Tones"));
        assert_eq!(tones, vec!["neutral", "accent", "green", "amber", "red", "purple"]);
        for tone in tones {
            assert!(serde_json::from_value::<protocol::Tone>(serde_json::Value::String(tone)).is_ok());
        }
        let severities = names(section(SPEC, "### Severities"));
        assert_eq!(severities, vec!["critical", "high", "medium", "low", "info"]);
        for s in severities {
            assert!(serde_json::from_value::<protocol::Severity>(serde_json::Value::String(s)).is_ok());
        }
        for reserved in super::reserved_ids() {
            assert_has(SPEC, &format!("`{reserved}`"));
        }
    }

    #[test]
    fn the_limits_table_matches_the_code() {
        use super::{feed, manifest};
        let rows = [
            format!("| Manifest size | {} KB |", manifest::MAX_MANIFEST_BYTES / 1024),
            format!("| Plugin folders | {} |", manifest::MAX_PLUGINS),
            format!("| Line length | {} KB |", protocol::MAX_LINE_BYTES / 1024),
            format!("| Page line length | {} KB |", page::MAX_PAGE_LINE_BYTES / 1024),
            format!("| Rate | {} lines per second |", super::external::MAX_LINES_PER_SEC),
            format!("| Signals | {} per plugin |", protocol::MAX_SIGNALS),
            format!("| Widget rows / buttons | {} / {} |", protocol::MAX_ROWS, protocol::MAX_ACTIONS),
            format!("| Feed items | {} per plugin |", protocol::MAX_FEED_ITEMS),
            format!("| Feed item buttons | {} |", protocol::MAX_ITEM_ACTIONS),
            format!("| Dismissed feed ids | {} per plugin |", feed::MAX_DISMISSED),
            format!("| Blocks | {} per page, sections included |", page::MAX_BLOCKS),
            format!("| Rows | {} per list or table, {} per page |", page::MAX_LIST_ROWS, page::MAX_PAGE_ROWS),
            format!("| Block and row buttons | {} |", page::MAX_BLOCK_ACTIONS),
            format!("| Sidebar entries | {} per `nav` |", page::MAX_NAV_ITEMS),
            format!("| Cached pages | {} per plugin |", page::MAX_CACHED_PAGES),
        ];
        let table = section(SPEC, "## Limits");
        for row in &rows {
            assert_has(table, row);
        }
    }

    #[test]
    fn text_limits_match_the_code() {
        let summary = [
            format!("live text {}", protocol::MAX_LIVE_TEXT),
            format!("signal label {}", protocol::MAX_LABEL),
            format!("widget title {}", protocol::MAX_TITLE),
            format!("widget row {}", protocol::MAX_ROW_TEXT),
            format!("widget button label {}", protocol::MAX_ACTION_LABEL),
            format!("feed title {}", protocol::MAX_FEED_TITLE),
            format!("feed body {}", protocol::MAX_FEED_BODY),
            format!("feed detail {}", protocol::MAX_FEED_DETAIL),
            format!("task title {}", protocol::MAX_TASK_TITLE),
            format!("task notes {}", thousands(protocol::MAX_TASK_NOTES)),
            format!("page button label {}", page::MAX_BUTTON_LABEL),
            format!("error {}", protocol::MAX_ERROR),
            format!("badge {}", page::MAX_BADGE),
            format!("tab title {}", page::MAX_TAB_TITLE),
        ];
        let limits = flatten(section(SPEC, "## Limits"));
        for phrase in &summary {
            assert_has(&limits, phrase);
        }

        let details = [
            format!("at most **8 KB** ({} bytes", thousands(protocol::MAX_LINE_BYTES)),
            format!("up to **128 KB** ({}", thousands(page::MAX_PAGE_LINE_BYTES)),
            format!("(limit {} per second)", super::external::MAX_LINES_PER_SEC),
            format!("the limit is {}`", protocol::MAX_LINE_BYTES),
            format!("(limit {})`", page::MAX_PAGE_LINE_BYTES),
            format!("1–{} bytes of `A-Z`", protocol::MAX_ITEM_ID),
            format!("at most {} bytes", thousands(protocol::MAX_URL)),
            format!("Values above {} show as {}", thousands(protocol::MAX_COUNT), thousands(protocol::MAX_COUNT)),
            format!("Text is cut to {} characters", protocol::MAX_LIVE_TEXT),
            format!("last {} dismissed ids", thousands(super::feed::MAX_DISMISSED)),
            format!("`text` ({}, required", thousands(page::MAX_TEXT)),
            format!("| `body` | string | Multi-line, {}.", thousands(page::MAX_ROW_BODY)),
            format!("`items`: 1–{} tiles", page::MAX_STATS),
            format!("`items`: 1–{} of `label`", page::MAX_PAIRS),
            format!("`columns`: 1–{} of", page::MAX_COLUMNS),
            format!("Up to {} of `{{\"text\"", page::MAX_CHIPS),
            format!("Pill caches up to {} pages", page::MAX_CACHED_PAGES),
        ];
        let flat = flatten(SPEC);
        for phrase in &details {
            assert_has(&flat, phrase);
        }
    }
}
