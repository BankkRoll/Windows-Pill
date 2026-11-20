//! Helpers for spawning external programs (git, gh, claude, test commands).
//!
//! - Every child gets no console window and a PATH that includes entries the
//!   user added after Pill started (a fresh install of git, gh or a CLI).
//! - Batch shims (npm's `.cmd` files) are quoted by the standard library,
//!   which also copes with install folders that contain spaces.
//! - A missing program or folder becomes a sentence with the fix, not an OS
//!   error code.

use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// `CREATE_NO_WINDOW`: a console child gets no console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How long a PATH read from the registry is reused before reading it again.
const PATH_TTL: Duration = Duration::from_secs(10);

/// What to do when git can't be found.
pub const GIT_MISSING: &str =
    "Git isn't installed or isn't on PATH. Install Git for Windows (`winget install --id Git.Git`), then try again.";

/// A `Command` that never flashes a console window on Windows and searches
/// the current PATH.
///
/// NOTE: Pill is a GUI-subsystem app, so every console child (git, gh…)
/// would otherwise pop up its own terminal window for a split second.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    // NOTE: std resolves the program against the child's PATH when one is
    // set, so a CLI installed while Pill runs is found without a restart.
    cmd.env("PATH", search_path());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Pill's own PATH followed by the user and machine PATH entries it lacks.
///
/// A process keeps the environment it started with, and Pill usually starts
/// at sign-in, long before the user installs git, gh, Claude Code or Codex.
/// Windows keeps the live values in the registry, so they are read from
/// there, at most every few seconds.
pub fn search_path() -> OsString {
    static CACHE: Mutex<Option<(Instant, OsString)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, path)) = cache.as_ref() {
        if at.elapsed() < PATH_TTL {
            return path.clone();
        }
    }
    let own = env::var_os("PATH").unwrap_or_default();
    let fresh = merge_path(&own, &registry_path());
    *cache = Some((Instant::now(), fresh.clone()));
    fresh
}

/// Appends `extra` PATH strings to `current`, skipping folders already
/// present. Folders compare case-insensitively and without trailing
/// separators, as Windows compares them.
///
/// # Example
/// ```ignore
/// let p = merge_path(OsStr::new(r"C:\a;C:\b"), &[r"c:\B\;C:\c".into()]);
/// assert_eq!(p, OsString::from(r"C:\a;C:\b;C:\c"));
/// ```
pub fn merge_path(current: &OsStr, extra: &[String]) -> OsString {
    fn key(p: &Path) -> String {
        p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase()
    }
    let mut dirs: Vec<PathBuf> = env::split_paths(current).filter(|p| !p.as_os_str().is_empty()).collect();
    let mut seen: std::collections::HashSet<String> = dirs.iter().map(|p| key(p)).collect();
    for entry in extra {
        for dir in env::split_paths(entry) {
            if !dir.as_os_str().is_empty() && seen.insert(key(&dir)) {
                dirs.push(dir);
            }
        }
    }
    env::join_paths(dirs).unwrap_or_else(|_| current.to_os_string())
}

/// The machine and user PATH values as Windows stores them right now, with
/// `%VARIABLES%` expanded.
#[cfg(windows)]
fn registry_path() -> Vec<String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    fn read(root: HKEY, key: PCWSTR) -> Option<String> {
        let mut size = 0u32;
        // SAFETY: a size query without a buffer; only `size` is written.
        let status = unsafe { RegGetValueW(root, key, w!("Path"), RRF_RT_REG_SZ, None, None, Some(&mut size)) };
        if status.is_err() || size == 0 {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        // SAFETY: `buf` holds at least `size` bytes, the length passed in.
        let status = unsafe {
            RegGetValueW(root, key, w!("Path"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
        };
        if status.is_err() {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    [
        read(HKEY_LOCAL_MACHINE, w!(r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment")),
        read(HKEY_CURRENT_USER, w!("Environment")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(not(windows))]
fn registry_path() -> Vec<String> {
    Vec::new()
}

/// Finds an executable on the current PATH (plus common per-user install
/// folders), trying `.exe`, then `.cmd` / `.bat` shims.
pub fn which(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = env::split_paths(&search_path()).collect();
    // Installers that put CLIs here don't always add the folder to PATH.
    if let Some(home) = env::var_os("USERPROFILE").map(PathBuf::from) {
        dirs.push(home.join(r".local\bin"));
    }
    if let Some(appdata) = env::var_os("APPDATA").map(PathBuf::from) {
        dirs.push(appdata.join("npm"));
    }
    for ext in ["exe", "cmd", "bat"] {
        for dir in &dirs {
            let candidate = dir.join(format!("{name}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// A command for a resolved executable, including npm's `.cmd` shims.
///
/// COMPAT: for batch files std builds the `cmd.exe` command line itself,
/// with quoting that survives spaces in the path (`C:\Users\Jane Doe\…`), and
/// refuses arguments it can't pass safely, such as ones with line breaks.
/// Wrapping shims in `cmd /C` by hand broke on both.
pub fn tool(exe: &Path) -> Command {
    command(exe)
}

/// Runs a command line (a project's test or lint command) through the shell
/// exactly as the user typed it.
///
/// COMPAT: `/S` makes `cmd.exe` strip only the outer quotes Pill adds, so
/// quotes inside the line (`npm test -- --grep "a b"`) reach the program
/// unchanged; std's own quoting would escape them with backslashes, which
/// `cmd.exe` doesn't understand.
pub fn shell(line: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = command("cmd.exe");
        cmd.raw_arg(format!("/D /S /C \"{line}\""));
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = command("sh");
        cmd.args(["-c", line]);
        cmd
    }
}

/// Runs a command to completion and returns trimmed stdout, or an error
/// containing stderr (or stdout when stderr is empty).
pub fn run(mut cmd: Command) -> Result<String, String> {
    let out: Output = cmd.output().map_err(|e| spawn_error(cmd.get_program(), cmd.get_current_dir(), &e))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if err.is_empty() { String::from_utf8_lossy(&out.stdout).trim().to_string() } else { err };
        Err(if msg.is_empty() { format!("exited with {}", out.status) } else { msg })
    }
}

/// Explains why a program couldn't start: its working folder is gone, it
/// isn't installed, or something else the OS reported.
pub fn spawn_error(program: &OsStr, cwd: Option<&Path>, e: &std::io::Error) -> String {
    if let Some(dir) = cwd.filter(|d| !d.is_dir()) {
        return format!("The folder {} no longer exists. It may have been moved, renamed or deleted.", dir.display());
    }
    let name = Path::new(program).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if e.kind() == std::io::ErrorKind::NotFound {
        return if name.eq_ignore_ascii_case("git") {
            GIT_MISSING.to_string()
        } else {
            format!("{name} isn't installed or isn't on PATH.")
        };
    }
    format!("Couldn't start {name}: {e}")
}

/// Runs `git` with `args` in `cwd`.
pub fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = command("git");
    // COMPAT: without core.longpaths, Git for Windows can't check out files
    // whose full path passes 260 characters, which deep trees reach quickly
    // inside a worktree folder. Other platforms ignore the setting.
    cmd.current_dir(cwd).args(["-c", "core.longpaths=true"]).args(args);
    run(cmd).map_err(|e| {
        if e == GIT_MISSING || e.starts_with("The folder ") {
            e
        } else {
            format!("git {}: {e}", args.first().copied().unwrap_or_default())
        }
    })
}

/// Kills an entire process tree on Stop or timeout.
///
/// Killing only the direct child would leave everything it spawned (test
/// runners, dev servers, compilers) running. A Job Object with
/// kill-on-close terminates the whole tree, including when Pill exits.
#[cfg(windows)]
pub mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// An owned Job Object handle.
    pub struct Job(HANDLE);

    // SAFETY: a job handle is a kernel object handle, usable from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// Creates a job whose processes all die when it's closed or terminated.
        pub fn new() -> windows::core::Result<Self> {
            unsafe {
                let handle = CreateJobObjectW(None, None)?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )?;
                Ok(Self(handle))
            }
        }

        /// Puts a freshly spawned child (and its future children) in the job.
        pub fn assign(&self, child: &Child) -> windows::core::Result<()> {
            unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())) }
        }

        /// Kills every process in the job.
        pub fn kill(&self) {
            unsafe {
                let _ = TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = env::temp_dir().join(format!("pill-process-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_git_and_runs_it() {
        let out = run({
            let mut c = command("git");
            c.arg("--version");
            c
        })
        .unwrap();
        assert!(out.starts_with("git version"));
    }

    #[test]
    fn reports_stderr_on_failure() {
        let err = git(Path::new("."), &["definitely-not-a-git-command"]).unwrap_err();
        assert!(err.contains("git definitely-not-a-git-command"));
    }

    #[test]
    fn missing_programs_and_folders_read_as_sentences() {
        let err = run(command("pill-no-such-program-xyz")).unwrap_err();
        assert_eq!(err, "pill-no-such-program-xyz isn't installed or isn't on PATH.");
        let gone = env::temp_dir().join("pill-process-deleted-folder-xyz");
        let err = git(&gone, &["status"]).unwrap_err();
        assert!(err.starts_with("The folder ") && err.contains("no longer exists"), "{err}");
        let not_found = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(spawn_error(OsStr::new("git"), None, &not_found), GIT_MISSING);
    }

    #[cfg(windows)]
    #[test]
    fn merges_new_path_entries_once() {
        let merged = merge_path(OsStr::new(r"C:\a;C:\b\"), &[r"c:\B;C:\c".into(), r"C:\c\;;C:\d".into()]);
        assert_eq!(merged, OsString::from(r"C:\a;C:\b\;C:\c;C:\d"));
        assert_eq!(merge_path(OsStr::new(""), &[r"C:\x".into()]), OsString::from(r"C:\x"));
    }

    #[cfg(windows)]
    #[test]
    fn search_path_keeps_the_process_path_first() {
        let own: Vec<PathBuf> = env::split_paths(&env::var_os("PATH").unwrap_or_default()).filter(|p| !p.as_os_str().is_empty()).collect();
        let fresh: Vec<PathBuf> = env::split_paths(&search_path()).collect();
        assert!(fresh.len() >= own.len());
        assert_eq!(&fresh[..own.len().min(3)], &own[..own.len().min(3)]);
    }

    /// npm installs CLIs as `.cmd` shims, often under a profile folder with a
    /// space in it; quoted arguments must arrive intact.
    #[cfg(windows)]
    #[test]
    fn batch_shims_in_folders_with_spaces_get_their_arguments() {
        let dir = tmp("shim").join("Jane Doe");
        std::fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("echo-args.cmd");
        std::fs::write(&shim, "@echo off\r\necho %*\r\n").unwrap();
        let mut cmd = tool(&shim);
        cmd.args(["--config", "sandbox_mode=\"workspace-write\"", "Bash(git status:*)"]);
        let out = run(cmd).unwrap();
        assert!(out.contains("sandbox_mode=") && out.contains("Bash(git status:*)"), "{out}");
        let mut multi = tool(&shim);
        multi.arg("two\nlines");
        assert!(run(multi).is_err());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn shell_lines_keep_their_inner_quotes() {
        let out = run(shell(r#"echo "two words" & echo done"#)).unwrap();
        assert_eq!(out.lines().map(str::trim).collect::<Vec<_>>(), vec![r#""two words""#, "done"]);
        assert!(run(shell("exit 3")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn job_kills_the_child() {
        let mut child = command("cmd.exe").args(["/C", "ping -n 30 127.0.0.1 >NUL"]).spawn().unwrap();
        let job = job::Job::new().unwrap();
        job.assign(&child).unwrap();
        job.kill();
        let status = child.wait().unwrap();
        assert!(!status.success());
    }
}
