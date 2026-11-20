//! Notifications for the moments an agent needs the user, and nothing else.
//!
//! Responsibilities:
//! - Track what each Pill surface (the expanded notch, the pop-out window)
//!   is showing, as reported by the UI through `set_viewing`.
//! - Send a Windows toast for a [`Notice`] unless the user is already looking
//!   at that run or task.
//! - Build and parse the `pill://open?…` links toasts launch, and bring Pill
//!   up on the run or task a link names.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// How long a surface's report holds without a refresh. The UI re-reports
/// every 30 s while a surface is showing, so a webview that dies without
/// clearing its view stops counting soon after.
pub const VIEW_TTL: Duration = Duration::from_secs(75);

/// What one surface is showing, as reported by the UI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Viewing {
    /// Run open in the Agents tab, if any.
    pub run_id: Option<String>,
    /// Task open in the task page, or the task of the run being shown.
    pub task_id: Option<String>,
}

/// What every surface is showing right now.
#[derive(Default)]
pub struct Presence {
    views: Mutex<HashMap<String, (Viewing, Instant)>>,
}

impl Presence {
    /// Records what `surface` (a window label) shows; `None` means the
    /// surface is closed, collapsed or in the background.
    pub fn set(&self, surface: &str, viewing: Option<Viewing>) {
        let mut views = self.views.lock().unwrap();
        match viewing {
            Some(v) => {
                views.insert(surface.to_string(), (v, Instant::now()));
            }
            None => {
                views.remove(surface);
            }
        }
    }

    /// Whether any surface is showing `run_id` or `task_id` right now.
    pub fn is_viewing(&self, run_id: Option<&str>, task_id: Option<&str>) -> bool {
        let now = Instant::now();
        let views = self.views.lock().unwrap();
        watching(views.values().map(|(v, at)| (v, now.duration_since(*at))), run_id, task_id)
    }
}

/// Whether any fresh view matches the run or the task. Views older than
/// [`VIEW_TTL`] are ignored.
///
/// @param views Each surface's view and how long ago it was reported.
pub fn watching<'a>(views: impl IntoIterator<Item = (&'a Viewing, Duration)>, run_id: Option<&str>, task_id: Option<&str>) -> bool {
    views.into_iter().filter(|(_, age)| *age < VIEW_TTL).any(|(v, _)| {
        let same_run = run_id.is_some() && v.run_id.as_deref() == run_id;
        let same_task = task_id.is_some() && v.task_id.as_deref() == task_id;
        same_run || same_task
    })
}

/// Why the user is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// A plan waits for approval.
    Plan,
    /// The CLI asks to use a tool outside its allowed set.
    Tool,
    /// Work shipped and is ready for review.
    Review,
    /// A run failed and the task waits for a decision.
    Failed,
    /// Every run of a comparison finished; one is to be picked.
    Compare,
}

/// A notification about a run or task.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub need: Need,
    pub title: String,
    pub body: String,
    /// Project name, shown small under the text.
    pub project: String,
    pub project_id: Option<String>,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    /// Adds an "Open PR" button.
    pub pr_url: Option<String>,
}

/// Toasts `notice` unless the user already looks at its run or task.
pub fn send(app: &AppHandle, notice: &Notice) {
    use tauri::Manager;
    let presence = app.state::<std::sync::Arc<Presence>>();
    if presence.is_viewing(notice.run_id.as_deref(), notice.task_id.as_deref()) {
        log::debug!("{:?} toast skipped: the user is looking at it", notice.need);
        return;
    }
    let target = Target { run_id: notice.run_id.clone(), task_id: notice.task_id.clone(), project_id: notice.project_id.clone() };
    crate::toast::need(app, &notice.title, &notice.body, &notice.project, &link(&target), notice.pr_url.as_deref());
}

/// What a `pill://open` link points at.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub project_id: Option<String>,
}

/// Builds the link a toast launches, e.g. `pill://open?run=r_1&task=t_2`.
pub fn link(target: &Target) -> String {
    let parts: Vec<String> = [("run", &target.run_id), ("task", &target.task_id), ("project", &target.project_id)]
        .into_iter()
        .filter_map(|(k, v)| v.as_ref().filter(|v| is_id(v)).map(|v| format!("{k}={v}")))
        .collect();
    format!("{}://open?{}", crate::toast::SCHEME, parts.join("&"))
}

/// Parses a link built by [`link`]. Returns `None` for anything else.
///
/// SECURITY: any program or web page can launch a `pill://` URL, so only
/// the known keys with id-shaped values are accepted, and the result can
/// only ever select something in Pill's own UI.
pub fn parse_link(raw: &str) -> Option<Target> {
    let rest = raw.trim().strip_prefix(crate::toast::SCHEME)?.strip_prefix("://open")?;
    let query = rest.trim_start_matches('/').strip_prefix('?').unwrap_or("");
    let mut target = Target::default();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=')?;
        if !is_id(value) {
            return None;
        }
        match key {
            "run" => target.run_id = Some(value.to_string()),
            "task" => target.task_id = Some(value.to_string()),
            "project" => target.project_id = Some(value.to_string()),
            _ => {}
        }
    }
    Some(target)
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 96 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Brings Pill up on a link's run (Agents tab) or task (task page) in the
/// pop-out window. The pop-out is used because it is a normal window that
/// can take focus over whatever the user is doing; the notch never does.
pub fn open_target(app: &AppHandle, target: Target) {
    let nav = crate::detached::Nav {
        tab: Some(if target.run_id.is_some() { "agents" } else { "board" }.into()),
        project_id: target.project_id.clone(),
        run_id: target.run_id,
        task_id: target.task_id,
    };
    if let Err(e) = crate::detached::open(app, nav) {
        log::warn!("couldn't open Pill from a notification: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(run: Option<&str>, task: Option<&str>) -> Viewing {
        Viewing { run_id: run.map(Into::into), task_id: task.map(Into::into) }
    }

    #[test]
    fn suppressed_only_while_looking_at_the_same_run_or_task() {
        let agents = view(Some("r_1"), Some("t_1"));
        let page = view(None, Some("t_2"));
        let fresh = Duration::from_secs(5);
        let views = [(&agents, fresh), (&page, fresh)];
        assert!(watching(views, Some("r_1"), Some("t_1")));
        assert!(watching(views, Some("r_other"), Some("t_1")), "a newer run of the task on screen counts");
        assert!(watching(views, None, Some("t_2")));
        assert!(!watching(views, Some("r_9"), Some("t_9")));
        assert!(!watching(views, None, None));
    }

    #[test]
    fn stale_views_stop_suppressing() {
        let v = view(Some("r_1"), Some("t_1"));
        assert!(!watching([(&v, VIEW_TTL + Duration::from_secs(1))], Some("r_1"), None));
        assert!(!watching([], Some("r_1"), Some("t_1")));
    }

    #[test]
    fn empty_view_never_matches_missing_ids() {
        let v = Viewing::default();
        assert!(!watching([(&v, Duration::ZERO)], None, None));
        assert!(!watching([(&v, Duration::ZERO)], Some("r_1"), Some("t_1")));
    }

    #[test]
    fn presence_set_and_clear() {
        let p = Presence::default();
        p.set("pill", Some(view(Some("r_1"), Some("t_1"))));
        assert!(p.is_viewing(Some("r_1"), None));
        p.set("detached", Some(view(None, Some("t_2"))));
        p.set("pill", None);
        assert!(!p.is_viewing(Some("r_1"), Some("t_1")));
        assert!(p.is_viewing(None, Some("t_2")));
    }

    #[test]
    fn links_round_trip() {
        let t = Target { run_id: Some("r_20261119_210000_8f2k1".into()), task_id: Some("t_8f2k1".into()), project_id: Some("p_0123456789".into()) };
        let url = link(&t);
        assert_eq!(url, "pill://open?run=r_20261119_210000_8f2k1&task=t_8f2k1&project=p_0123456789");
        assert_eq!(parse_link(&url), Some(t));
        // Windows may add a trailing slash before the query.
        assert_eq!(parse_link("pill://open/?task=t_1").unwrap().task_id.as_deref(), Some("t_1"));
    }

    #[test]
    fn hostile_links_are_rejected() {
        assert_eq!(parse_link("https://evil.example/open?run=r_1"), None);
        assert_eq!(parse_link("pill://delete?task=t_1"), None);
        assert_eq!(parse_link("pill://open?task=..%2F..%2Fx"), None);
        assert_eq!(parse_link("pill://open?task=a b"), None);
        assert_eq!(parse_link("pill://open?task"), None);
        assert_eq!(parse_link("--some-flag"), None);
        let ids_only = link(&Target { task_id: Some("t_1\" & calc".into()), ..Default::default() });
        assert_eq!(ids_only, "pill://open?");
    }
}
