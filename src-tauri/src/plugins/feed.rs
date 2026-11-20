//! Feed items the user dismissed, kept on disk so they never come back.
//!
//! Responsibilities:
//! - Load and save `feed-dismissed.json` next to the plugins folder.
//! - Answer whether a plugin's item id was dismissed, and bound the list so
//!   a plugin that churns through ids can't grow the file forever.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Dismissed ids remembered per plugin; the oldest are forgotten first.
pub const MAX_DISMISSED: usize = 2_000;

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    dismissed: BTreeMap<String, Vec<String>>,
}

/// Dismissed feed item ids per plugin, backed by a JSON file.
#[derive(Debug)]
pub struct Dismissed {
    path: PathBuf,
    by_plugin: BTreeMap<String, Vec<String>>,
}

impl Dismissed {
    /// Loads the list from `path`. A missing or broken file starts empty;
    /// the worst case is that dismissed items show again.
    ///
    /// @param path Where the list lives.
    pub fn load(path: PathBuf) -> Self {
        let by_plugin = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| match serde_json::from_str::<File>(&text) {
                Ok(f) => Some(f.dismissed),
                Err(e) => {
                    log::warn!("{}: {e}", path.display());
                    None
                }
            })
            .unwrap_or_default();
        Self { path, by_plugin }
    }

    /// Whether `plugin` has dismissed `item`.
    pub fn contains(&self, plugin: &str, item: &str) -> bool {
        self.by_plugin.get(plugin).is_some_and(|ids| ids.iter().any(|i| i == item))
    }

    /// Remembers `items` as dismissed and saves the file.
    ///
    /// @param plugin Plugin id.
    /// @param items Item ids to hide.
    pub fn add(&mut self, plugin: &str, items: impl IntoIterator<Item = String>) -> std::io::Result<()> {
        let ids = self.by_plugin.entry(plugin.to_string()).or_default();
        for item in items {
            if !ids.contains(&item) {
                ids.push(item);
            }
        }
        let excess = ids.len().saturating_sub(MAX_DISMISSED);
        ids.drain(..excess);
        self.save()
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&File { dismissed: self.by_plugin.clone() }).map_err(std::io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pill-feed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("feed-dismissed.json")
    }

    #[test]
    fn dismissals_persist_per_plugin() {
        let path = temp("persist");
        let mut d = Dismissed::load(path.clone());
        assert!(!d.contains("cve-watch", "GHSA-1"));
        d.add("cve-watch", ["GHSA-1".to_string(), "GHSA-1".to_string()]).unwrap();
        let again = Dismissed::load(path.clone());
        assert!(again.contains("cve-watch", "GHSA-1"));
        assert!(!again.contains("other", "GHSA-1"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_broken_file_starts_empty_and_the_list_is_bounded() {
        let path = temp("bounded");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ nope").unwrap();
        let mut d = Dismissed::load(path.clone());
        d.add("p", (0..MAX_DISMISSED + 5).map(|i| format!("i{i}"))).unwrap();
        assert!(!d.contains("p", "i0"));
        assert!(d.contains("p", &format!("i{}", MAX_DISMISSED + 4)));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
