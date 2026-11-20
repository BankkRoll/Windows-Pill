//! Read-only git facts about one project, for its settings page:
//!
//! - whether the folder is a repository at all,
//! - its remote, described without credentials and linked to the web page,
//! - the checked-out branch, its upstream, ahead/behind and uncommitted changes,
//! - local and remote branch names for the base branch picker,
//! - whether the GitHub CLI can open PRs.
//!
//! Every git call is read-only. Pure parsers are split out so they can be
//! tested without a repository.

use crate::agent::{self, Check};
use crate::process::git;
use crate::scan;
use serde::Serialize;
use std::path::Path;

/// The project's remote, safe to show: credentials never leave this module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    /// Remote name, usually `origin`.
    pub name: String,
    /// Short form such as `github.com/owner/repo`, or the path for a local remote.
    pub display: String,
    /// Web page for the repository, when the host is known to serve one.
    pub web_url: Option<String>,
    /// Whether the host looks like GitHub, where `gh` can open PRs.
    pub github: bool,
}

/// Everything the settings page shows about a project's repository.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    /// False for a plain folder; every other field is then empty.
    pub is_repo: bool,
    /// Checked-out branch; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Upstream of the checked-out branch, e.g. `origin/main`.
    pub upstream: Option<String>,
    /// Commits on the branch that the upstream lacks.
    pub ahead: u32,
    /// Commits on the upstream that the branch lacks.
    pub behind: u32,
    /// Changed, staged and untracked files in the working folder.
    pub changes: u32,
    /// Whether the repository has no commits yet.
    pub unborn: bool,
    /// What an empty base branch setting resolves to: `origin/HEAD`, else
    /// a local `main` or `master`.
    pub default_branch: Option<String>,
    /// Preferred remote (`origin`, else the first one).
    pub remote: Option<RemoteInfo>,
    /// Local branches, most recently committed first.
    pub local_branches: Vec<String>,
    /// Branches on the preferred remote without the `origin/` part, most
    /// recently committed first.
    pub remote_branches: Vec<String>,
    /// GitHub CLI install and login, only checked when there is a remote.
    pub gh: Option<Check>,
}

/// Collects [`RepoInfo`] for `path`. Never fails: a missing piece is left
/// empty. Spawns a handful of short git commands and, when there is a remote,
/// `gh auth status` in parallel.
pub fn repo_info(path: &Path) -> RepoInfo {
    if !scan::is_repo(path) {
        return RepoInfo::default();
    }
    let remote = git(path, &["config", "--get-regexp", r"^remote\..*\.url$"])
        .ok()
        .and_then(|out| pick_remote(&out))
        .map(|(name, url)| describe_remote(&name, &url));

    std::thread::scope(|s| {
        let gh = remote.is_some().then(|| s.spawn(agent::gh_check));
        let status = git(path, &["status", "--porcelain=v2", "--branch"]).map(|s| parse_status(&s)).unwrap_or_default();
        let remote_name = remote.as_ref().map(|r| r.name.as_str());
        let mut refs_args = vec!["for-each-ref", "--sort=-committerdate", "--format=%(refname)", "refs/heads"];
        let remote_glob = remote_name.map(|n| format!("refs/remotes/{n}"));
        if let Some(glob) = &remote_glob {
            refs_args.push(glob);
        }
        let (local_branches, remote_branches) =
            git(path, &refs_args).map(|out| split_refs(&out, remote_name.unwrap_or("origin"))).unwrap_or_default();
        RepoInfo {
            is_repo: true,
            branch: status.branch,
            upstream: status.upstream,
            ahead: status.ahead,
            behind: status.behind,
            changes: status.changes,
            unborn: status.unborn,
            default_branch: scan::git_info(path).default_branch,
            remote: remote.clone(),
            local_branches,
            remote_branches,
            gh: gh.and_then(|h| h.join().ok()),
        }
    })
}

/// Branch facts from `git status --porcelain=v2 --branch`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusSummary {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changes: u32,
    pub unborn: bool,
}

/// Parses `git status --porcelain=v2 --branch` output.
pub fn parse_status(out: &str) -> StatusSummary {
    let mut s = StatusSummary::default();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("# branch.oid ") {
            s.unborn = rest.trim() == "(initial)";
        } else if let Some(rest) = line.strip_prefix("# branch.head ") {
            let head = rest.trim();
            s.branch = (head != "(detached)").then(|| head.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            s.upstream = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            for part in rest.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    s.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    s.behind = n.parse().unwrap_or(0);
                }
            }
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            s.changes += 1;
        }
    }
    s
}

/// Picks `origin` (else the first remote) from
/// `git config --get-regexp ^remote\..*\.url$` output. Returns (name, url).
pub fn pick_remote(out: &str) -> Option<(String, String)> {
    let remotes: Vec<(String, String)> = out
        .lines()
        .filter_map(|l| {
            let (key, url) = l.trim().split_once(' ')?;
            let name = key.strip_prefix("remote.")?.strip_suffix(".url")?;
            Some((name.to_string(), url.trim().to_string()))
        })
        .collect();
    remotes.iter().find(|(n, _)| n == "origin").or_else(|| remotes.first()).cloned()
}

/// Splits `for-each-ref --format=%(refname)` output into local branch names
/// and branch names on `remote` (without the `remote/` part, skipping `HEAD`).
pub fn split_refs(out: &str, remote: &str) -> (Vec<String>, Vec<String>) {
    let remote_prefix = format!("refs/remotes/{remote}/");
    let mut local = Vec::new();
    let mut remote_branches = Vec::new();
    for line in out.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("refs/heads/") {
            local.push(name.to_string());
        } else if let Some(name) = line.strip_prefix(&remote_prefix) {
            if name != "HEAD" {
                remote_branches.push(name.to_string());
            }
        }
    }
    (local, remote_branches)
}

/// Hosts known to serve a repository web page at `https://host/owner/repo`.
const WEB_HOSTS: &[&str] = &["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"];

/// Describes a remote URL for display.
///
/// Handles `https://`, `ssh://`, `git://` and scp-like `git@host:owner/repo`
/// forms; strips credentials, ports and a trailing `.git`. Anything else
/// (a local path or `file://`) is shown as-is with no web link.
///
/// # Example
/// ```ignore
/// let r = describe_remote("origin", "git@github.com:owner/repo.git");
/// assert_eq!(r.display, "github.com/owner/repo");
/// ```
pub fn describe_remote(name: &str, url: &str) -> RemoteInfo {
    let url = url.trim();
    let local = || RemoteInfo { name: name.into(), display: url.to_string(), web_url: None, github: false };

    let (scheme, host_and_path) = if let Some((scheme, rest)) = url.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https" | "ssh" | "git" | "git+ssh" | "ssh+git") {
            return local();
        }
        (scheme, rest.to_string())
    } else {
        // scp-like `[user@]host:path`. A single letter before the colon is a
        // Windows drive (`C:\repo`), not a host.
        match url.split_once(':') {
            Some((host, path)) if host.len() > 1 && !host.contains(['/', '\\']) && !path.starts_with('\\') => {
                ("ssh".to_string(), format!("{host}/{}", path.trim_start_matches('/')))
            }
            _ => return local(),
        }
    };

    let (authority, path) = host_and_path.split_once('/').unwrap_or((&host_and_path, ""));
    // SECURITY: https remotes can embed a token as `user:token@host`; drop it.
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host_port.split(':').next().unwrap_or(host_port).to_ascii_lowercase();
    if host.is_empty() {
        return local();
    }
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path).trim_end_matches('/');
    let display = if path.is_empty() { host.clone() } else { format!("{host}/{path}") };

    let github = host == "github.com" || host.contains("github");
    let web_url = if scheme == "http" || scheme == "https" {
        Some(format!("{scheme}://{display}"))
    } else if WEB_HOSTS.contains(&host.as_str()) || github {
        Some(format!("https://{display}"))
    } else {
        None
    };
    RemoteInfo { name: name.into(), display, web_url, github }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(url: &str) -> (String, Option<String>, bool) {
        let r = describe_remote("origin", url);
        (r.display, r.web_url, r.github)
    }

    #[test]
    fn cleans_common_remote_urls() {
        let gh = ("github.com/owner/repo".to_string(), Some("https://github.com/owner/repo".to_string()), true);
        assert_eq!(d("https://github.com/owner/repo.git"), gh);
        assert_eq!(d("https://github.com/owner/repo/"), gh);
        assert_eq!(d("git@github.com:owner/repo.git"), gh);
        assert_eq!(d("ssh://git@github.com:22/owner/repo.git"), gh);
        assert_eq!(d("git+ssh://git@github.com/owner/repo"), gh);
    }

    #[test]
    fn strips_credentials() {
        let (display, web, _) = d("https://user:ghp_secret@github.com/owner/repo.git");
        assert_eq!(display, "github.com/owner/repo");
        assert_eq!(web.as_deref(), Some("https://github.com/owner/repo"));
        assert!(!format!("{:?}", describe_remote("o", "https://x:ghp_secret@gitlab.com/a/b")).contains("secret"));
    }

    #[test]
    fn other_hosts_and_local_paths() {
        assert_eq!(d("git@gitlab.com:group/sub/proj.git"), ("gitlab.com/group/sub/proj".into(), Some("https://gitlab.com/group/sub/proj".into()), false));
        assert_eq!(d("git@git.internal:team/app.git"), ("git.internal/team/app".into(), None, false));
        assert_eq!(d("http://git.local/team/app"), ("git.local/team/app".into(), Some("http://git.local/team/app".into()), false));
        assert_eq!(d(r"C:\repos\app.git"), (r"C:\repos\app.git".into(), None, false));
        assert_eq!(d("../app.git"), ("../app.git".into(), None, false));
        assert_eq!(d("file:///C:/repos/app.git"), ("file:///C:/repos/app.git".into(), None, false));
        assert_eq!(d(r"\\server\share\app.git"), (r"\\server\share\app.git".into(), None, false));
    }

    #[test]
    fn parses_porcelain_v2_status() {
        let out = "# branch.oid 1234abcd\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -5\n1 .M N... 100644 100644 100644 a b src/x.ts\n? notes.txt\n";
        assert_eq!(
            parse_status(out),
            StatusSummary { branch: Some("main".into()), upstream: Some("origin/main".into()), ahead: 2, behind: 5, changes: 2, unborn: false }
        );
        let fresh = parse_status("# branch.oid (initial)\n# branch.head master\n");
        assert!(fresh.unborn);
        assert_eq!(fresh.branch.as_deref(), Some("master"));
        assert_eq!(parse_status("# branch.oid abc\n# branch.head (detached)\n").branch, None);
    }

    #[test]
    fn prefers_origin_remote() {
        let out = "remote.upstream.url https://github.com/a/b\nremote.origin.url git@github.com:me/b.git\n";
        assert_eq!(pick_remote(out), Some(("origin".into(), "git@github.com:me/b.git".into())));
        assert_eq!(pick_remote("remote.fork.url x\n"), Some(("fork".into(), "x".into())));
        assert_eq!(pick_remote(""), None);
    }

    #[test]
    fn splits_local_and_remote_refs() {
        let out = "refs/heads/main\nrefs/heads/feat/x\nrefs/remotes/origin/HEAD\nrefs/remotes/origin/main\nrefs/remotes/origin/release\n";
        let (local, remote) = split_refs(out, "origin");
        assert_eq!(local, vec!["main", "feat/x"]);
        assert_eq!(remote, vec!["main", "release"]);
    }
}
