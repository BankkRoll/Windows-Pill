//! External plugin manifests (`plugin.json`) and the plugins folder.
//!
//! Responsibilities:
//! - Find plugin folders under `%APPDATA%\Pill\plugins`.
//! - Parse and validate each manifest, keeping a readable error for broken
//!   ones so Settings can show what to fix.
//! - Render what the user approves: the exact command line, the access and
//!   task capabilities it asks for, and the resolved plugin folder. That
//!   text doubles as the approval fingerprint: if any part changes, Pill
//!   asks again.

use super::page::{self, TabSpec};
use super::protocol::{Capability, Icon};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Name of the manifest file inside each plugin folder.
pub const MANIFEST_FILE: &str = "plugin.json";
/// Largest manifest Pill reads.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// Plugin folders scanned; more are ignored.
pub const MAX_PLUGINS: usize = 32;
const MAX_ARGS: usize = 32;
const MAX_ARG_LEN: usize = 1024;
const MAX_COMMAND_LEN: usize = 260;

/// A validated `plugin.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    /// 1–32 characters of `a-z`, `0-9`, `-`, `_`; unique across plugins.
    pub id: String,
    pub name: String,
    pub description: String,
    /// Program to run: a file relative to the plugin folder, or a name on PATH.
    pub command: String,
    pub args: Vec<String>,
    /// What the plugin may send. Defaults to `signals`, `live` and `widget`.
    pub capabilities: Vec<Capability>,
    /// The plugin's tab; present exactly when `capabilities` has `tab`.
    pub tab: Option<TabSpec>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManifest {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    capabilities: Option<Vec<Capability>>,
    #[serde(default)]
    tab: Option<RawTab>,
}

#[derive(Deserialize)]
struct RawTab {
    title: String,
    icon: Icon,
}

/// A plugin folder and what was found in it.
#[derive(Debug, Clone)]
pub struct Found {
    /// The plugin folder as found under the plugins folder (not resolved;
    /// see [`canonical_folder`]).
    pub dir: PathBuf,
    /// The validated manifest, or why `plugin.json` was refused.
    pub manifest: Result<Manifest, String>,
}

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// Parses and validates manifest JSON.
///
/// @param text Contents of `plugin.json`.
/// @param reserved Ids taken by built-in plugins.
/// @returns The manifest, or a message naming the first problem.
pub fn parse(text: &str, reserved: &[&str]) -> Result<Manifest, String> {
    let raw: RawManifest = serde_json::from_str(text).map_err(|e| format!("{MANIFEST_FILE} isn't valid: {e}"))?;
    if !super::protocol::is_token(&raw.id) {
        return Err(format!("id {:?} must be 1-32 characters of a-z, 0-9, - or _", raw.id));
    }
    if reserved.contains(&raw.id.as_str()) {
        return Err(format!("id {:?} belongs to a built-in plugin", raw.id));
    }
    let name = raw.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 40 || has_control(&name) {
        return Err("name must be 1-40 characters on one line".into());
    }
    let description = raw.description.trim().to_string();
    if description.chars().count() > 160 || has_control(&description) {
        return Err("description must be at most 160 characters on one line".into());
    }
    let command = raw.command.trim().to_string();
    if command.is_empty() || command.len() > MAX_COMMAND_LEN || has_control(&command) {
        return Err(format!("command must be 1-{MAX_COMMAND_LEN} characters"));
    }
    if raw.args.len() > MAX_ARGS {
        return Err(format!("at most {MAX_ARGS} args"));
    }
    if raw.args.iter().any(|a| a.len() > MAX_ARG_LEN || a.contains('\0')) {
        return Err(format!("each arg must be under {MAX_ARG_LEN} bytes"));
    }
    let mut capabilities: Vec<Capability> = Vec::new();
    for cap in raw.capabilities.unwrap_or_else(|| vec![Capability::Signals, Capability::Live, Capability::Widget]) {
        if !capabilities.contains(&cap) {
            capabilities.push(cap);
        }
    }
    let tab = match (raw.tab, capabilities.contains(&Capability::Tab)) {
        (Some(t), true) => Some(page::tab_spec(&t.title, t.icon)?),
        (None, false) => None,
        (Some(_), false) => return Err("\"tab\" needs the tab capability".into()),
        (None, true) => return Err("the tab capability needs \"tab\": { \"title\": …, \"icon\": … }".into()),
    };
    Ok(Manifest { id: raw.id, name, description, command, args: raw.args, capabilities, tab })
}

/// Reads one plugin folder's manifest.
///
/// @param dir Plugin folder.
/// @param reserved Ids taken by built-in plugins.
pub fn read(dir: &Path, reserved: &[&str]) -> Result<Manifest, String> {
    let path = dir.join(MANIFEST_FILE);
    let meta = fs::metadata(&path).map_err(|e| format!("Couldn't read {MANIFEST_FILE}: {e}"))?;
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err(format!("{MANIFEST_FILE} is larger than {} KB", MAX_MANIFEST_BYTES / 1024));
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("Couldn't read {MANIFEST_FILE}: {e}"))?;
    parse(text.trim_start_matches('\u{feff}'), reserved)
}

/// Lists plugin folders (those containing `plugin.json`) sorted by name, and
/// marks later duplicates of an id as errors.
///
/// @param root The plugins folder; created if missing.
/// @param reserved Ids taken by built-in plugins.
pub fn discover(root: &Path, reserved: &[&str]) -> Vec<Found> {
    if let Err(e) = fs::create_dir_all(root) {
        log::warn!("plugins folder {}: {e}", root.display());
        return Vec::new();
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .map(|it| {
            it.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir() && p.join(MANIFEST_FILE).is_file())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs.truncate(MAX_PLUGINS);
    let mut seen: Vec<String> = Vec::new();
    dirs.into_iter()
        .map(|dir| {
            let manifest = read(&dir, reserved).and_then(|m| {
                if seen.contains(&m.id) {
                    Err(format!("another plugin already uses the id {:?}", m.id))
                } else {
                    seen.push(m.id.clone());
                    Ok(m)
                }
            });
            Found { dir, manifest }
        })
        .collect()
}

/// A plugin folder with junctions and links resolved, as approvals name it
/// and as the plugin runs from; `dir` itself when it can't be resolved.
///
/// @param dir A folder from [`discover`].
pub fn canonical_folder(dir: &Path) -> PathBuf {
    dir.canonicalize().map(strip_verbatim).unwrap_or_else(|_| dir.to_path_buf())
}

/// COMPAT: `canonicalize` returns `\\?\C:\…` paths, which `cmd.exe` can't
/// use as a script path; plain drive paths work everywhere.
pub(crate) fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p,
    }
}

/// Quotes one argument the way Windows command lines parse it.
fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|c| c.is_whitespace() || c == '"') {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

impl Manifest {
    /// The command line exactly as the user sees and approves it.
    ///
    /// @example `node plugin.js --interval "5 s"`
    pub fn command_line(&self) -> String {
        std::iter::once(&self.command).chain(&self.args).map(|a| quote(a)).collect::<Vec<_>>().join(" ")
    }

    /// What the manifest asks the user to approve: the command line, then
    /// each capability that gives the plugin access to something (see
    /// [`Capability::grants_access`]) on its own line, then `+tasks` when it
    /// may suggest tasks (see [`Capability::suggests_tasks`]). Plugins with
    /// neither approve the bare command line.
    ///
    /// SECURITY: a manifest that later asks for `projects`, `feed` or `tab`
    /// no longer matches the stored approval, so the plugin stops until the
    /// user approves it again. The line breaks can't come from the command
    /// line itself, because [`quote`] quotes any argument containing
    /// whitespace.
    ///
    /// @example `node plugin.js\n+projects\n+tasks`
    pub fn approval(&self) -> String {
        let mut out = self.command_line();
        for cap in self.capabilities.iter().filter(|c| c.grants_access()) {
            let name = serde_json::to_value(cap).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            out.push_str(&format!("\n+{name}"));
        }
        if self.capabilities.iter().any(|c| c.suggests_tasks()) {
            out.push_str("\n+tasks");
        }
        out
    }

    /// The full approval Pill stores: [`Manifest::approval`] plus the plugin
    /// folder on a last line starting with `@`.
    ///
    /// SECURITY: without the folder, another folder whose manifest reuses an
    /// approved id and command would start without asking. Windows paths
    /// can't hold line breaks, so the folder can't fake another line.
    ///
    /// @param folder The plugin folder, as [`canonical_folder`] resolves it.
    /// @example `node plugin.js\n+projects\n+tasks\n@C:\Users\me\AppData\Roaming\Pill\plugins\cve-watch`
    pub fn approval_in(&self, folder: &Path) -> String {
        format!("{}\n@{}", self.approval(), folder.display())
    }

    /// Resolves the program: paths are taken relative to the plugin folder
    /// and must stay inside it; bare names are looked up on PATH.
    ///
    /// @param dir The plugin folder.
    pub fn resolve_program(&self, dir: &Path) -> Result<PathBuf, String> {
        let cmd = Path::new(&self.command);
        let is_path = cmd.is_absolute() || self.command.contains(['/', '\\']);
        if is_path {
            let full = if cmd.is_absolute() { cmd.to_path_buf() } else { dir.join(cmd) };
            let full = full.canonicalize().map_err(|_| format!("{} doesn't exist", full.display()))?;
            if !cmd.is_absolute() {
                let base = dir.canonicalize().map_err(|e| e.to_string())?;
                // SECURITY: `..\..\something.exe` would run a program outside
                // the folder the user looked at.
                if !full.starts_with(&base) {
                    return Err(format!("{} is outside the plugin folder", self.command));
                }
            }
            return Ok(full);
        }
        crate::process::which(&self.command).ok_or_else(|| format!("{} isn't installed or isn't on PATH", self.command))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESERVED: &[&str] = &["now-playing", "notifications"];

    #[test]
    fn parses_a_full_manifest() {
        let m = parse(
            r#"{"id":"build-status","name":"Build status","description":"CI at a glance","command":"node","args":["plugin.js"],"capabilities":["signals","widget"]}"#,
            RESERVED,
        )
        .unwrap();
        assert_eq!(m.id, "build-status");
        assert_eq!(m.capabilities, vec![Capability::Signals, Capability::Widget]);
        assert_eq!(m.command_line(), "node plugin.js");
    }

    #[test]
    fn approval_names_access_and_tasks() {
        let plain = parse(r#"{"id":"a","name":"A","command":"node","args":["plugin.js"],"capabilities":["live","widget"]}"#, RESERVED).unwrap();
        assert_eq!(plain.approval(), "node plugin.js");
        let fed = parse(r#"{"id":"a","name":"A","command":"node","args":["plugin.js"],"capabilities":["feed","widget"]}"#, RESERVED).unwrap();
        assert_eq!(fed.approval(), "node plugin.js\n+tasks", "feed items can carry tasks");
        let reads = parse(r#"{"id":"a","name":"A","command":"node","args":["plugin.js"],"capabilities":["feed","projects","feed"]}"#, RESERVED).unwrap();
        assert_eq!(reads.approval(), "node plugin.js\n+projects\n+tasks");
        let lookalike = parse(r#"{"id":"a","name":"A","command":"node","args":["plugin.js\n+projects"],"capabilities":["feed"]}"#, RESERVED).unwrap();
        assert_ne!(lookalike.approval(), reads.approval());
        let folder = Path::new(r"C:\Users\me\AppData\Roaming\Pill\plugins\a");
        assert_eq!(reads.approval_in(folder), "node plugin.js\n+projects\n+tasks\n@C:\\Users\\me\\AppData\\Roaming\\Pill\\plugins\\a");
        assert_ne!(reads.approval_in(folder), reads.approval_in(Path::new(r"C:\elsewhere\a")));
    }

    #[test]
    fn capabilities_drop_repeats_in_any_order() {
        let m = parse(r#"{"id":"a","name":"A","command":"node","capabilities":["feed","widget","feed","widget","live"]}"#, RESERVED).unwrap();
        assert_eq!(m.capabilities, vec![Capability::Feed, Capability::Widget, Capability::Live]);
    }

    #[cfg(windows)]
    #[test]
    fn folders_resolve_to_plain_drive_paths() {
        let dir = std::env::temp_dir().join(format!("pill-plugin-canonical-{}", std::process::id()));
        fs::create_dir_all(dir.join("p")).unwrap();
        let resolved = canonical_folder(&dir.join("p").join("..").join("p"));
        assert!(!resolved.to_string_lossy().starts_with(r"\\?\"));
        assert!(resolved.ends_with("p") && !resolved.to_string_lossy().contains(".."));
        assert_eq!(canonical_folder(&dir.join("missing")), dir.join("missing"));
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(strip_verbatim(r"\\?\C:\a\b.cmd".into()), PathBuf::from(r"C:\a\b.cmd"));
        assert_eq!(strip_verbatim(r"\\?\UNC\srv\x".into()), PathBuf::from(r"\\?\UNC\srv\x"));
    }

    #[test]
    fn tabs_come_with_the_tab_capability() {
        let m = parse(r#"{"id":"a","name":"A","command":"node","capabilities":["tab","projects"],"tab":{"title":"Security","icon":"shield"}}"#, RESERVED).unwrap();
        assert_eq!(m.tab, Some(TabSpec { title: "Security".into(), icon: Icon::Shield }));
        assert_eq!(m.approval(), "node\n+projects\n+tasks", "a tab grants no access, but its buttons can suggest tasks");
        assert!(parse(r#"{"id":"a","name":"A","command":"node","capabilities":["tab"]}"#, RESERVED).unwrap_err().contains("needs \"tab\""));
        assert!(parse(r#"{"id":"a","name":"A","command":"node","tab":{"title":"T","icon":"shield"}}"#, RESERVED).unwrap_err().contains("capability"));
        assert!(parse(r#"{"id":"a","name":"A","command":"node","capabilities":["tab"],"tab":{"title":"T","icon":"skull"}}"#, RESERVED).is_err());
        assert!(parse(r#"{"id":"a","name":"A","command":"node","capabilities":["tab"],"tab":{"title":"A very long tab title","icon":"bug"}}"#, RESERVED).is_err());
    }

    #[test]
    fn capabilities_default_to_everything() {
        let m = parse(r#"{"id":"a","name":"A","command":"a.exe"}"#, RESERVED).unwrap();
        assert_eq!(m.capabilities.len(), 3);
        assert!(m.args.is_empty());
    }

    #[test]
    fn rejects_bad_manifests() {
        assert!(parse(r#"{"id":"Has Space","name":"A","command":"a"}"#, RESERVED).is_err());
        assert!(parse(r#"{"id":"now-playing","name":"A","command":"a"}"#, RESERVED).unwrap_err().contains("built-in"));
        assert!(parse(r#"{"id":"a","name":"","command":"a"}"#, RESERVED).is_err());
        assert!(parse(r#"{"id":"a","name":"A","command":"  "}"#, RESERVED).is_err());
        assert!(parse(r#"{"id":"a","name":"A"}"#, RESERVED).is_err());
        assert!(parse(r#"{"id":"a","name":"A\nB","command":"a"}"#, RESERVED).is_err());
        assert!(parse(r#"{"id":"a","name":"A","command":"a","capabilities":["html"]}"#, RESERVED).is_err());
        let many = format!(r#"{{"id":"a","name":"A","command":"a","args":[{}]}}"#, vec!["\"x\""; 33].join(","));
        assert!(parse(&many, RESERVED).is_err());
    }

    #[test]
    fn command_lines_quote_like_windows() {
        let mut m = parse(r#"{"id":"a","name":"A","command":"C:\\Program Files\\node\\node.exe"}"#, RESERVED).unwrap();
        m.args = vec!["plugin.js".into(), "two words".into(), "say \"hi\"".into(), "".into(), r"C:\dir\".into()];
        assert_eq!(
            m.command_line(),
            r#""C:\Program Files\node\node.exe" plugin.js "two words" "say \"hi\"" "" C:\dir\"#
        );
    }

    #[test]
    fn discovery_reads_folders_and_flags_duplicates() {
        let root = std::env::temp_dir().join(format!("pill-plugins-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (folder, body) in [
            ("a", r#"{"id":"dup","name":"First","command":"x.exe"}"#),
            ("b", r#"{"id":"dup","name":"Second","command":"x.exe"}"#),
            ("c", "{ broken"),
        ] {
            fs::create_dir_all(root.join(folder)).unwrap();
            fs::write(root.join(folder).join(MANIFEST_FILE), body).unwrap();
        }
        fs::create_dir_all(root.join("no-manifest")).unwrap();
        let found = discover(&root, RESERVED);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].manifest.as_ref().unwrap().name, "First");
        assert!(found[1].manifest.as_ref().unwrap_err().contains("already uses"));
        assert!(found[2].manifest.is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn programs_resolve_inside_the_folder_only() {
        let root = std::env::temp_dir().join(format!("pill-plugin-resolve-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("p")).unwrap();
        fs::write(root.join("p").join("run.cmd"), "@echo off").unwrap();
        fs::write(root.join("outside.cmd"), "@echo off").unwrap();
        let dir = root.join("p");
        let inside = parse(r#"{"id":"a","name":"A","command":"./run.cmd"}"#, RESERVED).unwrap();
        assert!(inside.resolve_program(&dir).unwrap().ends_with("run.cmd"));
        let escape = parse(r#"{"id":"a","name":"A","command":"..\\outside.cmd"}"#, RESERVED).unwrap();
        assert!(escape.resolve_program(&dir).unwrap_err().contains("outside"));
        let missing = parse(r#"{"id":"a","name":"A","command":"./nope.exe"}"#, RESERVED).unwrap();
        assert!(missing.resolve_program(&dir).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
