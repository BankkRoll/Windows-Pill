//! Model and reasoning-effort choices for each provider.
//!
//! Nothing is hard-coded: Claude Code reports its models in the response to
//! an `initialize` control request (the way the IDE extensions get them),
//! and Codex prints its catalog with `codex debug models`. Both lists follow
//! CLI updates and the user's plan.
//!
//! A choice resolves chat → project → global; an empty string at every level
//! means "let the CLI decide", and no flag is passed.

use crate::config::Config;
use crate::process;
use crate::provider::Provider;
use crate::workspace::Project;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// One selectable model.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// Value passed to `--model`.
    pub id: String,
    pub label: String,
    pub description: String,
    /// Reasoning efforts this model accepts, lowest first.
    pub efforts: Vec<String>,
    /// Effort the CLI uses when none is given.
    pub default_effort: Option<String>,
}

/// Every model Pill can offer, per provider.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub claude: Vec<ModelInfo>,
    /// What Claude Code's "Default" currently means, e.g. `Opus 5.5 · Best for…`.
    pub claude_default: Option<String>,
    /// Why the Claude list is empty (not installed, no answer).
    pub claude_error: Option<String>,
    pub codex: Vec<ModelInfo>,
    /// Why the Codex list is empty (not installed, catalog unreadable).
    pub codex_error: Option<String>,
}

/// A model and effort; empty strings defer to the next level.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelChoice {
    pub model: String,
    pub effort: String,
}

/// What a single agent run uses: a provider (or the project's), plus a model
/// and effort that, when empty, fall back like any other choice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunChoice {
    pub provider: Option<Provider>,
    pub model: String,
    pub effort: String,
}

impl RunChoice {
    /// The model and effort part.
    pub fn model_choice(&self) -> ModelChoice {
        ModelChoice { model: self.model.clone(), effort: self.effort.clone() }
    }

    /// Whether the model and effort are safe to pass to a CLI.
    pub fn is_valid(&self) -> bool {
        valid_model(&self.model) && valid_effort(&self.effort)
    }
}

/// Global defaults per provider.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelDefaults {
    pub claude: ModelChoice,
    pub codex: ModelChoice,
}

impl ModelDefaults {
    /// The defaults for `provider`.
    pub fn get(&self, provider: Provider) -> &ModelChoice {
        match provider {
            Provider::Claude => &self.claude,
            Provider::Codex => &self.codex,
        }
    }

    /// Drops values that would be rejected on the command line.
    pub fn sanitized(mut self) -> Self {
        for c in [&mut self.claude, &mut self.codex] {
            if !valid_model(&c.model) {
                c.model.clear();
            }
            if !valid_effort(&c.effort) {
                c.effort.clear();
            }
        }
        self
    }
}


/// One entry of Claude Code's `initialize` response.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeModel {
    value: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    supports_effort: bool,
    #[serde(default)]
    supported_effort_levels: Vec<String>,
}

/// Parses the `models` array from Claude Code's `initialize` response. The
/// `default` entry becomes the Default row's description, not a model.
fn parse_claude_models(models: &serde_json::Value) -> (Vec<ModelInfo>, Option<String>) {
    let list: Vec<ClaudeModel> = serde_json::from_value(models.clone()).unwrap_or_default();
    let mut default = None;
    let mut out = Vec::new();
    for m in list {
        if m.value == "default" {
            default = m.description;
            continue;
        }
        if !valid_model(&m.value) {
            continue;
        }
        out.push(ModelInfo {
            label: m.display_name.unwrap_or_else(|| m.value.clone()),
            id: m.value,
            description: m.description.unwrap_or_default(),
            efforts: if m.supports_effort {
                m.supported_effort_levels.into_iter().filter(|e| valid_effort(e)).collect()
            } else {
                Vec::new()
            },
            default_effort: None,
        });
    }
    (out, default)
}

/// Asks the installed Claude Code for its models the way the IDE extensions
/// do: an `initialize` control request over the stream-json protocol. No
/// prompt is sent, so it costs nothing.
fn claude_models_live() -> Result<(Vec<ModelInfo>, Option<String>), String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    let exe = process::which("claude").ok_or("Claude Code isn't installed")?;
    let mut cmd = process::tool(&exe);
    cmd.args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
        .env_remove("ANTHROPIC_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start Claude Code: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    stdin
        .write_all(b"{\"type\":\"control_request\",\"request_id\":\"pill-models\",\"request\":{\"subtype\":\"initialize\"}}\n")
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if v["type"] == "control_response" && v["response"]["request_id"] == "pill-models" {
                    let _ = tx.send(v["response"]["response"]["models"].clone());
                    return;
                }
            }
        }
    });
    let answer = rx.recv_timeout(std::time::Duration::from_secs(20));
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let models = answer.map_err(|_| "Claude Code didn't list its models".to_string())?;
    let (list, default) = parse_claude_models(&models);
    if list.is_empty() {
        return Err("Claude Code listed no models".into());
    }
    Ok((list, default))
}

// PERF: each lookup starts the CLI, so the list is kept until refreshed.
static CLAUDE_CACHE: Mutex<Option<(Vec<ModelInfo>, Option<String>)>> = Mutex::new(None);

fn claude_models(refresh: bool) -> Result<(Vec<ModelInfo>, Option<String>), String> {
    if !refresh {
        if let Some(cached) = CLAUDE_CACHE.lock().unwrap().clone() {
            return Ok(cached);
        }
    }
    let fresh = claude_models_live()?;
    *CLAUDE_CACHE.lock().unwrap() = Some(fresh.clone());
    Ok(fresh)
}

#[derive(Deserialize)]
struct CodexCatalog {
    models: Vec<CodexModel>,
}

#[derive(Deserialize)]
struct CodexModel {
    slug: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    default_reasoning_level: Option<String>,
    #[serde(default)]
    supported_reasoning_levels: Vec<CodexEffort>,
    #[serde(default)]
    visibility: Option<String>,
}

#[derive(Deserialize)]
struct CodexEffort {
    effort: String,
}

/// Parses `codex debug models`, keeping the models Codex itself lists.
fn parse_codex_catalog(json: &str) -> Result<Vec<ModelInfo>, String> {
    let catalog: CodexCatalog = serde_json::from_str(json).map_err(|e| format!("unreadable Codex model list: {e}"))?;
    Ok(catalog
        .models
        .into_iter()
        .filter(|m| m.visibility.as_deref().unwrap_or("list") == "list" && valid_model(&m.slug))
        .map(|m| ModelInfo {
            label: m.display_name.unwrap_or_else(|| m.slug.clone()),
            id: m.slug,
            description: m.description.unwrap_or_default(),
            efforts: m.supported_reasoning_levels.into_iter().map(|e| e.effort).filter(|e| valid_effort(e)).collect(),
            default_effort: m.default_reasoning_level,
        })
        .collect())
}

// PERF: `codex debug models` prints about 1 MB and takes a moment, so the
// parsed list is kept until the user asks to refresh it.
static CODEX_CACHE: Mutex<Option<Vec<ModelInfo>>> = Mutex::new(None);

fn codex_models(refresh: bool) -> Result<Vec<ModelInfo>, String> {
    if !refresh {
        if let Some(cached) = CODEX_CACHE.lock().unwrap().clone() {
            return Ok(cached);
        }
    }
    let exe = process::which("codex").ok_or("Codex isn't installed")?;
    let mut cmd = process::tool(&exe);
    cmd.args(["debug", "models"]);
    let out = process::run(cmd)?;
    let models = parse_codex_catalog(&out)?;
    *CODEX_CACHE.lock().unwrap() = Some(models.clone());
    Ok(models)
}

/// Both providers' models. Slow on the first call (runs Codex).
pub fn catalog(refresh: bool) -> Catalog {
    // Both CLIs take a moment to answer, so ask them at the same time.
    let claude = std::thread::spawn(move || claude_models(refresh));
    let (codex, codex_error) = match codex_models(refresh) {
        Ok(m) => (m, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let ((claude, claude_default), claude_error) = match claude.join().unwrap_or_else(|_| Err("model lookup crashed".into())) {
        Ok(c) => (c, None),
        Err(e) => ((Vec::new(), None), Some(e)),
    };
    Catalog { claude, claude_default, claude_error, codex, codex_error }
}

/// SECURITY: model names reach a command line, so only plain identifier
/// characters are allowed; a leading `-` could otherwise smuggle in a flag.
pub fn valid_model(s: &str) -> bool {
    s.is_empty()
        || (s.len() <= 80
            && !s.starts_with('-')
            && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/' | '[' | ']')))
}

/// Same rule for efforts: short lowercase words only.
pub fn valid_effort(s: &str) -> bool {
    s.len() <= 16 && s.chars().all(|c| c.is_ascii_lowercase())
}

/// Resolves the model and effort for a run or chat: the explicit choice
/// first, then the project's, then the global default for its provider.
pub fn resolve(config: &Config, project: &Project, provider: Provider, explicit: &ModelChoice) -> (Option<String>, Option<String>) {
    let global = config.models.get(provider);
    let pick = |a: &str, b: &str, c: &str| {
        [a, b, c].into_iter().find(|s| !s.is_empty()).map(str::to_string)
    };
    // A project's model override names a model of its own provider; it means
    // nothing (or the wrong thing) when a chat or run uses the other one.
    let own = project.settings.provider == provider;
    let (project_model, project_effort) = if own { (project.settings.model.as_str(), project.settings.effort.as_str()) } else { ("", "") };
    let model = pick(&explicit.model, project_model, &global.model).filter(|m| valid_model(m));
    let effort = pick(&explicit.effort, project_effort, &global.effort).filter(|e| valid_effort(e));
    (model, effort)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_catalog_and_hides_internal_models() {
        let json = r#"{"models":[
          {"slug":"gpt-6.2-sol","display_name":"GPT-6.2-Sol","description":"Fast","default_reasoning_level":"low",
           "supported_reasoning_levels":[{"effort":"low"},{"effort":"max"}],"visibility":"list","extra":1},
          {"slug":"codex-auto-review","visibility":"hide","supported_reasoning_levels":[]}
        ]}"#;
        let models = parse_codex_catalog(json).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-6.2-sol");
        assert_eq!(models[0].efforts, vec!["low", "max"]);
        assert_eq!(models[0].default_effort.as_deref(), Some("low"));
    }

    #[test]
    fn parses_claude_initialize_models() {
        let v = serde_json::json!([
            {"value": "default", "displayName": "Default (recommended)", "description": "Opus 5.5 · Best for everyday"},
            {"value": "opus", "displayName": "Opus 5.5", "description": "Complex work", "supportsEffort": true,
             "supportedEffortLevels": ["low", "max"]},
            {"value": "claude-haiku-4-5-20251001", "displayName": "Haiku 4.5", "description": "Fast"},
            {"value": "--bad", "displayName": "Bad"}
        ]);
        let (models, default) = parse_claude_models(&v);
        assert_eq!(default.as_deref(), Some("Opus 5.5 · Best for everyday"));
        assert_eq!(models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["opus", "claude-haiku-4-5-20251001"]);
        assert_eq!(models[0].efforts, ["low", "max"]);
        assert!(models[1].efforts.is_empty());
    }

    #[test]
    fn rejects_flag_like_model_names() {
        assert!(valid_model("claude-opus-5-5"));
        assert!(valid_model("opus[1m]"));
        assert!(valid_model(""));
        assert!(!valid_model("--dangerously-skip-permissions"));
        assert!(!valid_model("opus; rm"));
        assert!(!valid_effort("HIGH"));
        assert!(valid_effort("xhigh"));
    }
}
