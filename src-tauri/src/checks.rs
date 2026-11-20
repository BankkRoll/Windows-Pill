//! Finds how a project runs its tests and lint from the files it already
//! has: `package.json` scripts, `Cargo.toml`, `pyproject.toml`, `go.mod`,
//! .NET solutions, Deno, a Makefile.
//!
//! Used to offer a test command when a project has none set, and to verify
//! a run that set tests up itself (the command then exists only in the
//! run's worktree).

use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

/// A command found in the project, and the file it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub command: String,
    /// File the command was read from, e.g. `package.json`.
    pub source: String,
}

/// Test and lint commands found in a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    pub test: Option<Found>,
    pub lint: Option<Found>,
}

/// What `npm init` writes as the test script: a placeholder that always fails.
const NPM_PLACEHOLDER: &str = "no test specified";

/// Looks for test and lint commands in `dir`. Nothing runs; only files are read.
///
/// # Examples
///
/// ```ignore
/// let found = checks::detect(Path::new(r"C:\dev\game"));
/// assert_eq!(found.test.unwrap().command, "pnpm test");
/// ```
pub fn detect(dir: &Path) -> Detected {
    node(dir)
        .or_else(|| rust(dir))
        .or_else(|| python(dir))
        .or_else(|| go(dir))
        .or_else(|| dotnet(dir))
        .or_else(|| deno(dir))
        .or_else(|| make(dir))
        .unwrap_or_default()
}

fn found(command: impl Into<String>, source: &str) -> Option<Found> {
    Some(Found { command: command.into(), source: source.into() })
}

fn node(dir: &Path) -> Option<Detected> {
    let text = fs::read_to_string(dir.join("package.json")).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    let scripts = &json["scripts"];
    let pm = if dir.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if dir.join("yarn.lock").is_file() {
        "yarn"
    } else if dir.join("bun.lockb").is_file() || dir.join("bun.lock").is_file() {
        "bun"
    } else {
        "npm"
    };
    let script = |name: &str| scripts[name].as_str().map(str::trim).filter(|s| !s.is_empty());
    let test = script("test")
        .filter(|s| !s.contains(NPM_PLACEHOLDER))
        // `bun test` is Bun's own runner, not the script.
        .and_then(|_| found(if pm == "bun" { "bun run test".into() } else { format!("{pm} test") }, "package.json"));
    let lint = ["lint", "typecheck", "check"]
        .into_iter()
        .find(|name| script(name).is_some())
        .and_then(|name| found(if pm == "npm" || pm == "bun" { format!("{pm} run {name}") } else { format!("{pm} {name}") }, "package.json"));
    // NOTE: a package.json with neither script still wins over the checks
    // below: a JS project that also has a Makefile tests through npm.
    Some(Detected { test, lint })
}

fn rust(dir: &Path) -> Option<Detected> {
    dir.join("Cargo.toml").is_file().then(|| Detected { test: found("cargo test", "Cargo.toml"), lint: None })
}

fn python(dir: &Path) -> Option<Detected> {
    let pyproject = fs::read_to_string(dir.join("pyproject.toml")).unwrap_or_default();
    let pytest = pyproject.contains("pytest")
        || dir.join("pytest.ini").is_file()
        || dir.join("conftest.py").is_file()
        || fs::read_to_string(dir.join("setup.cfg")).is_ok_and(|s| s.contains("[tool:pytest]"));
    let has_tests = dir.join("tests").is_dir() || dir.join("test").is_dir();
    if pyproject.is_empty() && !pytest && !dir.join("setup.py").is_file() {
        return None;
    }
    let source = if pyproject.is_empty() { "setup.py" } else { "pyproject.toml" };
    let runner = if dir.join("uv.lock").is_file() {
        "uv run "
    } else if dir.join("poetry.lock").is_file() {
        "poetry run "
    } else {
        ""
    };
    let test = if pytest || has_tests { found(format!("{runner}pytest"), source) } else { None };
    let lint = pyproject.contains("[tool.ruff").then(|| Found { command: format!("{runner}ruff check ."), source: "pyproject.toml".into() });
    Some(Detected { test, lint })
}

fn go(dir: &Path) -> Option<Detected> {
    dir.join("go.mod").is_file().then(|| Detected { test: found("go test ./...", "go.mod"), lint: found("go vet ./...", "go.mod") })
}

fn dotnet(dir: &Path) -> Option<Detected> {
    let entry = fs::read_dir(dir).ok()?.flatten().find(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        name.ends_with(".sln") || name.ends_with(".slnx") || name.ends_with(".csproj")
    })?;
    Some(Detected { test: found("dotnet test", &entry.file_name().to_string_lossy()), lint: None })
}

fn deno(dir: &Path) -> Option<Detected> {
    let source = ["deno.json", "deno.jsonc"].into_iter().find(|f| dir.join(f).is_file())?;
    Some(Detected { test: found("deno test", source), lint: found("deno lint", source) })
}

fn make(dir: &Path) -> Option<Detected> {
    let text = fs::read_to_string(dir.join("Makefile")).ok()?;
    let target = |name: &str| text.lines().any(|l| l.strip_prefix(name).is_some_and(|rest| rest.starts_with(':')));
    let test = target("test").then(|| Found { command: "make test".into(), source: "Makefile".into() });
    let lint = target("lint").then(|| Found { command: "make lint".into(), source: "Makefile".into() });
    (test.is_some() || lint.is_some()).then_some(Detected { test, lint })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Scratch(PathBuf);

    impl Scratch {
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn dir(files: &[(&str, &str)]) -> Scratch {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = Scratch(std::env::temp_dir().join(format!("pill-checks-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))));
        let _ = fs::remove_dir_all(d.path());
        fs::create_dir_all(d.path()).unwrap();
        for (name, body) in files {
            let path = d.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, body).unwrap();
        }
        d
    }

    fn test_of(files: &[(&str, &str)]) -> Option<String> {
        detect(dir(files).path()).test.map(|f| f.command)
    }

    #[test]
    fn node_uses_the_lockfiles_package_manager() {
        let pkg = r#"{"scripts":{"test":"vitest run","lint":"eslint ."}}"#;
        let d = dir(&[("package.json", pkg), ("pnpm-lock.yaml", "")]);
        let found = detect(d.path());
        assert_eq!(found.test.unwrap().command, "pnpm test");
        assert_eq!(found.lint.unwrap().command, "pnpm lint");
        assert_eq!(test_of(&[("package.json", pkg)]).as_deref(), Some("npm test"));
        assert_eq!(test_of(&[("package.json", pkg), ("yarn.lock", "")]).as_deref(), Some("yarn test"));
        assert_eq!(test_of(&[("package.json", pkg), ("bun.lock", "")]).as_deref(), Some("bun run test"));
    }

    #[test]
    fn npm_init_placeholder_is_not_a_test_command() {
        let pkg = r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#;
        assert_eq!(test_of(&[("package.json", pkg)]), None);
    }

    #[test]
    fn a_js_project_without_scripts_does_not_fall_through_to_make() {
        assert_eq!(test_of(&[("package.json", "{}"), ("Makefile", "test:\n\techo hi\n")]), None);
    }

    #[test]
    fn other_stacks() {
        assert_eq!(test_of(&[("Cargo.toml", "[package]")]).as_deref(), Some("cargo test"));
        assert_eq!(test_of(&[("go.mod", "module x")]).as_deref(), Some("go test ./..."));
        assert_eq!(test_of(&[("Game.sln", "")]).as_deref(), Some("dotnet test"));
        assert_eq!(test_of(&[("deno.json", "{}")]).as_deref(), Some("deno test"));
        assert_eq!(test_of(&[("Makefile", "build:\n\tcc\ntest: build\n\t./t\n")]).as_deref(), Some("make test"));
        assert_eq!(test_of(&[("README.md", "hi")]), None);
    }

    #[test]
    fn python_needs_pytest_or_a_tests_folder() {
        assert_eq!(test_of(&[("pyproject.toml", "[project]\nname='x'")]), None);
        assert_eq!(test_of(&[("pyproject.toml", "[project]"), ("tests/test_a.py", "")]).as_deref(), Some("pytest"));
        assert_eq!(
            test_of(&[("pyproject.toml", "[tool.pytest.ini_options]"), ("uv.lock", "")]).as_deref(),
            Some("uv run pytest")
        );
        assert_eq!(test_of(&[("pytest.ini", ""), ("poetry.lock", "")]).as_deref(), Some("poetry run pytest"));
    }
}
