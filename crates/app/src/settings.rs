use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// `%APPDATA%\Folio` (or `FOLIO_DATA_DIR`, which tests and portable setups can use).
pub fn data_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(d) = PRIVATE_DATA_DIR.with(|p| p.borrow().clone()) {
        return d;
    }
    if let Some(d) = std::env::var_os("FOLIO_DATA_DIR") {
        return PathBuf::from(d);
    }
    std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("Folio"))
        .unwrap_or_else(|| std::env::temp_dir().join("Folio"))
}

/// Where the unpacked pdfium library lives: `%LOCALAPPDATA%\Folio`.
pub fn local_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(d) = PRIVATE_DATA_DIR.with(|p| p.borrow().clone()) {
        return d.join("local");
    }
    if let Some(d) = std::env::var_os("FOLIO_DATA_DIR") {
        return PathBuf::from(d).join("local");
    }
    std::env::var_os("LOCALAPPDATA")
        .map(|a| PathBuf::from(a).join("Folio"))
        .unwrap_or_else(|| std::env::temp_dir().join("Folio-local"))
}

static READ_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// After this, nothing is written to the settings or the library: used by the screenshot mode.
pub fn set_read_only() {
    READ_ONLY.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_read_only() -> bool {
    READ_ONLY.load(std::sync::atomic::Ordering::Relaxed)
}

/// Writes next to the target and renames, so a crash in the middle never leaves a half-written file.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeId {
    Mocha,
    Light,
    Sepia,
    Night,
}

impl ThemeId {
    pub const ALL: [ThemeId; 4] = [ThemeId::Mocha, ThemeId::Light, ThemeId::Sepia, ThemeId::Night];

    pub fn next(self) -> ThemeId {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadingFont {
    Serif,
    Sans,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// "ru" or "en".
    pub language: String,
    pub theme: ThemeId,
    pub font: ReadingFont,
    pub font_size: f32,
    pub line_spacing: f32,
    /// Width of the text column in points.
    pub column_width: f32,
    pub justify: bool,
    /// Tone PDF pages to match the theme (dark pages in dark themes).
    pub tone_pdf: bool,
    pub sidebar_open: bool,
    pub sidebar_tab: String,
    pub check_updates: bool,
    pub last_update_check: i64,
    /// Speech rate for read aloud, -10..10.
    pub speech_rate: i32,
    /// A second launch hands its files to the running window.
    pub single_window: bool,
    pub restore_tabs: bool,
    pub open_tabs: Vec<String>,
    pub window_size: Option<[f32; 2]>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "ru".into(),
            theme: ThemeId::Mocha,
            font: ReadingFont::Serif,
            font_size: 18.0,
            line_spacing: 1.45,
            column_width: 780.0,
            justify: false,
            tone_pdf: true,
            sidebar_open: true,
            sidebar_tab: "contents".into(),
            check_updates: true,
            last_update_check: 0,
            speech_rate: 0,
            single_window: true,
            restore_tabs: true,
            open_tabs: Vec::new(),
            window_size: None,
        }
    }
}

impl Settings {
    fn path() -> PathBuf {
        data_dir().join("settings.json")
    }

    pub fn load() -> Self {
        let mut s: Settings = std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        s.sanitize();
        s
    }

    /// Hand-edited or damaged values must not break the window.
    fn sanitize(&mut self) {
        let fix = |v: &mut f32, lo: f32, hi: f32, default: f32| {
            *v = if v.is_finite() { v.clamp(lo, hi) } else { default };
        };
        fix(&mut self.font_size, 11.0, 40.0, 18.0);
        fix(&mut self.line_spacing, 1.0, 2.5, 1.45);
        fix(&mut self.column_width, 300.0, 1600.0, 780.0);
        self.speech_rate = self.speech_rate.clamp(-10, 10);
        if let Some(sz) = self.window_size
            && !(sz[0].is_finite()
                && sz[1].is_finite()
                && (400.0..=8000.0).contains(&sz[0])
                && (300.0..=6000.0).contains(&sz[1]))
        {
            self.window_size = None;
        }
    }

    pub fn save(&self) {
        if is_read_only() {
            return;
        }
        let _ = std::fs::create_dir_all(data_dir());
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            write_atomic(&Self::path(), &json);
        }
    }
}

#[cfg(test)]
thread_local! {
    static PRIVATE_DATA_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Gives the calling test thread a data folder of its own, for tests that change and save settings.
#[cfg(test)]
pub fn use_private_test_data_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    PRIVATE_DATA_DIR.with(|p| *p.borrow_mut() = Some(dir.path().to_path_buf()));
    dir
}

/// Points `FOLIO_DATA_DIR` at one temp folder shared by all tests of this binary.
#[cfg(test)]
pub fn use_test_data_dir() -> PathBuf {
    use std::sync::OnceLock;
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    let dir = DIR.get_or_init(|| tempfile::tempdir().expect("temp dir"));
    // SAFETY: set before any test reads it and always to the same value.
    unsafe { std::env::set_var("FOLIO_DATA_DIR", dir.path()) };
    dir.path().to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absurd_values_are_repaired() {
        let mut s: Settings =
            serde_json::from_str(r#"{"font_size":0.0,"column_width":1e30,"window_size":[0.0,1e9]}"#).unwrap();
        s.sanitize();
        assert_eq!((s.font_size, s.column_width, s.window_size), (11.0, 1600.0, None));
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"language":"en","font_size":20.0}"#).unwrap();
        assert_eq!(s.language, "en");
        assert_eq!(s.font_size, 20.0);
        assert!(s.check_updates && s.restore_tabs);
        assert_eq!(s.theme, ThemeId::Mocha);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let s = Settings {
            theme: ThemeId::Sepia,
            font: ReadingFont::Sans,
            ..Settings::default()
        };
        let back: Settings = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn themes_cycle() {
        let mut t = ThemeId::Mocha;
        for _ in 0..ThemeId::ALL.len() {
            t = t.next();
        }
        assert_eq!(t, ThemeId::Mocha);
    }
}
