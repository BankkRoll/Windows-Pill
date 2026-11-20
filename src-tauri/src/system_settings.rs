//! Windows Settings pages Pill links to when the OS blocks something.
//!
//! SECURITY: the UI names a page from this fixed list; it can't make Pill
//! open an arbitrary `ms-settings:` (or any other) URI.

use serde::{Deserialize, Serialize};

/// A Windows Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SystemPage {
    /// Privacy & security > Notifications (notification access for apps).
    Notifications,
    /// Privacy & security > Microphone.
    Microphone,
    /// Time & language > Speech (speech languages and packs).
    Speech,
    /// System > Sound (input devices).
    Sound,
}

impl SystemPage {
    /// The `ms-settings:` URI for this page.
    pub fn uri(self) -> &'static str {
        match self {
            Self::Notifications => "ms-settings:privacy-notifications",
            Self::Microphone => "ms-settings:privacy-microphone",
            Self::Speech => "ms-settings:speech",
            Self::Sound => "ms-settings:sound",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_parse_from_camel_case() {
        let p: SystemPage = serde_json::from_str("\"notifications\"").unwrap();
        assert_eq!(p.uri(), "ms-settings:privacy-notifications");
        assert!(serde_json::from_str::<SystemPage>("\"ms-settings:anything\"").is_err());
    }
}
