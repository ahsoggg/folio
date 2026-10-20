//! pdfium travels inside the executable, so updating and installing stay a one-file affair: the library is unpacked
//! to `%LOCALAPPDATA%\Folio` under a name that carries its content hash and loaded from there.

use crate::settings::local_dir;
use std::fs;
use std::path::PathBuf;

static PDFIUM: &[u8] = include_bytes!("../../../assets/pdfium/pdfium.dll");

fn fingerprint(bytes: &[u8]) -> u64 {
    // FNV-1a over the whole library: a name that changes whenever the bytes do
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Unpacks the embedded library if it is not there yet and returns its path.
pub fn ensure_extracted() -> Result<PathBuf, String> {
    let dir = local_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let name = format!("pdfium-{:016x}.dll", fingerprint(PDFIUM));
    let path = dir.join(&name);
    if fs::read(&path).map(|b| b == PDFIUM).unwrap_or(false) {
        return Ok(path);
    }
    let tmp = dir.join(format!("{name}.tmp{}", std::process::id()));
    fs::write(&tmp, PDFIUM).map_err(|e| format!("cannot write pdfium: {e}"))?;
    if fs::rename(&tmp, &path).is_err() {
        // another instance got there first
        let _ = fs::remove_file(&tmp);
        if !fs::read(&path).map(|b| b == PDFIUM).unwrap_or(false) {
            return Err("cannot place pdfium".into());
        }
    }
    // older copies from previous versions
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("pdfium-") && n.ends_with(".dll") && n != name {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn the_embedded_library_is_the_pinned_one() {
        let pinned = include_str!("../../../assets/pdfium/pdfium.dll.sha256");
        let expected = pinned.split_whitespace().next().unwrap();
        let actual: String = Sha256::digest(PDFIUM).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            actual, expected,
            "assets/pdfium/pdfium.dll changed without updating pdfium.dll.sha256 and VERSION.txt"
        );
    }

    #[test]
    fn the_library_is_unpacked_once_and_reused() {
        let dir = crate::settings::use_private_test_data_dir();
        let first = ensure_extracted().unwrap();
        assert!(first.exists() && first.starts_with(dir.path()));
        let stamp = fs::metadata(&first).unwrap().modified().unwrap();
        let second = ensure_extracted().unwrap();
        assert_eq!(first, second);
        assert_eq!(fs::metadata(&second).unwrap().modified().unwrap(), stamp);
        assert_eq!(fs::read(&first).unwrap().len(), PDFIUM.len());
    }
}
