//! One window per user: a second start hands its files to the first through a small inbox folder and exits.

use crate::settings::data_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Holds the named mutex for as long as the first instance lives.
pub struct Guard {
    #[cfg(windows)]
    _handle: windows::Win32::Foundation::HANDLE,
}

pub enum Role {
    First(Guard),
    Another,
}

#[cfg(windows)]
pub fn acquire() -> Role {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::w;
    // SAFETY: a named mutex with default security; the handle stays open until the process ends.
    unsafe {
        match CreateMutexW(None, true, w!("Local\\Folio.SingleInstance")) {
            Ok(h) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    Role::Another
                } else {
                    Role::First(Guard { _handle: h })
                }
            }
            Err(_) => Role::First(Guard {
                _handle: Default::default(),
            }),
        }
    }
}

#[cfg(not(windows))]
pub fn acquire() -> Role {
    Role::First(Guard {})
}

fn inbox() -> PathBuf {
    data_dir().join("inbox")
}

/// Leaves the paths for the running instance (an empty list just asks it to come to the front) and waits until it
/// has taken them. `false` when nobody answered, so the caller can start normally.
pub fn forward(paths: &[PathBuf], wait: Duration) -> bool {
    let dir = inbox();
    if fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let file = dir.join(format!("{}-{}.txt", std::process::id(), crate::timeutil::now()));
    let text: String = paths.iter().map(|p| format!("{}\n", p.display())).collect();
    let tmp = file.with_extension("tmp");
    if fs::write(&tmp, if text.is_empty() { "\n" } else { &text }).is_err() || fs::rename(&tmp, &file).is_err() {
        return false;
    }
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if !file.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let _ = fs::remove_file(&file);
    false
}

/// Takes everything left in the inbox. `None` when nothing was left, `Some(vec![])` for a plain "come to front".
pub fn drain() -> Option<Vec<PathBuf>> {
    let rd = fs::read_dir(inbox()).ok()?;
    let mut any = false;
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "txt") {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&p) {
            any = true;
            out.extend(text.lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from));
            let _ = fs::remove_file(&p);
        }
    }
    any.then_some(out)
}

/// Wakes the window when a second start leaves something in the inbox: a minimised window does not repaint on its
/// own, and the second start only waits a couple of seconds for an answer.
pub fn watch(wake: impl Fn() + Send + 'static) {
    let dir = inbox();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(150));
            let pending = fs::read_dir(&dir)
                .map(|rd| rd.flatten().any(|e| e.path().extension().is_some_and(|x| x == "txt")))
                .unwrap_or(false);
            if pending {
                wake();
            }
        }
    });
}

pub fn existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.into_iter().filter(|p: &PathBuf| Path::new(p).is_file()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarding_times_out_when_nobody_listens() {
        let _dir = crate::settings::use_private_test_data_dir();
        assert!(!forward(&[PathBuf::from("C:\\x.pdf")], Duration::from_millis(120)));
        assert!(drain().is_none(), "the unanswered request is withdrawn");
    }

    #[test]
    fn drain_returns_paths_and_empties_the_inbox() {
        let dir = crate::settings::use_private_test_data_dir();
        let inbox = dir.path().join("inbox");
        fs::create_dir_all(&inbox).unwrap();
        fs::write(inbox.join("1-1.txt"), "C:\\a.pdf\nC:\\b.docx\n").unwrap();
        fs::write(inbox.join("2-2.txt"), "\n").unwrap();
        let got = drain().unwrap();
        assert_eq!(got, vec![PathBuf::from("C:\\a.pdf"), PathBuf::from("C:\\b.docx")]);
        assert!(drain().is_none());
    }
}
