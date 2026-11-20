//! Discovers git repositories under the configured roots and reads cheap git
//! metadata straight from the `.git` directory, without spawning `git`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// How deep below a root to look for repositories.
pub const MAX_DEPTH: usize = 4;

/// Directory names never descended into: dependency caches, build output,
/// system folders and Pill's own worktrees.
const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "dist", "build", "out", ".next", ".turbo", ".cache", "vendor",
    "venv", ".venv", "__pycache__", "AppData", "$Recycle.Bin", "System Volume Information",
    ".pill-worktrees", ".pill", ".idea", ".vscode", "bin", "obj", "Library", "Pods",
    // System folders, so whole drives (e.g. `D:\`) work as roots.
    "Windows", "Program Files", "Program Files (x86)", "ProgramData", "Recovery",
    "PerfLogs", "OneDriveTemp", "msys64", "cygwin64",
];

/// Drive roots (`C:\`, `D:\`, …) that currently exist, for one-click roots.
pub fn available_drives() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|c| PathBuf::from(format!("{}:\\", c as char)))
        .filter(|p| p.is_dir())
        .collect()
}

/// Whether `dir` is the root of a git repository (a `.git` dir, or a `.git`
/// file as used by worktrees and submodules).
pub fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Finds repositories under `root`, not descending into repositories, so
/// nested repos and submodules don't appear as separate projects.
pub fn find_repos(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, 0, &mut found);
    found.sort();
    found
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if is_repo(dir) {
        found.push(dir.to_path_buf());
        return;
    }
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        // Symlinks and junctions are skipped to avoid cycles and duplicates.
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name != ".config" || SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
            continue;
        }
        walk(&entry.path(), depth + 1, found);
    }
}

/// Resolves the real git directory, following `gitdir:` files.
pub fn git_dir(repo: &Path) -> Option<PathBuf> {
    let dot = repo.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = fs::read_to_string(&dot).ok()?;
    let target = text.trim().strip_prefix("gitdir:")?.trim();
    let p = PathBuf::from(target);
    Some(if p.is_absolute() { p } else { repo.join(p) })
}

/// Cheap facts about a repository.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GitInfo {
    /// Unix seconds of the last HEAD movement (commit, checkout, merge).
    pub last_activity: Option<i64>,
    /// Default branch from the preferred remote's `HEAD`, else a local
    /// `main` or `master`.
    pub default_branch: Option<String>,
    /// Remote agents fetch from and push to: `origin`, else the first one
    /// configured. `None` for a local-only repository.
    pub remote: Option<String>,
}

/// Reads [`GitInfo`] from the git directory's files.
pub fn git_info(repo: &Path) -> GitInfo {
    let Some(dir) = git_dir(repo) else { return GitInfo::default() };
    // Worktrees keep shared config and refs in the "common" dir.
    let common = fs::read_to_string(dir.join("commondir"))
        .ok()
        .map(|c| dir.join(c.trim()))
        .unwrap_or_else(|| dir.clone());

    let last_activity = fs::metadata(dir.join("logs").join("HEAD"))
        .or_else(|_| fs::metadata(dir.join("HEAD")))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    let config = fs::read_to_string(common.join("config")).unwrap_or_default();
    let remote = preferred_remote(&remote_names(&config));

    let default_branch = remote
        .as_ref()
        .and_then(|r| fs::read_to_string(common.join("refs/remotes").join(r).join("HEAD")).ok().and_then(|s| parse_remote_head(&s, r)))
        .or_else(|| {
            let packed = fs::read_to_string(common.join("packed-refs")).unwrap_or_default();
            local_default_branch(|name| common.join("refs/heads").join(name).is_file(), &packed)
        });

    GitInfo { last_activity, default_branch, remote }
}

/// Names of the remotes declared in a git `config` file, in file order.
pub fn remote_names(config: &str) -> Vec<String> {
    config
        .lines()
        .filter_map(|l| l.trim().strip_prefix("[remote \"")?.strip_suffix("\"]").map(str::to_string))
        .filter(|n| !n.is_empty())
        .collect()
}

/// `origin` when it exists, else the first remote, matching what
/// `gitinfo::pick_remote` shows in settings.
pub fn preferred_remote(names: &[String]) -> Option<String> {
    names.iter().find(|n| *n == "origin").or_else(|| names.first()).cloned()
}

/// Without a remote `HEAD` (local-only repos, or a remote added later), guesses
/// the default branch: `main`, then `master`, as a loose ref or in `packed-refs`.
pub fn local_default_branch(loose_exists: impl Fn(&str) -> bool, packed_refs: &str) -> Option<String> {
    ["main", "master"]
        .into_iter()
        .find(|name| {
            let full = format!("refs/heads/{name}");
            loose_exists(name) || packed_refs.lines().any(|l| l.split_whitespace().nth(1) == Some(full.as_str()))
        })
        .map(str::to_string)
}

/// Parses `ref: refs/remotes/<remote>/main` into `main`.
pub fn parse_remote_head(contents: &str, remote: &str) -> Option<String> {
    contents
        .trim()
        .strip_prefix("ref: refs/remotes/")?
        .strip_prefix(remote)?
        .strip_prefix('/')
        .filter(|b| !b.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pill-scan-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_repos_but_not_nested_or_skipped() {
        let root = tmp("find");
        fs::create_dir_all(root.join("a/.git")).unwrap();
        fs::create_dir_all(root.join("a/sub/.git")).unwrap();
        fs::create_dir_all(root.join("group/b/.git")).unwrap();
        fs::create_dir_all(root.join("node_modules/c/.git")).unwrap();
        fs::create_dir_all(root.join("x/y/z/w/v/deep/.git")).unwrap();
        let repos = find_repos(&root);
        assert_eq!(repos, vec![root.join("a"), root.join("group/b")]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_default_branch_and_remote() {
        let root = tmp("info");
        let git = root.join(".git");
        fs::create_dir_all(git.join("refs/remotes/origin")).unwrap();
        fs::write(git.join("refs/remotes/origin/HEAD"), "ref: refs/remotes/origin/trunk\n").unwrap();
        fs::write(git.join("config"), "[core]\n[remote \"origin\"]\n\turl = x\n").unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/trunk\n").unwrap();
        let info = git_info(&root);
        assert_eq!(info.default_branch.as_deref(), Some("trunk"));
        assert_eq!(info.remote.as_deref(), Some("origin"));
        assert!(info.last_activity.is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uses_a_remote_not_named_origin() {
        let root = tmp("upstream");
        let git = root.join(".git");
        fs::create_dir_all(git.join("refs/remotes/upstream")).unwrap();
        fs::write(git.join("refs/remotes/upstream/HEAD"), "ref: refs/remotes/upstream/develop\n").unwrap();
        fs::write(git.join("config"), "[core]\n[remote \"upstream\"]\n\turl = x\n").unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/develop\n").unwrap();
        let info = git_info(&root);
        assert_eq!(info.remote.as_deref(), Some("upstream"));
        assert_eq!(info.default_branch.as_deref(), Some("develop"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_only_repos_have_no_remote() {
        let root = tmp("local");
        fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
        fs::write(root.join(".git/config"), "[core]\n\tbare = false\n").unwrap();
        fs::write(root.join(".git/refs/heads/master"), "abc\n").unwrap();
        let info = git_info(&root);
        assert_eq!(info.remote, None);
        assert_eq!(info.default_branch.as_deref(), Some("master"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn picks_origin_then_the_first_remote() {
        let cfg = "[core]\n[remote \"fork\"]\n\turl = a\n[remote \"origin\"]\n\turl = b\n[branch \"main\"]\n";
        assert_eq!(remote_names(cfg), vec!["fork", "origin"]);
        assert_eq!(preferred_remote(&remote_names(cfg)).as_deref(), Some("origin"));
        assert_eq!(preferred_remote(&["fork".into(), "mirror".into()]).as_deref(), Some("fork"));
        assert_eq!(preferred_remote(&[]), None);
        assert_eq!(parse_remote_head("ref: refs/remotes/fork/trunk\n", "fork").as_deref(), Some("trunk"));
        assert_eq!(parse_remote_head("ref: refs/remotes/forked/trunk", "fork"), None);
        assert_eq!(parse_remote_head("ref: refs/remotes/fork/", "fork"), None);
    }

    #[test]
    fn guesses_local_default_branch() {
        assert_eq!(local_default_branch(|n| n == "master", ""), Some("master".into()));
        assert_eq!(local_default_branch(|n| n == "main" || n == "master", ""), Some("main".into()));
        let packed = "# pack-refs with: peeled\nabc123 refs/heads/master\ndef456 refs/tags/v1\n";
        assert_eq!(local_default_branch(|_| false, packed), Some("master".into()));
        assert_eq!(local_default_branch(|_| false, "abc refs/heads/dev\n"), None);
    }

    #[test]
    fn follows_gitdir_files() {
        let root = tmp("gitdir");
        let real = root.join("real-git");
        fs::create_dir_all(&real).unwrap();
        fs::create_dir_all(root.join("wt")).unwrap();
        fs::write(root.join("wt/.git"), format!("gitdir: {}\n", real.display())).unwrap();
        assert_eq!(git_dir(&root.join("wt")), Some(real));
        fs::remove_dir_all(root).unwrap();
    }
}
