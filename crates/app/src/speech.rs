//! Read aloud through the Windows speech engine (SAPI). Everything runs on the UI thread: speaking is
//! asynchronous, the caller polls `is_speaking` and hands over the next paragraph when one has ended.

#[cfg(windows)]
mod imp {
    use windows::Win32::Media::Speech::{
        IEnumSpObjectTokens, ISpObjectToken, ISpObjectTokenCategory, ISpVoice, SPCAT_VOICES, SPF_ASYNC, SPF_IS_NOT_XML,
        SPF_PURGEBEFORESPEAK, SPRS_IS_SPEAKING, SPVOICESTATUS, SpObjectTokenCategory, SpVoice,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    use windows::core::{PCWSTR, PWSTR, w};

    pub struct Speaker {
        voice: Option<ISpVoice>,
        /// The language the voice was last chosen for (true = Russian).
        russian: Option<bool>,
        pub paused: bool,
    }

    impl Speaker {
        pub fn new() -> Speaker {
            Speaker {
                voice: None,
                russian: None,
                paused: false,
            }
        }

        fn voice(&mut self) -> Result<&ISpVoice, String> {
            if self.voice.is_none() {
                // SAFETY: plain COM activation on the UI thread, which winit has initialised for COM.
                let v: ISpVoice = unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) }.map_err(|e| e.to_string())?;
                self.voice = Some(v);
            }
            self.voice.as_ref().ok_or_else(|| "no voice".to_owned())
        }

        /// Picks an installed voice for the language when there is one; `false` when only the default exists.
        fn choose_voice(&mut self, russian: bool) -> bool {
            if self.russian == Some(russian) {
                return true;
            }
            let Ok(voice) = self.voice().cloned() else { return false };
            self.russian = Some(russian);
            // SAFETY: COM calls with valid pointers to locals.
            unsafe {
                let Ok(cat) = CoCreateInstance::<_, ISpObjectTokenCategory>(&SpObjectTokenCategory, None, CLSCTX_ALL)
                else {
                    return false;
                };
                if cat.SetId(SPCAT_VOICES, false).is_err() {
                    return false;
                }
                let wanted = if russian {
                    w!("Language=419")
                } else {
                    w!("Language=409")
                };
                let Ok(tokens) = cat.EnumTokens(wanted, PCWSTR::null()) else {
                    return false;
                };
                let tokens: IEnumSpObjectTokens = tokens;
                let mut token: Option<ISpObjectToken> = None;
                let mut fetched = 0u32;
                if tokens.Next(1, &mut token, Some(&mut fetched)).is_ok() && fetched == 1 {
                    if let Some(t) = token {
                        return voice.SetVoice(&t).is_ok();
                    }
                }
            }
            false
        }

        /// Starts speaking `text`, replacing whatever is being said. Returns whether a voice for the language exists.
        pub fn speak(&mut self, text: &str, russian: bool, rate: i32) -> Result<bool, String> {
            let found = self.choose_voice(russian);
            let voice = self.voice()?.clone();
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: `wide` is NUL-terminated and lives for the call; SAPI copies the text.
            unsafe {
                let _ = voice.SetRate(rate.clamp(-10, 10));
                let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0 | SPF_IS_NOT_XML.0) as u32;
                voice
                    .Speak(PCWSTR(wide.as_ptr()), flags, None)
                    .map_err(|e| e.to_string())?;
                if self.paused {
                    let _ = voice.Resume();
                    self.paused = false;
                }
            }
            Ok(found)
        }

        pub fn is_speaking(&self) -> bool {
            let Some(voice) = &self.voice else { return false };
            let mut status = SPVOICESTATUS::default();
            let mut bookmark = PWSTR::null();
            // SAFETY: both out-pointers refer to locals.
            let ok = unsafe { voice.GetStatus(&mut status, &mut bookmark) }.is_ok();
            ok && status.dwRunningState & (SPRS_IS_SPEAKING.0 as u32) != 0
        }

        pub fn pause(&mut self) {
            if let Some(v) = &self.voice {
                // SAFETY: no pointers.
                let _ = unsafe { v.Pause() };
                self.paused = true;
            }
        }

        pub fn resume(&mut self) {
            if let Some(v) = &self.voice {
                // SAFETY: no pointers.
                let _ = unsafe { v.Resume() };
                self.paused = false;
            }
        }

        pub fn stop(&mut self) {
            if let Some(v) = &self.voice {
                let empty = [0u16];
                // SAFETY: a NUL-terminated empty string; purging stops the current speech.
                unsafe {
                    let _ = v.Resume();
                    let _ = v.Speak(
                        PCWSTR(empty.as_ptr()),
                        (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32,
                        None,
                    );
                }
            }
            self.paused = false;
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub struct Speaker {
        pub paused: bool,
    }
    impl Speaker {
        pub fn new() -> Speaker {
            Speaker { paused: false }
        }
        pub fn speak(&mut self, _: &str, _: bool, _: i32) -> Result<bool, String> {
            Err("speech needs Windows".into())
        }
        pub fn is_speaking(&self) -> bool {
            false
        }
        pub fn pause(&mut self) {}
        pub fn resume(&mut self) {}
        pub fn stop(&mut self) {}
    }
}

pub use imp::Speaker;

/// True when the text is mostly Cyrillic, so a Russian voice should read it.
pub fn is_russian(text: &str) -> bool {
    let (mut cyr, mut lat) = (0usize, 0usize);
    for c in text.chars().take(2000) {
        if ('\u{400}'..='\u{4ff}').contains(&c) {
            cyr += 1;
        } else if c.is_ascii_alphabetic() {
            lat += 1;
        }
    }
    cyr > lat
}

/// The text with everything a voice stumbles on removed: page-number-like lines, soft hyphens, stray control characters.
pub fn clean_for_speech(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '\u{ad}' && (!c.is_control() || *c == '\n'))
        .collect::<String>()
        .replace("-\n", "")
        .split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_is_guessed_from_letters() {
        assert!(is_russian("Привет, мир! Hello"));
        assert!(!is_russian("Hello, мир and the rest of the English sentence"));
    }

    #[test]
    fn text_is_cleaned_for_the_voice() {
        assert_eq!(clean_for_speech("раз-\nделение  \n\nстроки\u{ad}"), "разделение строки");
    }
}
