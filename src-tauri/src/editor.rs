//! Finds and launches the user's code editor.
//!
//! Editors are launched through their GUI executables rather than the
//! `code`/`cursor` CLI shims: the shims are `.cmd` batch files, which would
//! need `cmd.exe` and flash a console window.

use crate::model::EditorChoice;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A resolved editor ready to launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editor {
    Cursor(PathBuf),
    Code(PathBuf),
    Explorer,
}

fn first_existing(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|p| p.is_file())
}

fn path_dirs() -> Vec<PathBuf> {
    env::var_os("PATH").map(|p| env::split_paths(&p).collect()).unwrap_or_default()
}

fn env_path(var: &str) -> Option<PathBuf> {
    env::var_os(var).map(PathBuf::from)
}

/// Looks for Cursor in its default install locations, then via the `cursor`
/// shim on PATH (`<install>\resources\app\bin\cursor.cmd`).
pub fn find_cursor() -> Option<PathBuf> {
    let mut c = Vec::new();
    if let Some(local) = env_path("LOCALAPPDATA") {
        c.push(local.join(r"Programs\cursor\Cursor.exe"));
    }
    if let Some(pf) = env_path("ProgramFiles") {
        c.push(pf.join(r"Cursor\Cursor.exe"));
    }
    for dir in path_dirs() {
        if dir.join("cursor.cmd").is_file() {
            c.push(dir.join(r"..\..\..\Cursor.exe"));
        }
    }
    first_existing(c).map(|p| p.canonicalize().unwrap_or(p))
}

/// Looks for VS Code in its default install locations, then via the `code`
/// shim on PATH (`<install>\bin\code.cmd`).
pub fn find_code() -> Option<PathBuf> {
    let mut c = Vec::new();
    if let Some(local) = env_path("LOCALAPPDATA") {
        c.push(local.join(r"Programs\Microsoft VS Code\Code.exe"));
    }
    if let Some(pf) = env_path("ProgramFiles") {
        c.push(pf.join(r"Microsoft VS Code\Code.exe"));
    }
    for dir in path_dirs() {
        if dir.join("code.cmd").is_file() {
            c.push(dir.join(r"..\Code.exe"));
        }
    }
    first_existing(c).map(|p| p.canonicalize().unwrap_or(p))
}

/// Resolves a project's editor choice. `Auto` prefers Cursor, then VS Code,
/// then Explorer; an explicit choice that isn't installed falls back the same way.
pub fn resolve(choice: EditorChoice) -> Editor {
    fn cursor() -> Option<Editor> {
        find_cursor().map(Editor::Cursor)
    }
    fn code() -> Option<Editor> {
        find_code().map(Editor::Code)
    }
    let found = match choice {
        EditorChoice::Explorer => return Editor::Explorer,
        EditorChoice::Code => code().or_else(cursor),
        EditorChoice::Cursor | EditorChoice::Auto => cursor().or_else(code),
    };
    found.unwrap_or(Editor::Explorer)
}

/// Opens `folder` in `editor`.
pub fn open(editor: &Editor, folder: &Path) -> std::io::Result<()> {
    let mut cmd = match editor {
        Editor::Cursor(exe) | Editor::Code(exe) => Command::new(exe),
        Editor::Explorer => Command::new("explorer.exe"),
    };
    cmd.arg(folder).spawn().map(|_| ())
}

/// Short display name of an editor.
pub fn label(editor: &Editor) -> &'static str {
    match editor {
        Editor::Cursor(_) => "Cursor",
        Editor::Code(_) => "VS Code",
        Editor::Explorer => "Explorer",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_explorer_never_searches() {
        assert_eq!(resolve(EditorChoice::Explorer), Editor::Explorer);
    }
}
