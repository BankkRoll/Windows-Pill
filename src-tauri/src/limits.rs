//! Plan usage limits of Claude Code and Codex.
//!
//! Both CLIs run on the user's subscription, which has rolling usage limits.
//! When one is hit, every further run would fail the same way, so Pill:
//! - recognizes the limit in each CLI's own words (stream events, result
//!   text, stderr), including when it resets if the CLI says;
//! - reports it as one sentence ("Codex usage limit reached, resets at …");
//! - lets [`crate::agent::Agents`] hold queued runs and autopilot for that
//!   provider until the reset instead of burning through the queue.
//!
//! Parsing is deliberately forgiving: the wording changes between CLI
//! versions, so a limit without a readable reset time is still a limit.

use crate::provider::Provider;
use chrono::{DateTime, Duration, Local, NaiveTime, TimeZone};
use serde::Serialize;

/// How long to wait when a CLI hits a limit without saying when it resets.
pub const UNKNOWN_RESET_WAIT: Duration = Duration::minutes(30);

/// A usage limit one provider reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimit {
    pub provider: Provider,
    /// Unix seconds when the limit lifts, when the CLI said and it parsed.
    pub resets_at: Option<i64>,
    /// The CLI's own reset wording when it couldn't be turned into a time,
    /// e.g. `resets Oct 9, 10am`.
    pub hint: Option<String>,
}

impl UsageLimit {
    /// One sentence for run errors, chat replies and toasts.
    ///
    /// # Example
    /// ```ignore
    /// // "Claude Code usage limit reached, resets at Thu 15:00"
    /// ```
    pub fn describe(&self) -> String {
        let label = self.provider.label();
        match (self.resets_at.and_then(|t| Local.timestamp_opt(t, 0).single()), &self.hint) {
            (Some(at), _) => format!("{label} usage limit reached, resets at {}", at.format("%a %H:%M")),
            (None, Some(hint)) => format!("{label} usage limit reached, {hint}"),
            (None, None) => format!("{label} usage limit reached; try again later"),
        }
    }

    /// When queued work for this provider may start again.
    pub fn wait_until(&self, now: DateTime<Local>) -> i64 {
        let earliest = (now + Duration::minutes(1)).timestamp();
        self.resets_at.unwrap_or_else(|| (now + UNKNOWN_RESET_WAIT).timestamp()).max(earliest)
    }
}

/// Phrases the CLIs use for plan limits, lowercased.
const LIMIT_PHRASES: &[&str] = &[
    "usage limit",
    "hit your limit",
    "limit reached",
    "rate limit",
    "rate_limit",
    "credits_depleted",
    "credit balance is too low",
    "quota exceeded",
    "insufficient_quota",
    "too many requests",
];

/// Whether `text` (an error, never a successful answer) reports a usage limit.
pub fn is_limit(text: &str) -> bool {
    let lower = text.to_lowercase();
    LIMIT_PHRASES.iter().any(|p| lower.contains(p)) || lower.contains(" 429")
}

/// Recognizes a usage limit in a failure message and reads its reset time.
pub fn detect(provider: Provider, text: &str) -> Option<UsageLimit> {
    detect_at(provider, text, Local::now())
}

/// [`detect`] with an explicit clock, for tests.
pub fn detect_at(provider: Provider, text: &str, now: DateTime<Local>) -> Option<UsageLimit> {
    if !is_limit(text) {
        return None;
    }
    let (resets_at, hint) = reset_time(text, now);
    Some(UsageLimit { provider, resets_at, hint })
}

/// Reads a reset time from the formats the CLIs print:
/// - `…limit reached|1795492800` (Claude, Unix seconds after a bar);
/// - `try again in 2 hours 5 minutes` (Codex);
/// - `resets 3pm`, `resets at 3:04 PM`, `try again at 15:30` (either).
///
/// Returns the time, or the CLI's own phrase when the time can't be read.
fn reset_time(text: &str, now: DateTime<Local>) -> (Option<i64>, Option<String>) {
    if let Some((_, tail)) = text.rsplit_once('|') {
        if let Ok(secs) = tail.trim().parse::<i64>() {
            if secs > 1_000_000_000 {
                return (Some(secs), None);
            }
        }
    }
    let lower = text.to_lowercase();
    if let Some(i) = lower.find("try again in ") {
        if let Some(at) = parse_duration(&lower[i + "try again in ".len()..]).and_then(|d| now.checked_add_signed(d)) {
            return (Some(at.timestamp()), None);
        }
    }
    for marker in ["resets at ", "resets ", "reset at ", "try again at "] {
        if let Some(i) = lower.find(marker) {
            if let Some(t) = parse_clock(&lower[i + marker.len()..]) {
                return (Some(next_occurrence(now, t).timestamp()), None);
            }
            // Lowercasing can change byte offsets in non-ASCII text; fall back
            // to the lowercased phrase rather than slicing mid-character.
            let from = text.get(i..).filter(|_| lower.len() == text.len()).unwrap_or(&lower[i..]);
            let phrase = from.split(['.', '\n']).next().unwrap_or_default().trim().to_string();
            return (None, (!phrase.is_empty()).then_some(phrase));
        }
    }
    (None, None)
}

/// `2 hours 5 minutes`, `37 minutes`, `4 days 2 hours 39 minutes`, `1h 30m`.
/// `None` when no duration reads, or one too large to represent: the text
/// comes from a CLI and must never panic the agent worker.
fn parse_duration(s: &str) -> Option<Duration> {
    let mut total = Duration::zero();
    let mut found = false;
    let mut words = s.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).peekable();
    while let Some(word) = words.next() {
        let digits: String = word.chars().take_while(char::is_ascii_digit).collect();
        let Ok(n) = digits.parse::<i64>() else { break };
        let attached = &word[digits.len()..];
        let unit = if attached.is_empty() { words.next().unwrap_or_default() } else { attached };
        let unit = unit.trim_end_matches(['.', ',']);
        let d = match unit {
            u if u.starts_with('d') => Duration::try_days(n),
            u if u.starts_with('h') => Duration::try_hours(n),
            u if u.starts_with("mi") || u == "m" => Duration::try_minutes(n),
            u if u.starts_with('s') => Duration::try_seconds(n),
            _ => break,
        };
        total = total.checked_add(&d?)?;
        found = true;
        if words.peek().is_some_and(|w| *w == "and") {
            words.next();
        }
    }
    found.then_some(total)
}

/// `3pm`, `3:04 pm`, `15:30`, `10am (europe/berlin)` at the start of `s`.
fn parse_clock(s: &str) -> Option<NaiveTime> {
    let s = s.trim_start();
    let hour_digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    let mut hour: u32 = hour_digits.parse().ok()?;
    let mut rest = &s[hour_digits.len()..];
    let mut minute = 0;
    if let Some(after) = rest.strip_prefix(':') {
        let min_digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if min_digits.len() != 2 {
            return None;
        }
        minute = min_digits.parse().ok()?;
        rest = &after[2..];
    }
    let rest = rest.trim_start();
    if rest.starts_with("pm") || rest.starts_with("p.m") {
        if hour == 0 || hour > 12 {
            return None;
        }
        if hour != 12 {
            hour += 12;
        }
    } else if rest.starts_with("am") || rest.starts_with("a.m") {
        if hour == 0 || hour > 12 {
            return None;
        }
        if hour == 12 {
            hour = 0;
        }
    } else if !s[hour_digits.len()..].starts_with(':') {
        // A bare number ("resets 9 Oct") is a date, not a time.
        return None;
    }
    NaiveTime::from_hms_opt(hour, minute, 0)
}

/// The next time the local clock shows `t`: today if still ahead, else tomorrow.
fn next_occurrence(now: DateTime<Local>, t: NaiveTime) -> DateTime<Local> {
    let today = now.date_naive().and_time(t);
    let candidate = Local.from_local_datetime(&today).earliest().unwrap_or(now);
    if candidate > now {
        candidate
    } else {
        Local.from_local_datetime(&(today + Duration::days(1))).earliest().unwrap_or(now + Duration::days(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 19, 14, 0, 0).unwrap()
    }

    fn at(text: &str) -> Option<UsageLimit> {
        detect_at(Provider::Codex, text, now())
    }

    #[test]
    fn ignores_ordinary_failures() {
        assert_eq!(at("Tests failed after Codex finished"), None);
        assert_eq!(at("fatal: not a git repository"), None);
    }

    #[test]
    fn reads_claude_epoch_resets() {
        let l = detect_at(Provider::Claude, "Claude AI usage limit reached|1795492800", now()).unwrap();
        assert_eq!(l.resets_at, Some(1_795_492_800));
        assert!(l.describe().starts_with("Claude Code usage limit reached, resets at "));
    }

    #[test]
    fn reads_codex_relative_resets() {
        let l = at("You've hit your usage limit. Upgrade to Pro (https://x) or try again in 2 hours 5 minutes.").unwrap();
        assert_eq!(l.resets_at, Some((now() + Duration::minutes(125)).timestamp()));
        let days = at("You've hit your usage limit. Try again in 4 days 2 hours 39 minutes.").unwrap();
        assert_eq!(days.resets_at, Some((now() + Duration::days(4) + Duration::hours(2) + Duration::minutes(39)).timestamp()));
        let short = at("rate limit exceeded, try again in 1h 30m").unwrap();
        assert_eq!(short.resets_at, Some((now() + Duration::minutes(90)).timestamp()));
    }

    #[test]
    fn reads_clock_resets_today_or_tomorrow() {
        let later = detect_at(Provider::Claude, "5-hour limit reached ∙ resets 3pm", now()).unwrap();
        assert_eq!(later.resets_at, Some(Local.with_ymd_and_hms(2026, 11, 19, 15, 0, 0).unwrap().timestamp()));
        let tomorrow = detect_at(Provider::Claude, "You've hit your limit · resets at 9:30 am (Europe/Berlin)", now()).unwrap();
        assert_eq!(tomorrow.resets_at, Some(Local.with_ymd_and_hms(2026, 11, 20, 9, 30, 0).unwrap().timestamp()));
        let h24 = at("usage limit reached, try again at 16:45").unwrap();
        assert_eq!(h24.resets_at, Some(Local.with_ymd_and_hms(2026, 11, 19, 16, 45, 0).unwrap().timestamp()));
    }

    #[test]
    fn keeps_the_clis_words_when_the_time_is_a_date() {
        let l = detect_at(Provider::Claude, "Weekly limit reached ∙ resets Oct 9, 10am", now()).unwrap();
        assert_eq!(l.resets_at, None);
        assert_eq!(l.describe(), "Claude Code usage limit reached, resets Oct 9, 10am");
        let bare = at("429 Too Many Requests").unwrap();
        assert_eq!(bare.describe(), "Codex usage limit reached; try again later");
    }

    #[test]
    fn waits_at_least_a_minute_and_half_an_hour_when_unknown() {
        let unknown = UsageLimit { provider: Provider::Claude, resets_at: None, hint: None };
        assert_eq!(unknown.wait_until(now()), (now() + UNKNOWN_RESET_WAIT).timestamp());
        let past = UsageLimit { provider: Provider::Claude, resets_at: Some(now().timestamp() - 10), hint: None };
        assert_eq!(past.wait_until(now()), (now() + Duration::minutes(1)).timestamp());
    }

    #[test]
    fn absurd_durations_never_panic() {
        for text in [
            "usage limit reached, try again in 99999999999999 days",
            "usage limit reached, try again in 9223372036854775807 seconds",
            "usage limit reached, try again in 100000000 days 100000000 days",
            "usage limit reached, try again in 106751991167300 days",
        ] {
            let l = at(text).unwrap();
            assert_eq!(l.resets_at, None, "{text}");
        }
        assert_eq!(parse_duration("2562047788015215 hours 2562047788015215 hours"), None);
    }

    #[test]
    fn clock_parsing_edges() {
        assert_eq!(parse_clock("12am"), NaiveTime::from_hms_opt(0, 0, 0));
        assert_eq!(parse_clock("12:15 pm"), NaiveTime::from_hms_opt(12, 15, 0));
        assert_eq!(parse_clock("9 oct"), None);
        assert_eq!(parse_clock("13pm"), None);
        assert_eq!(parse_clock("oct 9"), None);
    }
}
