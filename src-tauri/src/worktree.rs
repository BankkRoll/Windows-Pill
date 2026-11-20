//! Git worktrees for agent runs and editing chats.
//!
//! Responsibilities:
//! - Pick the commit a new branch starts from: the freshly fetched remote
//!   base, else the last fetched copy, else the local base branch.
//! - Create a worktree on a branch without ever touching the user's own
//!   checkout: no stash, reset or checkout there, whatever state it is in.
//! - Remove a worktree even when the user deleted its folder, the repository
//!   moved, or git no longer knows about it, and refuse to delete anything
//!   that isn't one of Pill's own worktree folders.
//!
//! Pure parsers and decisions are split out so they can be tested without a
//! repository; the rest is covered by tests against real temporary repos.

use crate::model::MAX_ID_LEN;
use crate::process::git;
use crate::workspace::Project;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Folder, next to each repository, that holds Pill's worktrees.
pub const WORKTREES_DIR: &str = ".pill-worktrees";

/// Where a new branch starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseChoice {
    /// Ref to branch from and to compare against, e.g. `origin/main` or `main`.
    pub base_ref: String,
    /// Said in the run log when the choice isn't the usual one.
    pub note: Option<String>,
}

/// Picks the base ref from what is known about the repository.
///
/// - `remote`: the remote agents use, if any.
/// - `fetch_error`: why fetching the base branch failed, if it did.
/// - `has_remote_ref` / `has_local_ref`: whether `refs/remotes/<remote>/<base>`
///   and `refs/heads/<base>` exist.
pub fn choose_base(
    remote: Option<&str>,
    base: &str,
    fetch_error: Option<&str>,
    has_remote_ref: bool,
    has_local_ref: bool,
) -> Result<BaseChoice, String> {
    let missing = || {
        format!("The base branch `{base}` doesn't exist in this repository. Pick another base branch in the project's settings.")
    };
    let Some(remote) = remote else {
        return if has_local_ref { Ok(BaseChoice { base_ref: base.to_string(), note: None }) } else { Err(missing()) };
    };
    match fetch_error {
        None if has_remote_ref => Ok(BaseChoice { base_ref: format!("{remote}/{base}"), note: None }),
        None if has_local_ref => Ok(BaseChoice {
            base_ref: base.to_string(),
            note: Some(format!("`{base}` isn't on {remote}; starting from your local `{base}`")),
        }),
        None => Err(missing()),
        Some(e) if has_remote_ref => Ok(BaseChoice {
            base_ref: format!("{remote}/{base}"),
            note: Some(format!("Couldn't fetch from {remote} ({}); starting from the last fetched {remote}/{base}", first_line(e))),
        }),
        Some(e) if has_local_ref => Ok(BaseChoice {
            base_ref: base.to_string(),
            note: Some(format!("Couldn't fetch `{base}` from {remote} ({}); starting from your local `{base}`", first_line(e))),
        }),
        Some(e) => Err(format!("Couldn't fetch `{base}` from {remote}: {}", first_line(e))),
    }
}

fn first_line(s: &str) -> &str {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or(s)
}

/// Whether `name` (e.g. `refs/heads/main`) exists in `repo`.
pub fn ref_exists(repo: &Path, name: &str) -> bool {
    git(repo, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")]).is_ok()
}

/// Checks that `name` is a plain branch name git accepts, as written.
///
/// SECURITY: the base branch comes from `.pill/project.json`, which a
/// repository can commit. It is passed to git, so a name starting with `-`
/// must never read as an option, and `@{-1}`-style shorthands must not
/// expand to some other branch.
pub fn check_branch_name(repo: &Path, name: &str) -> Result<(), String> {
    let invalid = || format!("`{name}` isn't a valid branch name. Pick another base branch in the project's settings.");
    if name.is_empty() || name.starts_with('-') || name.contains("@{") {
        return Err(invalid());
    }
    match git(repo, &["check-ref-format", "--branch", name]) {
        Ok(out) if out.trim() == name => Ok(()),
        _ => Err(invalid()),
    }
}

/// Fetches the project's base branch and picks where a new branch starts.
/// Only reads the remote; nothing in the user's checkout changes.
pub fn resolve_base(project: &Project) -> Result<BaseChoice, String> {
    let base = project.base_branch.as_str();
    check_branch_name(&project.path, base)?;
    let remote = project.remote.as_deref();
    let fetch_error = remote.and_then(|r| git(&project.path, &["fetch", "--quiet", "--end-of-options", r, base]).err());
    let has_remote_ref = remote.is_some_and(|r| ref_exists(&project.path, &format!("refs/remotes/{r}/{base}")));
    let has_local_ref = ref_exists(&project.path, &format!("refs/heads/{base}"));
    choose_base(remote, base, fetch_error.as_deref(), has_remote_ref, has_local_ref)
}

/// One entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    /// Checked-out branch without `refs/heads/`; `None` when detached or bare.
    pub branch: Option<String>,
}

/// Parses `git worktree list --porcelain` output.
pub fn parse_worktrees(out: &str) -> Vec<WorktreeEntry> {
    let mut list = Vec::new();
    for block in out.split("\n\n") {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p.trim()));
            } else if let Some(b) = line.strip_prefix("branch ") {
                branch = Some(b.trim().trim_start_matches("refs/heads/").to_string());
            }
        }
        if let Some(path) = path {
            list.push(WorktreeEntry { path, branch });
        }
    }
    list
}

/// Compares paths the way Windows does: case-insensitive, either slash, no
/// trailing separator.
pub fn same_path(a: &Path, b: &Path) -> bool {
    fn norm(p: &Path) -> String {
        p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase()
    }
    norm(a) == norm(b)
}

/// Whether `name` can name a worktree folder: a task, run or chat id, plus
/// the `-<provider>-<n>` suffix compared runs add.
fn is_worktree_name(name: &str) -> bool {
    (1..=MAX_ID_LEN + 16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Whether `path` is one of `project`'s worktree folders, the only kind Pill
/// will delete from disk itself: exactly [`path_for`]`(project, name)` for a
/// plain `name`.
///
/// SECURITY: worktree paths are read from run and chat records a repository
/// can commit. Every component must be a plain name, so `..` can't walk out
/// of `.pill-worktrees` to the projects folder or further.
pub fn is_pill_worktree(path: &Path, project: &Path) -> bool {
    let plain = path
        .components()
        .skip_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
        .all(|c| matches!(c, Component::Normal(_)));
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    plain && is_worktree_name(name) && same_path(path, &path_for(project, name)) && !project.starts_with(path)
}

/// The folder for a worktree named `name` of the repository at `project`:
/// `<parent>\.pill-worktrees\<repo folder>\<name>`. `name` must be a plain
/// id; [`is_pill_worktree`] refuses anything else.
pub fn path_for(project: &Path, name: &str) -> PathBuf {
    let folder = project.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    let parent = project.parent().unwrap_or(project);
    parent.join(WORKTREES_DIR).join(folder).join(name)
}

/// Creates a worktree at `path` on `branch`, (re)pointing the branch at
/// `base_ref`.
///
/// Works whatever state the user's checkout is in (dirty, on another branch,
/// mid-rebase): `git worktree add` only reads the repository. Refuses when
/// the branch is checked out somewhere else, since moving it would pull the
/// rug from under that checkout.
pub fn add(repo: &Path, branch: &str, path: &Path, base_ref: &str) -> Result<(), String> {
    let _ = git(repo, &["worktree", "prune"]);
    if let Ok(out) = git(repo, &["worktree", "list", "--porcelain"]) {
        if let Some(other) = parse_worktrees(&out).into_iter().find(|w| w.branch.as_deref() == Some(branch) && !same_path(&w.path, path)) {
            return Err(if same_path(&other.path, repo) {
                format!("`{branch}` is checked out in your working folder. Switch it to another branch and try again; Pill never changes your checkout.")
            } else {
                format!("`{branch}` is checked out in another worktree at {}. Remove that worktree or switch it to another branch.", other.path.display())
            });
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
    }
    git(repo, &["worktree", "add", "-B", branch, "--end-of-options", &path.to_string_lossy(), base_ref]).map(|_| ())
}

/// Removes a Pill worktree and forgets it, tolerating a folder the user
/// already deleted, a repository that moved, or a worktree git no longer
/// tracks. Never touches anything outside `.pill-worktrees`.
pub fn remove(repo: &Path, path: &Path) -> Result<(), String> {
    if !is_pill_worktree(path, repo) {
        // SECURITY: a hand-edited or corrupt run record must never point
        // `worktree remove --force` (or a folder delete) at real work.
        return Err(format!("Refusing to remove {}: it isn't one of Pill's worktree folders", path.display()));
    }
    if !repo.is_dir() {
        // The repository is gone; its worktree folder is just files now.
        return delete_folder(path);
    }
    if path.exists() {
        let wt = path.to_string_lossy();
        if git(repo, &["worktree", "remove", "--force", &wt]).is_err() {
            // A moved repository leaves the worktree's back-link stale; repair
            // it and try again before falling back to deleting the folder.
            let _ = git(repo, &["worktree", "repair", &wt]);
            if git(repo, &["worktree", "remove", "--force", &wt]).is_err() {
                delete_folder(path)?;
            }
        }
    }
    let _ = git(repo, &["worktree", "prune"]);
    Ok(())
}

fn delete_folder(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!(
            "Couldn't delete {}: {e}. Close any editor or terminal using it and try again.",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::command;

    #[test]
    fn base_follows_the_remote_then_falls_back() {
        let ok = |r: Result<BaseChoice, String>| r.unwrap();
        assert_eq!(ok(choose_base(Some("origin"), "main", None, true, true)), BaseChoice { base_ref: "origin/main".into(), note: None });
        assert_eq!(ok(choose_base(Some("upstream"), "dev", None, true, false)).base_ref, "upstream/dev");
        let offline = ok(choose_base(Some("origin"), "main", Some("fatal: unable to access\nmore"), true, true));
        assert_eq!(offline.base_ref, "origin/main");
        assert!(offline.note.unwrap().contains("fatal: unable to access"));
        let local_only_branch = ok(choose_base(Some("origin"), "wip", Some("couldn't find remote ref wip"), false, true));
        assert_eq!(local_only_branch.base_ref, "wip");
        assert!(choose_base(Some("origin"), "nope", Some("couldn't find remote ref nope"), false, false).is_err());
    }

    #[test]
    fn local_repos_never_touch_a_remote() {
        assert_eq!(choose_base(None, "trunk", None, false, true).unwrap(), BaseChoice { base_ref: "trunk".into(), note: None });
        assert!(choose_base(None, "main", None, false, false).unwrap_err().contains("doesn't exist"));
    }

    #[test]
    fn parses_worktree_lists() {
        let out = "worktree C:/dev/app\nHEAD abc\nbranch refs/heads/main\n\nworktree C:/dev/.pill-worktrees/app/t_1\nHEAD def\nbranch refs/heads/agent/t_1-fix\n\nworktree C:/dev/x\nHEAD 123\ndetached\n";
        let list = parse_worktrees(out);
        assert_eq!(list.len(), 3);
        assert_eq!(list[1].branch.as_deref(), Some("agent/t_1-fix"));
        assert!(same_path(&list[1].path, Path::new(r"c:\dev\.pill-worktrees\APP\t_1\")));
        assert_eq!(list[2].branch, None);
    }

    #[test]
    fn only_pill_worktree_folders_may_be_deleted() {
        let repo = Path::new(r"C:\dev\app");
        assert!(is_pill_worktree(Path::new(r"C:\dev\.pill-worktrees\app\t_1"), repo));
        assert!(!is_pill_worktree(repo, repo));
        assert!(!is_pill_worktree(Path::new(r"C:\dev"), repo));
        assert!(!is_pill_worktree(Path::new(r"C:\dev\other"), repo));
        assert!(!is_pill_worktree(Path::new(r"C:\.pill-worktrees"), repo));
        assert!(!is_pill_worktree(Path::new(r"C:\dev\.pill-worktrees\app"), repo));
        assert!(!is_pill_worktree(Path::new(r"C:\dev\.pill-worktrees\other-app\t_1"), repo), "another repository's worktree");
        assert!(!is_pill_worktree(Path::new(r"C:\elsewhere\.pill-worktrees\app\t_1"), repo));
        assert_eq!(path_for(repo, "t_1"), PathBuf::from(r"C:\dev\.pill-worktrees\app\t_1"));
    }

    #[test]
    fn traversal_out_of_the_worktrees_folder_is_refused() {
        let repo = Path::new(r"C:\dev\app");
        for climb in [r"..\..\..", "..", r"t_1\..", r"..\..\dev", r".\t_1", "a/../t_1"] {
            assert!(!is_pill_worktree(&path_for(repo, climb), repo), "{climb}");
        }
        assert!(!is_pill_worktree(Path::new(r"C:\dev\.pill-worktrees\app\t_1\..\..\.."), repo));
        assert!(!is_pill_worktree(Path::new(r"C:\dev\.pill-worktrees\app\..\..\.pill-worktrees\app\t_1"), repo));
    }

    #[test]
    fn remove_never_deletes_through_a_committed_traversal_path() {
        let root = sandbox("traverse");
        let repo = root.join("projects").join("app");
        init_repo(&repo);
        let precious = root.join("projects").join("precious");
        fs::create_dir_all(&precious).unwrap();
        fs::write(precious.join("keep.txt"), "mine").unwrap();
        // What a run.json naming task `..\..\..` or `..\precious` would point at.
        for name in [r"..\..\..", r"..\..\precious", ".."] {
            let target = path_for(&repo, name);
            assert!(remove(&repo, &target).unwrap_err().contains("Refusing"), "{name}");
        }
        let gone_repo = root.join("projects").join("gone");
        assert!(remove(&gone_repo, &path_for(&gone_repo, r"..\..\precious")).is_err());
        assert!(precious.join("keep.txt").exists());
        assert!(repo.join("a.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    // -------------------------------------------------------------------------
    // Against real repositories
    // -------------------------------------------------------------------------

    fn sandbox(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pill-wt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let mut cmd = command("git");
        cmd.current_dir(dir)
            .args(["-c", "user.name=Pill Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false"])
            .args(args);
        crate::process::run(cmd).unwrap_or_else(|e| panic!("git {args:?}: {e}"))
    }

    fn init_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        sh(dir, &["init", "-q", "-b", "main"]);
        fs::write(dir.join("a.txt"), "one\n").unwrap();
        sh(dir, &["add", "."]);
        sh(dir, &["commit", "-q", "-m", "first"]);
    }

    fn project(path: &Path, remote: Option<&str>) -> Project {
        Project {
            id: crate::workspace::project_id(path),
            path: path.to_path_buf(),
            name: "app".into(),
            color: String::new(),
            base_branch: "main".into(),
            settings: Default::default(),
            tasks: Vec::new(),
            notes: String::new(),
            has_remote: remote.is_some(),
            remote: remote.map(str::to_string),
            last_active: 0,
            errors: Vec::new(),
            tasks_locked: false,
            manual: false,
        }
    }

    #[test]
    fn worktrees_leave_a_dirty_checkout_on_another_branch_alone() {
        let root = sandbox("dirty");
        let repo = root.join("my app ü");
        init_repo(&repo);
        sh(&repo, &["checkout", "-q", "-b", "feature"]);
        fs::write(repo.join("a.txt"), "edited but not committed\n").unwrap();
        fs::write(repo.join("new.txt"), "untracked\n").unwrap();
        sh(&repo, &["add", "new.txt"]);
        let status_before = sh(&repo, &["status", "--porcelain=v2", "--branch"]);

        let base = resolve_base(&project(&repo, None)).unwrap();
        assert_eq!(base.base_ref, "main");
        let wt = path_for(&repo, "t_1");
        add(&repo, "agent/t_1-fix", &wt, &base.base_ref).unwrap();

        assert_eq!(sh(&repo, &["status", "--porcelain=v2", "--branch"]), status_before);
        assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "edited but not committed\n");
        assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap().replace("\r\n", "\n"), "one\n");
        assert!(!wt.join("new.txt").exists());

        remove(&repo, &wt).unwrap();
        assert!(!wt.exists());
        assert_eq!(sh(&repo, &["status", "--porcelain=v2", "--branch"]), status_before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_a_branch_checked_out_in_the_users_folder() {
        let root = sandbox("checked-out");
        let repo = root.join("app");
        init_repo(&repo);
        sh(&repo, &["checkout", "-q", "-b", "agent/t_2-x"]);
        let err = add(&repo, "agent/t_2-x", &path_for(&repo, "t_2"), "main").unwrap_err();
        assert!(err.contains("checked out in your working folder"), "{err}");
        assert_eq!(sh(&repo, &["branch", "--show-current"]), "agent/t_2-x");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn removes_worktrees_whose_folder_or_registration_is_gone() {
        let root = sandbox("gone");
        let repo = root.join("app");
        init_repo(&repo);
        let wt = path_for(&repo, "t_3");
        add(&repo, "agent/t_3", &wt, "main").unwrap();
        fs::remove_dir_all(&wt).unwrap();
        remove(&repo, &wt).unwrap();
        assert!(!sh(&repo, &["worktree", "list"]).contains("t_3"));
        // Re-adding the same branch works once the stale entry is pruned.
        add(&repo, "agent/t_3", &wt, "main").unwrap();
        fs::remove_file(wt.join(".git")).unwrap();
        remove(&repo, &wt).unwrap();
        assert!(!wt.exists());
        assert!(remove(&repo, &repo).is_err());
        assert!(repo.join("a.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fetches_from_a_remote_not_named_origin() {
        let root = sandbox("upstream");
        let upstream = root.join("server");
        init_repo(&upstream);
        let repo = root.join("clone");
        sh(&root, &["clone", "-q", "-o", "upstream", &upstream.to_string_lossy(), &repo.to_string_lossy()]);
        fs::write(upstream.join("b.txt"), "later\n").unwrap();
        sh(&upstream, &["add", "."]);
        sh(&upstream, &["commit", "-q", "-m", "second"]);

        let info = crate::scan::git_info(&repo);
        assert_eq!(info.remote.as_deref(), Some("upstream"));
        let base = resolve_base(&project(&repo, info.remote.as_deref())).unwrap();
        assert_eq!(base, BaseChoice { base_ref: "upstream/main".into(), note: None });
        // The fetch brought the new commit in without moving the local branch.
        assert_eq!(sh(&repo, &["rev-list", "--count", "main..upstream/main"]), "1");

        fs::remove_dir_all(&upstream).unwrap();
        let offline = resolve_base(&project(&repo, Some("upstream"))).unwrap();
        assert_eq!(offline.base_ref, "upstream/main");
        assert!(offline.note.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn base_branches_that_could_read_as_options_are_refused() {
        let root = sandbox("refname");
        let repo = root.join("app");
        init_repo(&repo);
        assert!(check_branch_name(&repo, "main").is_ok());
        assert!(check_branch_name(&repo, "release/2.0").is_ok());
        for bad in ["--upload-pack=calc.exe", "-x", "", "a..b", "@{-1}", "main@{u}", "with space", "x.lock"] {
            assert!(check_branch_name(&repo, bad).is_err(), "{bad:?}");
        }
        let mut p = project(&repo, None);
        p.base_branch = "--upload-pack=calc.exe".into();
        assert!(resolve_base(&p).unwrap_err().contains("isn't a valid branch name"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deleted_repositories_still_lose_their_worktree_folders() {
        let root = sandbox("deleted-repo");
        let repo = root.join("app");
        init_repo(&repo);
        let wt = path_for(&repo, "t_4");
        add(&repo, "agent/t_4", &wt, "main").unwrap();
        fs::remove_dir_all(&repo).unwrap();
        remove(&repo, &wt).unwrap();
        assert!(!wt.exists());
        let _ = fs::remove_dir_all(root);
    }
}
