//! Runs one external plugin process and speaks the JSON lines protocol.
//!
//! The process starts suspended with no console window, joins a Job Object
//! and only then runs, so turning the plugin off (or quitting Pill) ends it
//! and everything it spawned. It gets a cleared environment with only the
//! variables in [`INHERITED_ENV`] plus Pill's own. stdout is read line by
//! line with a hard size cap and a rate limit; stderr is kept only as a
//! short tail for error messages; stdin is fed through a bounded queue.

use super::manifest::Manifest;
use super::page::MAX_PAGE_LINE_BYTES;
use super::protocol::{self, HostMessage, PageRef};
use super::{Fix, Runtime, Sink, Status, StatusState};
use std::ffi::OsString;
use std::io::{BufRead, Read};
use std::path::Path;

/// Lines accepted per second before the rest of that second is dropped.
pub(crate) const MAX_LINES_PER_SEC: u32 = 50;
/// Bytes of stderr kept for the error message after a crash.
const STDERR_TAIL: usize = 2048;
/// Messages waiting for the plugin to read its stdin; more are refused.
const STDIN_QUEUE: usize = 64;
/// Lines read from stdout but not yet handled.
const STDOUT_QUEUE: usize = 64;

/// Variables of Pill's environment a plugin inherits; everything else is
/// cleared. These are what Windows programs and runtimes such as Node and
/// Python need: system and profile folders, temp folders, the shell and
/// the processor. `PATH` and the `PILL_*` variables are set separately.
///
/// SECURITY: Pill's environment can hold credentials for the tools it
/// drives (API keys, `GH_TOKEN`, cloud and proxy settings). A denylist
/// misses whatever it doesn't name, so plugins get an allowlist instead.
pub const INHERITED_ENV: &[&str] = &[
    "SystemRoot",
    "windir",
    "SystemDrive",
    "PATHEXT",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "ComSpec",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "CommonProgramW6432",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
    "USERNAME",
    "COMPUTERNAME",
];

/// The complete environment of a plugin: [`INHERITED_ENV`] as Pill has
/// them, Pill's search `PATH` and the `PILL_*` variables.
///
/// @param id The manifest id.
/// @param data The plugin's data folder.
pub fn plugin_env(id: &str, data: &Path) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> =
        INHERITED_ENV.iter().filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(name), value))).collect();
    env.push(("PATH".into(), crate::process::search_path()));
    env.push(("PILL_PLUGIN_ID".into(), id.into()));
    env.push(("PILL_PROTOCOL".into(), protocol::PROTOCOL_VERSION.to_string().into()));
    env.push(("PILL_PLUGIN_DATA".into(), data.into()));
    env
}

/// Outcome of reading one line with [`read_capped_line`].
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    /// A complete line within the limit (newline and `\r` removed).
    Ok(String),
    /// A line over the limit; its bytes were discarded.
    TooLong(usize),
    /// End of stream.
    Eof,
}

/// Reads one `\n`-terminated line, never buffering more than `max` bytes:
/// longer lines are skipped to their end and reported as [`Line::TooLong`].
///
/// @param reader Buffered plugin stdout.
/// @param max Largest accepted line in bytes.
pub fn read_capped_line(reader: &mut impl BufRead, max: usize) -> std::io::Result<Line> {
    let mut buf: Vec<u8> = Vec::new();
    let mut total = 0usize;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if total == 0 {
                Line::Eof
            } else if total > max {
                Line::TooLong(total)
            } else {
                Line::Ok(finish(buf))
            });
        }
        let (part, done) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (&chunk[..i], Some(i + 1)),
            None => (chunk, None),
        };
        total += part.len();
        if total <= max {
            buf.extend_from_slice(part);
        } else {
            buf = Vec::new();
        }
        let used = done.unwrap_or(part.len());
        reader.consume(used);
        if done.is_some() {
            return Ok(if total > max { Line::TooLong(total) } else { Line::Ok(finish(buf)) });
        }
    }
}

fn finish(mut buf: Vec<u8>) -> String {
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// The command that runs the plugin, with its environment and pipes.
///
/// @param program The resolved program.
/// @param dir The plugin folder, used as the working directory.
/// @param data The plugin's data folder.
/// @param manifest The approved manifest.
#[cfg(windows)]
fn command(program: &Path, dir: &Path, data: &Path, manifest: &Manifest) -> std::process::Command {
    use std::process::Stdio;
    let mut cmd = crate::process::tool(program);
    cmd.args(&manifest.args)
        .current_dir(dir)
        .env_clear()
        .envs(plugin_env(&manifest.id, data))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Starts `cmd` suspended, puts it in a new Job Object and lets it run.
///
/// SECURITY: a process that runs before it joins the job can start a child
/// outside it, which then survives turning the plugin off. Suspended, it
/// can't run any code until it is contained.
#[cfg(windows)]
fn spawn_contained(mut cmd: std::process::Command) -> Result<(std::process::Child, crate::process::job::Job), String> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

    // NOTE: NtResumeProcess is exported by ntdll but not covered by the
    // windows crate's features; the alternative, walking the process's
    // threads with Toolhelp32, needs another feature and does the same.
    #[link(name = "ntdll", kind = "raw-dylib")]
    extern "system" {
        fn NtResumeProcess(process: HANDLE) -> i32;
    }

    // NOTE: creation_flags replaces the flags crate::process::command set,
    // so the no-window flag is repeated here.
    cmd.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let contained = crate::process::job::Job::new()
        .and_then(|job| job.assign(&child).map(|_| job))
        .map_err(|e| format!("Couldn't contain the plugin process: {e}"))
        .and_then(|job| {
            // SAFETY: the handle belongs to `child`, which outlives the call.
            let status = unsafe { NtResumeProcess(HANDLE(child.as_raw_handle())) };
            if status < 0 {
                Err(format!("Couldn't resume the plugin process (NTSTATUS {status:#x})"))
            } else {
                Ok(job)
            }
        });
    match contained {
        Ok(job) => Ok((child, job)),
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(e)
        }
    }
}

/// Starts the plugin described by `manifest`.
///
/// @param sink Where output and status go.
/// @param dir The plugin folder, used as the working directory.
/// @param data Folder the plugin may keep its own files in, passed as
///   `PILL_PLUGIN_DATA` and created if missing.
/// @param manifest The approved manifest.
#[cfg(windows)]
pub fn start(sink: Sink, dir: &Path, data: &Path, manifest: &Manifest) -> Result<Box<dyn Runtime>, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    let program = super::manifest::strip_verbatim(manifest.resolve_program(dir)?);
    std::fs::create_dir_all(data).map_err(|e| format!("Couldn't create {}: {e}", data.display()))?;
    // SECURITY: without the job, disabling couldn't reliably end the process
    // tree, so spawn_contained refuses to run the plugin at all.
    let (mut child, job) =
        spawn_contained(command(&program, dir, data, manifest)).map_err(|e| format!("Couldn't start {}: {e}", manifest.command))?;
    let job = Arc::new(job);
    let stopped = Arc::new(AtomicBool::new(false));

    let (tx, rx) = mpsc::sync_channel::<String>(STDIN_QUEUE);
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let _ = tx.try_send(HostMessage::Hello { protocol: protocol::PROTOCOL_VERSION, pill: env!("CARGO_PKG_VERSION") }.to_line());
    spawn("plugin-stdin", move || {
        use std::io::Write;
        for line in rx {
            // NOTE: write_all blocks while the pipe is full; the bounded
            // queue in front of it is what refuses messages to a plugin
            // that stopped reading. A failed write means the pipe is gone.
            if stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()).is_err() {
                break;
            }
        }
    })?;

    let stderr = child.stderr.take().ok_or("no stderr")?;
    let tail = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let tail_w = tail.clone();
    spawn("plugin-stderr", move || {
        let mut buf = [0u8; 1024];
        let mut stderr = stderr;
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut t = tail_w.lock().unwrap();
            t.extend_from_slice(&buf[..n]);
            let excess = t.len().saturating_sub(STDERR_TAIL);
            t.drain(..excess);
        }
    })?;

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (lines_tx, lines) = mpsc::sync_channel::<Line>(STDOUT_QUEUE);
    spawn("plugin-stdout", move || {
        let mut reader = std::io::BufReader::new(stdout);
        // NOTE: `page` and `nav` lines may be this long; parse_line holds
        // every other message to the 8 KB limit.
        while let Ok(line @ (Line::Ok(_) | Line::TooLong(_))) = read_capped_line(&mut reader, MAX_PAGE_LINE_BYTES) {
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    })?;

    let stopped_r = stopped.clone();
    spawn("plugin-lines", move || {
        sink.status(Status::plain(StatusState::Running));
        let second = Duration::from_secs(1);
        let mut window = Instant::now();
        let mut in_window = 0u32;
        let mut dropped = 0u32;
        loop {
            // NOTE: waking at the end of each second reports dropped lines
            // even when the plugin then goes quiet.
            let next = lines.recv_timeout(second.saturating_sub(window.elapsed()));
            if window.elapsed() >= second {
                if dropped > 0 {
                    report(&sink, format!("Too chatty: ignored {dropped} lines (limit {MAX_LINES_PER_SEC} per second)"));
                }
                window = Instant::now();
                in_window = 0;
                dropped = 0;
            }
            let line = match next {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            in_window += 1;
            if in_window > MAX_LINES_PER_SEC {
                dropped += 1;
                continue;
            }
            match line {
                Line::Ok(text) if text.trim().is_empty() => {}
                Line::Ok(text) => {
                    if let Err(e) = protocol::parse_line(&text).and_then(|m| sink.apply(m)) {
                        report(&sink, format!("Ignored a line: {e}"));
                    }
                }
                Line::TooLong(n) => report(&sink, format!("Ignored a {n}-byte line (limit {MAX_PAGE_LINE_BYTES})")),
                Line::Eof => break,
            }
        }
        let status = child.wait();
        if stopped_r.load(Ordering::SeqCst) {
            return;
        }
        sink.clear();
        let tail = String::from_utf8_lossy(&tail.lock().unwrap()).trim().to_string();
        let last = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default();
        let how = match status {
            Ok(s) => match s.code() {
                Some(c) => format!("Stopped with exit code {c}"),
                None => "Stopped".to_string(),
            },
            Err(e) => format!("Stopped: {e}"),
        };
        let msg = if last.is_empty() { how } else { format!("{how}: {}", protocol::clean_text(last, 160)) };
        sink.status(Status::with(StatusState::Error, msg, Some(Fix::Restart)));
    })?;

    Ok(Box::new(External { tx, job, stopped }))
}

/// Non-Windows builds can't contain a process tree, so externals don't run.
#[cfg(not(windows))]
pub fn start(_sink: Sink, _dir: &Path, _data: &Path, _manifest: &Manifest) -> Result<Box<dyn Runtime>, String> {
    Err("External plugins need Windows".into())
}

fn report(sink: &Sink, message: String) {
    log::warn!("plugin: {message}");
    let _ = sink.apply(protocol::Message::Error(Some(protocol::clean_text(&message, protocol::MAX_ERROR))));
}

#[cfg(windows)]
fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new().name(name.into()).spawn(f).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(windows)]
struct External {
    tx: std::sync::mpsc::SyncSender<String>,
    job: std::sync::Arc<crate::process::job::Job>,
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(windows)]
impl External {
    /// Queues a message for the plugin's stdin without ever blocking, since
    /// callers hold the plugin registry.
    fn send(&self, message: HostMessage) -> Result<(), String> {
        use std::sync::mpsc::TrySendError;
        self.tx.try_send(message.to_line()).map_err(|e| match e {
            TrySendError::Full(_) => "The plugin isn't reading its input".to_string(),
            TrySendError::Disconnected(_) => "The plugin's input is closed".to_string(),
        })
    }
}

#[cfg(windows)]
impl Runtime for External {
    fn action(&mut self, id: &str) -> Result<(), String> {
        self.send(HostMessage::Action { id, item: None, page: None })
    }

    fn item_action(&mut self, item: &str, id: &str) -> Result<(), String> {
        self.send(HostMessage::Action { id, item: Some(item), page: None })
    }

    fn view(&mut self, project: Option<&str>, open: bool) {
        let _ = self.send(HostMessage::View { project, closed: !open });
    }

    fn page_action(&mut self, project: Option<&str>, item: Option<&str>, id: &str) -> Result<(), String> {
        self.send(HostMessage::Action { id, item, page: Some(PageRef { project }) })
    }

    fn projects(&mut self, projects: &[super::protocol::ProjectInfo]) {
        let _ = self.send(HostMessage::Projects { projects });
    }

    fn stop(&mut self) {
        self.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
        self.job.kill();
    }
}

#[cfg(windows)]
impl Drop for External {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_lines_and_strips_crlf() {
        let mut r = std::io::BufReader::with_capacity(4, Cursor::new(b"one\r\ntwo\nthree".to_vec()));
        assert_eq!(read_capped_line(&mut r, 100).unwrap(), Line::Ok("one".into()));
        assert_eq!(read_capped_line(&mut r, 100).unwrap(), Line::Ok("two".into()));
        assert_eq!(read_capped_line(&mut r, 100).unwrap(), Line::Ok("three".into()));
        assert_eq!(read_capped_line(&mut r, 100).unwrap(), Line::Eof);
    }

    #[test]
    fn skips_long_lines_without_buffering_them() {
        let mut data = vec![b'x'; 50];
        data.extend_from_slice(b"\nok\n");
        let mut r = std::io::BufReader::with_capacity(8, Cursor::new(data));
        assert_eq!(read_capped_line(&mut r, 10).unwrap(), Line::TooLong(50));
        assert_eq!(read_capped_line(&mut r, 10).unwrap(), Line::Ok("ok".into()));
    }

    /// Runs a real `.cmd` plugin the way [`start`] does and parses its output.
    #[cfg(windows)]
    #[test]
    fn a_batch_plugin_runs_and_speaks_the_protocol() {
        use std::process::Stdio;
        let dir = std::env::temp_dir().join(format!("pill-plugin-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.cmd"),
            "@echo off\r\necho {\"type\":\"live\",\"icon\":\"rocket\",\"text\":\"Deploying\"}\r\necho {\"type\":\"signal\",\"id\":\"ci\",\"icon\":\"warning\",\"count\":2}\r\necho not json\r\n",
        )
        .unwrap();
        let manifest = super::super::manifest::parse(r#"{"id":"e2e","name":"E2E","command":"./plugin.cmd"}"#, &[]).unwrap();
        let program = super::super::manifest::strip_verbatim(manifest.resolve_program(&dir).unwrap());
        let mut child = crate::process::tool(&program).current_dir(&dir).stdin(Stdio::null()).stdout(Stdio::piped()).spawn().unwrap();
        let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut parsed = Vec::new();
        while let Line::Ok(text) = read_capped_line(&mut reader, protocol::MAX_LINE_BYTES).unwrap() {
            parsed.push(protocol::parse_line(&text));
        }
        child.wait().unwrap();
        assert_eq!(parsed.len(), 3);
        assert!(matches!(&parsed[0], Ok(protocol::Message::Live(Some(protocol::LiveItem::Text { text, .. }))) if text == "Deploying"));
        assert!(matches!(&parsed[1], Ok(protocol::Message::Signal(s)) if s.count == 2));
        assert!(parsed[2].is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_environment_is_an_allowlist() {
        let data = Path::new(r"C:\data\x");
        let env = plugin_env("x", data);
        let names: Vec<String> = env.iter().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        for name in &names {
            assert!(INHERITED_ENV.contains(&name.as_str()) || name == "PATH" || name.starts_with("PILL_"), "{name}");
        }
        assert!(names.iter().any(|n| n == "PATH"));
        assert!(env.contains(&("PILL_PLUGIN_DATA".into(), data.into())));
        assert!(env.contains(&("PILL_PLUGIN_ID".into(), "x".into())));
        // NOTE: cargo runs tests with its own CARGO_* variables set.
        assert!(!names.iter().any(|n| n.starts_with("CARGO")));
    }

    /// Runs the shipped CVE watch plugin with the environment, flags and job
    /// [`start`] uses: it must start, read `hello` and `projects` and
    /// answer with valid messages. Skipped when Node isn't installed.
    #[cfg(windows)]
    #[test]
    fn cve_watch_runs_with_the_plugin_environment() {
        use std::io::Write;
        if crate::process::which("node").is_none() {
            eprintln!("node isn't on PATH; skipping");
            return;
        }
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("plugins").join("cve-watch");
        let dir = super::super::manifest::canonical_folder(&dir);
        let manifest = super::super::manifest::read(&dir, &[]).unwrap();
        let data = std::env::temp_dir().join(format!("pill-cve-watch-env-{}", std::process::id()));
        let program = manifest.resolve_program(&dir).unwrap();
        let (mut child, job) = spawn_contained(command(&program, &dir, &data, &manifest)).unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let projects: [protocol::ProjectInfo; 0] = [];
        stdin.write_all(HostMessage::Hello { protocol: protocol::PROTOCOL_VERSION, pill: "test" }.to_line().as_bytes()).unwrap();
        stdin.write_all(HostMessage::Projects { projects: &projects }.to_line().as_bytes()).unwrap();
        stdin.flush().unwrap();
        let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut kinds = Vec::new();
        while let Ok(Line::Ok(text)) = read_capped_line(&mut reader, MAX_PAGE_LINE_BYTES) {
            protocol::parse_line(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
            let kind = serde_json::from_str::<serde_json::Value>(&text).unwrap()["type"].as_str().unwrap_or_default().to_string();
            let done = kind == "widget";
            kinds.push(kind);
            if done {
                break;
            }
        }
        job.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&data);
        assert!(kinds.iter().any(|k| k == "widget"), "CVE watch answered with {kinds:?}");
    }

    #[cfg(windows)]
    #[test]
    fn contained_processes_end_with_their_job() {
        use std::process::Stdio;
        let mut cmd = crate::process::command("cmd.exe");
        cmd.args(["/D", "/C", "ping -n 30 127.0.0.1 >NUL"]).stdin(Stdio::null()).stdout(Stdio::null());
        let (mut child, job) = spawn_contained(cmd).unwrap();
        assert!(child.try_wait().unwrap().is_none(), "it runs once resumed");
        job.kill();
        let status = child.wait().unwrap();
        assert!(!status.success());
    }
}
