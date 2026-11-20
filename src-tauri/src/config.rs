//! Global settings stored at `%APPDATA%\Pill\config.json`.
//!
//! Responsibilities:
//! - Define the on-disk schema with defaults for every field, so older or
//!   hand-edited files keep loading as new settings are added.
//! - Load, validate and atomically save the file.
//! - Never overwrite a file the user broke by hand; fall back to defaults in
//!   memory and report the parse error instead.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Current schema version written to `config.json`.
pub const CONFIG_VERSION: u32 = 1;

/// Most automatic fix turns a run may take; more mostly burns usage.
pub const MAX_FIX_ATTEMPTS: u32 = 3;

/// Limits applied to headless Claude Code runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentLimits {
    /// How many agent runs may execute at the same time.
    pub max_parallel_runs: u32,
    /// `--max-turns` passed to Claude Code.
    pub max_turns: u32,
    /// Wall-clock limit per run before Pill kills it.
    pub timeout_minutes: u32,
    /// Follow-up turns a run gets when its tests or lint fail: the failing
    /// output goes back to the same session before the run gives up.
    pub fix_attempts: u32,
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self { max_parallel_runs: 2, max_turns: 80, timeout_minutes: 45, fix_attempts: 1 }
    }
}

/// How the notch and the detached window are painted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Pure black, like a hardware notch.
    Black,
    /// Soft dark gray.
    Graphite,
    /// The wallpaper behind each surface, blurred and tinted like Mica.
    #[default]
    Glass,
}

/// Light or dark palette. Independent of [`Theme`], so every surface style
/// comes in both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    /// Light text on dark surfaces; what Pill has always looked like.
    #[default]
    Dark,
    /// Dark text on light surfaces.
    Light,
    /// Follows the Windows app mode (Personalization > Colors).
    System,
}

/// What opens the panel from the notch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenOn {
    /// Resting the cursor on the notch.
    #[default]
    Hover,
    /// Clicking the notch; leaving still closes it.
    Click,
}

/// What the glass theme blurs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GlassSource {
    /// The wallpaper, unless it's too dark or flat for glass to show.
    #[default]
    Auto,
    /// Always the desktop wallpaper.
    Wallpaper,
    /// Always one of Pill's gradients, see [`Config::glass_gradient`].
    Pill,
    /// A picture on disk, see [`Config::glass_image`].
    Image,
}

/// Ids of Pill's gradient backdrops. The UI paints them; the core only
/// validates the choice.
pub const GLASS_GRADIENTS: &[&str] = &["blue", "violet", "ember", "forest", "graphite", "dusk"];

/// How dark the glass tint is; lighter shows more wallpaper.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GlassTint {
    Light,
    #[default]
    Medium,
    Dark,
}

/// When hands-off tasks start automatically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Autopilot {
    /// Start hands-off tasks without a click.
    pub enabled: bool,
    /// Optional local start of the allowed window, `HH:MM` (e.g. `22:00`).
    pub window_start: Option<String>,
    /// Optional local end of the allowed window, `HH:MM`; may be past midnight.
    pub window_end: Option<String>,
}

impl Default for Autopilot {
    fn default() -> Self {
        Self { enabled: true, window_start: None, window_end: None }
    }
}

impl Autopilot {
    /// Whether automatic runs may start at `minutes` past local midnight.
    pub fn allows(&self, minutes: u32) -> bool {
        if !self.enabled {
            return false;
        }
        let parse = |s: &Option<String>| {
            let (h, m) = s.as_deref()?.split_once(':')?;
            Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
        };
        match (parse(&self.window_start), parse(&self.window_end)) {
            (Some(s), Some(e)) if s <= e => (s..e).contains(&minutes),
            // Overnight window, e.g. 22:00 to 07:00.
            (Some(s), Some(e)) => minutes >= s || minutes < e,
            _ => true,
        }
    }
}

/// Which plugins run, and how they're arranged. Built-ins not listed here use
/// their own default; external plugins never run unless listed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginSettings {
    /// Built-in plugin id to on/off, set when the user flips a toggle.
    pub builtin: std::collections::BTreeMap<String, bool>,
    /// External plugin id to the exact command line the user approved. A
    /// plugin whose manifest now runs something else stays off.
    pub external: std::collections::BTreeMap<String, String>,
    /// External plugin ids in the order the user arranged them in Settings.
    /// Every plugin list follows it: plugins missing here come last, ids of
    /// plugins that are gone are ignored (see [`crate::plugins::normalize_order`]).
    pub order: Vec<String>,
    /// Tab plugins kept out of the header. Their tabs still open from the
    /// header's overflow menu.
    pub menu_tabs: std::collections::BTreeSet<String>,
}

/// The user's global Pill settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Schema version, see [`CONFIG_VERSION`].
    pub version: u32,
    /// Folders (or whole drives) scanned for git repositories.
    pub roots: Vec<PathBuf>,
    /// Project folders added by hand, outside any root.
    pub projects: Vec<PathBuf>,
    /// Global quick-add accelerator, in Tauri accelerator syntax.
    pub hotkey: String,
    /// Index into the OS monitor list; `None` follows the primary monitor.
    pub monitor: Option<usize>,
    /// Register Pill to start at Windows sign-in.
    pub launch_at_startup: bool,
    /// Accent color as `#RRGGBB`.
    pub accent: String,
    /// How long the cursor must rest on the pill before it opens.
    pub hover_open_delay_ms: u32,
    /// How long after the cursor leaves before the panel closes.
    pub hover_close_delay_ms: u32,
    /// Limits for agent runs.
    pub agent: AgentLimits,
    /// Automatic runs for hands-off tasks.
    pub autopilot: Autopilot,
    /// Default model and effort per provider; projects and chats can override.
    pub models: crate::models::ModelDefaults,
    /// Surface style for the notch and the detached window.
    pub theme: Theme,
    /// Light or dark palette for every surface style. A value this version
    /// doesn't know falls back to the default instead of failing the file.
    #[serde(deserialize_with = "lenient")]
    pub appearance: Appearance,
    /// Tint strength for the glass theme.
    pub glass_tint: GlassTint,
    /// Whether hovering or clicking opens the panel.
    pub open_on: OpenOn,
    /// Keep the notch above other windows. When off, windows that reach the
    /// top of the screen cover it; the hotkey still brings it forward.
    pub on_top: bool,
    /// What the glass theme blurs.
    pub glass_source: GlassSource,
    /// Gradient id from [`GLASS_GRADIENTS`], used by [`GlassSource::Pill`]
    /// and by [`GlassSource::Auto`] when the wallpaper can't carry glass.
    pub glass_gradient: String,
    /// JPEG or PNG used by [`GlassSource::Image`]. Kept when another source
    /// is picked, so switching back doesn't need the file dialog again.
    pub glass_image: Option<PathBuf>,
    /// Plugin toggles and approvals.
    pub plugins: PluginSettings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            roots: Vec::new(),
            projects: Vec::new(),
            hotkey: "Ctrl+Shift+Space".into(),
            monitor: None,
            launch_at_startup: false,
            accent: "#2F7BFF".into(),
            hover_open_delay_ms: 120,
            hover_close_delay_ms: 350,
            agent: AgentLimits::default(),
            autopilot: Autopilot::default(),
            models: crate::models::ModelDefaults::default(),
            theme: Theme::default(),
            appearance: Appearance::default(),
            glass_tint: GlassTint::default(),
            open_on: OpenOn::default(),
            on_top: true,
            glass_source: GlassSource::default(),
            glass_gradient: GLASS_GRADIENTS[0].into(),
            glass_image: None,
            plugins: PluginSettings::default(),
        }
    }
}

impl Config {
    /// Clamps values a hand-edited file could set to something unusable.
    pub fn sanitized(mut self) -> Self {
        self.hover_open_delay_ms = self.hover_open_delay_ms.min(2_000);
        self.hover_close_delay_ms = self.hover_close_delay_ms.clamp(50, 5_000);
        self.agent.max_parallel_runs = self.agent.max_parallel_runs.clamp(1, 8);
        self.agent.max_turns = self.agent.max_turns.clamp(1, 500);
        self.agent.timeout_minutes = self.agent.timeout_minutes.clamp(1, 24 * 60);
        self.agent.fix_attempts = self.agent.fix_attempts.min(MAX_FIX_ATTEMPTS);
        if !is_hex_color(&self.accent) {
            self.accent = Config::default().accent;
        }
        self.models = self.models.sanitized();
        if !GLASS_GRADIENTS.contains(&self.glass_gradient.as_str()) {
            self.glass_gradient = Config::default().glass_gradient;
        }
        self
    }
}

/// Deserializes `T`, or its default when the value doesn't fit, e.g. a
/// setting written by a newer Pill or mistyped by hand.
fn lenient<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Result of loading the config file.
#[derive(Debug)]
pub struct Loaded {
    /// The effective config (defaults if the file was unreadable).
    pub config: Config,
    /// Set when the file exists but could not be parsed.
    pub error: Option<String>,
}

/// Loads `config.json` from `path`, creating it with defaults if it doesn't
/// exist. Returns the effective config, clamped by [`Config::sanitized`],
/// plus any parse error.
pub fn load(path: &Path) -> Loaded {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Config>(&text) {
            Ok(cfg) => Loaded { config: cfg.sanitized(), error: None },
            Err(e) => Loaded {
                config: Config::default(),
                error: Some(format!("{}: {e}", path.display())),
            },
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let config = Config::default();
            let error = save(path, &config).err().map(|e| e.to_string());
            Loaded { config, error }
        }
        Err(e) => Loaded { config: Config::default(), error: Some(e.to_string()) },
    }
}

/// Writes `config` to `config.json` at `path` atomically (temp file +
/// rename).
pub fn save(path: &Path, config: &Config) -> std::io::Result<()> {
    write_atomic(path, serde_json::to_string_pretty(config)?.as_bytes())
}

/// Writes the full file contents `bytes` to `path` via a sibling temp file
/// and rename, so readers (and file watchers) never observe a half-written
/// file. Parent directories are created.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let cfg: Config = serde_json::from_str(r##"{ "accent": "#112233" }"##).unwrap();
        assert_eq!(cfg.accent, "#112233");
        assert_eq!(cfg.hover_open_delay_ms, 120);
        assert_eq!(cfg.agent.max_parallel_runs, 2);
    }

    #[test]
    fn old_glass_settings_still_load() {
        let cfg: Config = serde_json::from_str(r#"{ "theme": "glass", "glassSource": "pill" }"#).unwrap();
        assert_eq!(cfg.glass_source, GlassSource::Pill);
        assert_eq!(cfg.glass_gradient, "blue");
        assert_eq!(cfg.glass_image, None);
    }

    #[test]
    fn glass_image_round_trips_and_bad_gradients_reset() {
        let cfg: Config = serde_json::from_str(
            r#"{ "glassSource": "image", "glassImage": "C:\\pics\\a.png", "glassGradient": "neon" }"#,
        )
        .unwrap();
        assert_eq!(cfg.glass_source, GlassSource::Image);
        assert_eq!(cfg.glass_image.as_deref(), Some(Path::new("C:\\pics\\a.png")));
        assert_eq!(cfg.sanitized().glass_gradient, "blue");
    }

    #[test]
    fn appearance_defaults_to_dark_and_tolerates_bad_values() {
        let cfg: Config = serde_json::from_str(r#"{ "theme": "graphite" }"#).unwrap();
        assert_eq!(cfg.appearance, Appearance::Dark);
        assert_eq!(cfg.theme, Theme::Graphite);
        for (raw, want) in [("light", Appearance::Light), ("system", Appearance::System), ("dark", Appearance::Dark)] {
            let cfg: Config = serde_json::from_str(&format!(r#"{{ "appearance": "{raw}" }}"#)).unwrap();
            assert_eq!(cfg.appearance, want);
        }
        let cfg: Config = serde_json::from_str(r##"{ "appearance": "sepia", "accent": "#112233" }"##).unwrap();
        assert_eq!(cfg.appearance, Appearance::Dark);
        assert_eq!(cfg.accent, "#112233");
        let cfg: Config = serde_json::from_str(r#"{ "appearance": 3 }"#).unwrap();
        assert_eq!(cfg.appearance, Appearance::Dark);
        let json = serde_json::to_string(&Config { appearance: Appearance::System, ..Config::default() }).unwrap();
        assert!(json.contains(r#""appearance":"system""#));
    }

    #[test]
    fn plugin_settings_default_to_nothing_approved() {
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.plugins.builtin.is_empty());
        assert!(cfg.plugins.external.is_empty());
        let cfg: Config = serde_json::from_str(r#"{ "plugins": { "builtin": { "now-playing": false } } }"#).unwrap();
        assert_eq!(cfg.plugins.builtin.get("now-playing"), Some(&false));
        assert!(cfg.plugins.external.is_empty());
        let round: Config = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(round.plugins, cfg.plugins);
    }

    #[test]
    fn plugin_order_and_menu_tabs_load_from_old_and_new_files() {
        let old: Config = serde_json::from_str(r#"{ "plugins": { "external": { "a": "node a.mjs" } } }"#).unwrap();
        assert!(old.plugins.order.is_empty());
        assert!(old.plugins.menu_tabs.is_empty());
        let cfg: Config = serde_json::from_str(r#"{ "plugins": { "order": ["b", "a"], "menuTabs": ["a"] } }"#).unwrap();
        assert_eq!(cfg.plugins.order, ["b", "a"]);
        assert!(cfg.plugins.menu_tabs.contains("a"));
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains(r#""order":["b","a"]"#) && json.contains(r#""menuTabs":["a"]"#));
    }

    #[test]
    fn autopilot_windows_include_overnight() {
        let mut a = Autopilot::default();
        assert!(a.allows(12 * 60));
        a.window_start = Some("22:00".into());
        a.window_end = Some("07:00".into());
        assert!(a.allows(23 * 60));
        assert!(a.allows(3 * 60));
        assert!(!a.allows(12 * 60));
        a.window_start = Some("09:00".into());
        a.window_end = Some("17:00".into());
        assert!(a.allows(9 * 60));
        assert!(!a.allows(17 * 60));
        a.enabled = false;
        assert!(!a.allows(10 * 60));
    }

    #[test]
    fn fix_attempts_default_to_one_and_are_capped() {
        let cfg: Config = serde_json::from_str(r#"{ "agent": { "maxParallelRuns": 3 } }"#).unwrap();
        assert_eq!(cfg.agent.fix_attempts, 1);
        let mut cfg = Config::default();
        cfg.agent.fix_attempts = 40;
        assert_eq!(cfg.sanitized().agent.fix_attempts, MAX_FIX_ATTEMPTS);
    }

    #[test]
    fn sanitize_clamps_and_rejects_bad_accent() {
        let cfg = Config { accent: "blue".into(), ..Config::default() }.sanitized();
        assert_eq!(cfg.accent, "#2F7BFF");
    }

    #[test]
    fn zero_agent_limits_load_as_one() {
        let json = r#"{ "agent": { "maxParallelRuns": 0, "maxTurns": 0, "timeoutMinutes": 0 } }"#;
        let dir = std::env::temp_dir().join(format!("pill-cfg-zero-{}", std::process::id()));
        let path = dir.join("config.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, json).unwrap();
        let agent = load(&path).config.agent;
        assert_eq!((agent.max_parallel_runs, agent.max_turns, agent.timeout_minutes), (1, 1, 1));
        fs::remove_dir_all(&dir).unwrap();
        // update_settings goes through the same clamp (AppState::update_config).
        let patched = Config { agent: AgentLimits { max_parallel_runs: 0, max_turns: 0, timeout_minutes: 0, fix_attempts: 0 }, ..Config::default() };
        let agent = patched.sanitized().agent;
        assert_eq!((agent.max_parallel_runs, agent.max_turns, agent.timeout_minutes), (1, 1, 1));
    }

    #[test]
    fn load_creates_file_then_round_trips() {
        let dir = std::env::temp_dir().join(format!("pill-cfg-{}", std::process::id()));
        let path = dir.join("config.json");
        let _ = fs::remove_dir_all(&dir);
        let first = load(&path);
        assert!(first.error.is_none());
        assert!(path.exists());
        let second = load(&path);
        assert_eq!(first.config, second.config);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn broken_file_is_reported_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("pill-cfg-bad-{}", std::process::id()));
        let path = dir.join("config.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "{ nope").unwrap();
        let loaded = load(&path);
        assert!(loaded.error.is_some());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ nope");
        fs::remove_dir_all(&dir).unwrap();
    }
}
