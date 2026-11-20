//! Voice quick-add: offline dictation for the command bar.
//!
//! Responsibilities:
//! - Check the microphone privacy switches and that a microphone exists,
//!   so failures come with a precise reason and a Settings page to fix it.
//! - Listen for one utterance with the Windows desktop speech recognizer,
//!   streaming partial text to the UI as `pill://voice`.
//! - Stop on request: keep what was heard so far, or cancel.
//!
//! COMPAT: the WinRT `Windows.Media.SpeechRecognition.SpeechRecognizer`
//! with the dictation topic constraint is *not* offline. On Windows 11 it
//! compiles the constraint fine but `RecognizeAsync` fails with `0x80045509`
//! ("The speech privacy policy was not accepted") unless "Online speech
//! recognition" is turned on, which sends audio to Microsoft. The SAPI 5
//! in-process recognizer ("Microsoft Speech Recognizer 8.0 for Windows",
//! installed with the English, French, German, Spanish, Japanese and Chinese
//! speech packs) runs dictation entirely on the device, so Pill uses that.

use crate::system_settings::SystemPage;
use serde::Serialize;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

/// Event carrying a [`VoiceUpdate`] while listening.
pub const VOICE_EVENT: &str = "pill://voice";

/// Why listening ended without text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VoiceErrorCode {
    /// Another dictation is already running.
    Busy,
    /// A Windows privacy switch blocks the microphone.
    MicDenied,
    /// No recording device.
    NoMic,
    /// No offline recognizer for dictation is installed.
    NoRecognizer,
    /// Silence, or nothing the recognizer could make out.
    NothingHeard,
    /// The user cancelled.
    Cancelled,
    /// Anything else; the message has the details.
    Failed,
}

/// A dictation failure with copy for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceError {
    pub code: VoiceErrorCode,
    pub message: String,
    /// Windows Settings page that fixes it, if any.
    pub fix: Option<SystemPage>,
}

impl VoiceError {
    fn new(code: VoiceErrorCode, message: impl Into<String>, fix: Option<SystemPage>) -> Self {
        Self { code, message: message.into(), fix }
    }
}

/// Progress sent while listening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum VoiceUpdate {
    /// The microphone is open.
    Listening,
    /// Best guess so far.
    Hearing { text: String },
}

/// Cleans a transcript for the task input: single spaces, no trailing
/// period (task titles don't end in one), first letter capitalized.
///
/// @param text Raw recognizer output.
/// @example tidy("  buy milk tomorrow. ") // "Buy milk tomorrow"
pub fn tidy(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = joined.strip_suffix('.').unwrap_or(&joined);
    let mut chars = trimmed.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Reads the three Windows microphone switches (`Value` under
/// `CapabilityAccessManager\ConsentStore\microphone`).
///
/// @param device Machine-wide switch (HKLM).
/// @param apps The user's "Microphone access" switch (HKCU).
/// @param desktop "Let desktop apps access your microphone" (HKCU `NonPackaged`).
/// @returns An error naming the switch that's off.
pub fn mic_consent(device: Option<&str>, apps: Option<&str>, desktop: Option<&str>) -> Result<(), VoiceError> {
    let off = |v: Option<&str>| v.is_some_and(|v| v.eq_ignore_ascii_case("deny"));
    let page = Some(SystemPage::Microphone);
    if off(device) {
        Err(VoiceError::new(VoiceErrorCode::MicDenied, "Microphone access is off for this device. Turn it on in privacy settings.", page))
    } else if off(apps) {
        Err(VoiceError::new(VoiceErrorCode::MicDenied, "Microphone access is off. Turn it on in privacy settings.", page))
    } else if off(desktop) {
        Err(VoiceError::new(VoiceErrorCode::MicDenied, "Desktop apps can't use the microphone. Turn on \"Let desktop apps access your microphone\".", page))
    } else {
        Ok(())
    }
}

enum Stop {
    Commit,
    Cancel,
}

/// The one dictation session that may run at a time.
#[derive(Default)]
pub struct Voice {
    session: Mutex<Option<Sender<Stop>>>,
}

impl Voice {
    /// Ends the current dictation. `commit` keeps the text heard so far.
    ///
    /// @param commit Keep the partial text instead of cancelling.
    pub fn stop(&self, commit: bool) {
        if let Some(tx) = self.session.lock().unwrap().as_ref() {
            let _ = tx.send(if commit { Stop::Commit } else { Stop::Cancel });
        }
    }

    /// Listens for one utterance. Blocks until it ends; call off the main
    /// thread.
    ///
    /// @param emit Receives progress updates.
    /// @returns The tidied transcript.
    pub fn listen(&self, emit: impl Fn(VoiceUpdate)) -> Result<String, VoiceError> {
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut s = self.session.lock().unwrap();
            if s.is_some() {
                return Err(VoiceError::new(VoiceErrorCode::Busy, "Pill is already listening in another window.", None));
            }
            *s = Some(tx);
        }
        struct Clear<'a>(&'a Mutex<Option<Sender<Stop>>>);
        impl Drop for Clear<'_> {
            fn drop(&mut self) {
                *self.0.lock().unwrap() = None;
            }
        }
        let _clear = Clear(&self.session);
        imp::listen(&rx, &emit).map(|t| tidy(&t))
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn listen(_rx: &std::sync::mpsc::Receiver<Stop>, _emit: &dyn Fn(VoiceUpdate)) -> Result<String, VoiceError> {
        Err(VoiceError::new(VoiceErrorCode::NoRecognizer, "Voice input needs Windows", None))
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::mpsc::{Receiver, TryRecvError};
    use std::time::{Duration, Instant};
    use windows::core::{Interface, IUnknown, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{E_ACCESSDENIED, ERROR_SUCCESS};
    use windows::Win32::Media::Speech::*;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    /// Give up when no sound starts within this long.
    const FIRST_SOUND: Duration = Duration::from_secs(8);
    /// Hard cap on one dictation.
    const MAX_LENGTH: Duration = Duration::from_secs(30);
    /// After a phrase, wait this long for the next before finishing.
    const PAUSE: Duration = Duration::from_millis(1200);

    const CONSENT: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

    fn reg_string(root: HKEY, path: &str) -> Option<String> {
        let path: Vec<u16> = path.encode_utf16().chain([0]).collect();
        let name: Vec<u16> = "Value".encode_utf16().chain([0]).collect();
        let mut buf = [0u16; 64];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: buffers are valid for the sizes given and NUL-terminated.
        let r = unsafe {
            RegGetValueW(root, PCWSTR(path.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
        };
        if r != ERROR_SUCCESS {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..len.min(buf.len())]))
    }

    fn preflight() -> Result<(), VoiceError> {
        mic_consent(
            reg_string(HKEY_LOCAL_MACHINE, CONSENT).as_deref(),
            reg_string(HKEY_CURRENT_USER, CONSENT).as_deref(),
            reg_string(HKEY_CURRENT_USER, &format!(r"{CONSENT}\NonPackaged")).as_deref(),
        )?;
        // SAFETY: plain query with no arguments.
        if unsafe { windows::Win32::Media::Audio::waveInGetNumDevs() } == 0 {
            return Err(VoiceError::new(VoiceErrorCode::NoMic, "No microphone found. Plug one in or turn it on in sound settings.", Some(SystemPage::Sound)));
        }
        Ok(())
    }

    fn no_recognizer(e: windows::core::Error) -> VoiceError {
        VoiceError::new(
            VoiceErrorCode::NoRecognizer,
            format!("No offline speech recognizer is installed ({:#010x}). Add a speech pack for English, French, German, Spanish, Japanese or Chinese in speech settings.", e.code().0),
            Some(SystemPage::Speech),
        )
    }

    fn mic_failed(e: windows::core::Error) -> VoiceError {
        if e.code() == E_ACCESSDENIED {
            return VoiceError::new(VoiceErrorCode::MicDenied, "Windows blocked the microphone. Allow microphone access for desktop apps.", Some(SystemPage::Microphone));
        }
        VoiceError::new(VoiceErrorCode::Failed, format!("Couldn't open the microphone ({:#010x}).", e.code().0), Some(SystemPage::Sound))
    }

    fn nothing_heard() -> VoiceError {
        VoiceError::new(VoiceErrorCode::NothingHeard, "Didn't catch that. Try again a little closer to the mic.", None)
    }

    /// SPFEI(): event bits always carry the two reserved "flag check" bits.
    fn interest(events: &[SPEVENTENUM]) -> u64 {
        events.iter().fold((1u64 << SPEI_RESERVED1.0) | (1u64 << SPEI_RESERVED2.0), |acc, e| acc | (1u64 << e.0))
    }

    /// Text of a recognition or hypothesis result.
    unsafe fn result_text(unk: &IUnknown) -> Option<String> {
        let result: ISpRecoResult = unk.cast().ok()?;
        let mut text = PWSTR::null();
        result.GetText(u32::MAX, u32::MAX, true, &mut text, None).ok()?;
        let s = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as *const _));
        s
    }

    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            // SAFETY: only constructed after a successful CoInitializeEx.
            unsafe { CoUninitialize() };
        }
    }

    pub fn listen(rx: &Receiver<Stop>, emit: &dyn Fn(VoiceUpdate)) -> Result<String, VoiceError> {
        preflight()?;
        // SAFETY: balanced by `Com`'s drop on this thread.
        let _com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().map(|_| Com);
        // SAFETY: every SAPI call below is a COM call on interfaces owned
        // here; event lParams are released exactly once (see the loop).
        unsafe {
            let reco: ISpRecognizer = CoCreateInstance(&SpInprocRecognizer, None, CLSCTX_ALL).map_err(no_recognizer)?;
            let audio: IUnknown = CoCreateInstance(&SpMMAudioIn, None, CLSCTX_ALL).map_err(mic_failed)?;
            reco.SetInput(&audio, true).map_err(mic_failed)?;
            let ctx = reco.CreateRecoContext().map_err(no_recognizer)?;
            ctx.SetNotifyWin32Event().map_err(no_recognizer)?;
            let wanted = interest(&[SPEI_SOUND_START, SPEI_HYPOTHESIS, SPEI_RECOGNITION, SPEI_FALSE_RECOGNITION]);
            ctx.SetInterest(wanted, wanted).map_err(no_recognizer)?;
            let grammar = ctx.CreateGrammar(0).map_err(no_recognizer)?;
            grammar.LoadDictation(PCWSTR::null(), SPLO_STATIC).map_err(no_recognizer)?;
            grammar.SetDictationState(SPRS_ACTIVE).map_err(no_recognizer)?;
            reco.SetRecoState(SPRST_ACTIVE).map_err(mic_failed)?;
            emit(VoiceUpdate::Listening);

            let started = Instant::now();
            let mut heard_sound = false;
            let mut phrases: Vec<String> = Vec::new();
            let mut partial = String::new();
            let mut pause_from: Option<Instant> = None;
            let joined = |phrases: &[String], partial: &str| {
                phrases.iter().map(String::as_str).chain((!partial.is_empty()).then_some(partial)).collect::<Vec<_>>().join(" ")
            };
            let outcome = loop {
                match rx.try_recv() {
                    Ok(Stop::Cancel) => break Err(VoiceError::new(VoiceErrorCode::Cancelled, "Cancelled", None)),
                    Ok(Stop::Commit) | Err(TryRecvError::Disconnected) => {
                        let text = joined(&phrases, &partial);
                        break if text.trim().is_empty() { Err(nothing_heard()) } else { Ok(text) };
                    }
                    Err(TryRecvError::Empty) => {}
                }
                if !heard_sound && started.elapsed() > FIRST_SOUND {
                    break Err(nothing_heard());
                }
                if started.elapsed() > MAX_LENGTH || pause_from.is_some_and(|t| t.elapsed() > PAUSE) {
                    let text = joined(&phrases, &partial);
                    break if text.trim().is_empty() { Err(nothing_heard()) } else { Ok(text) };
                }
                let _ = ctx.WaitForNotifyEvent(100);
                loop {
                    let mut ev = SPEVENT::default();
                    let mut fetched = 0u32;
                    if ctx.GetEvents(1, &mut ev, &mut fetched).is_err() || fetched == 0 {
                        break;
                    }
                    let id = SPEVENTENUM(ev._bitfield & 0xffff);
                    let lparam_type = SPEVENTLPARAMTYPE((ev._bitfield >> 16) & 0xffff);
                    // The event owns one reference (or allocation) in lParam.
                    let object = if lparam_type == SPET_LPARAM_IS_OBJECT && ev.lParam.0 != 0 {
                        Some(IUnknown::from_raw(ev.lParam.0 as *mut _))
                    } else {
                        if (lparam_type == SPET_LPARAM_IS_POINTER || lparam_type == SPET_LPARAM_IS_STRING) && ev.lParam.0 != 0 {
                            CoTaskMemFree(Some(ev.lParam.0 as *const _));
                        }
                        None
                    };
                    match id {
                        SPEI_SOUND_START => {
                            heard_sound = true;
                            pause_from = None;
                        }
                        SPEI_HYPOTHESIS => {
                            heard_sound = true;
                            pause_from = None;
                            if let Some(t) = object.as_ref().and_then(|o| result_text(o)) {
                                partial = t;
                                emit(VoiceUpdate::Hearing { text: tidy(&joined(&phrases, &partial)) });
                            }
                        }
                        SPEI_RECOGNITION => {
                            if let Some(t) = object.as_ref().and_then(|o| result_text(o)).filter(|t| !t.trim().is_empty()) {
                                phrases.push(t);
                            }
                            partial.clear();
                            pause_from = Some(Instant::now());
                            emit(VoiceUpdate::Hearing { text: tidy(&joined(&phrases, "")) });
                        }
                        SPEI_FALSE_RECOGNITION => {
                            // Low confidence: keep the best guess; the user
                            // edits it in the input before adding anything.
                            if !partial.trim().is_empty() {
                                phrases.push(std::mem::take(&mut partial));
                            }
                            pause_from = Some(Instant::now());
                        }
                        _ => {}
                    }
                }
            };
            let _ = reco.SetRecoState(SPRST_INACTIVE);
            outcome
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn interest_includes_flag_check_bits() {
            let bits = interest(&[SPEI_RECOGNITION]);
            assert_ne!(bits & (1 << SPEI_RECOGNITION.0), 0);
            assert_ne!(bits & (1 << 30), 0);
            assert_ne!(bits & (1 << 33), 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidies_transcripts() {
        assert_eq!(tidy("  buy   milk tomorrow. "), "Buy milk tomorrow");
        assert_eq!(tidy("fix login bug #web"), "Fix login bug #web");
        assert_eq!(tidy(""), "");
    }

    #[test]
    fn mic_switches_report_the_one_that_is_off() {
        assert!(mic_consent(Some("Allow"), Some("Allow"), Some("Allow")).is_ok());
        assert!(mic_consent(None, None, None).is_ok());
        let e = mic_consent(Some("Allow"), Some("Allow"), Some("Deny")).unwrap_err();
        assert_eq!(e.code, VoiceErrorCode::MicDenied);
        assert!(e.message.contains("desktop apps"));
        assert_eq!(e.fix, Some(SystemPage::Microphone));
        assert!(mic_consent(Some("Deny"), Some("Allow"), Some("Allow")).unwrap_err().message.contains("device"));
    }

    #[test]
    fn only_one_session_at_a_time() {
        let voice = Voice::default();
        let (tx, _rx) = std::sync::mpsc::channel();
        *voice.session.lock().unwrap() = Some(tx);
        let err = voice.listen(|_| {}).unwrap_err();
        assert_eq!(err.code, VoiceErrorCode::Busy);
    }
}
