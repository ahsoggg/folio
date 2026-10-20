//! Reading members of ZIP-based formats (docx, epub, odt, ...) with case-insensitive names.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};

const MAX_MEMBER: u64 = 512 << 20;

pub struct Archive<R: Read + Seek> {
    zip: zip::ZipArchive<R>,
    index: HashMap<String, usize>,
}

impl Archive<BufReader<File>> {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let f = File::open(path).map_err(|e| e.to_string())?;
        Archive::new(BufReader::new(f))
    }
}

impl<R: Read + Seek> Archive<R> {
    pub fn new(reader: R) -> Result<Self, String> {
        let mut zip = zip::ZipArchive::new(reader).map_err(|e| format!("not a valid ZIP container: {e}"))?;
        let mut index = HashMap::new();
        for i in 0..zip.len() {
            if let Ok(f) = zip.by_index_raw(i)
                && let Ok(name) = f.name()
            {
                index.insert(name.replace('\\', "/").to_ascii_lowercase(), i);
            }
        }
        Ok(Archive { zip, index })
    }

    pub fn has(&self, name: &str) -> bool {
        self.index.contains_key(&name.to_ascii_lowercase())
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<(usize, String)> = self.index.iter().map(|(k, &i)| (i, k.clone())).collect();
        v.sort();
        v.into_iter().map(|(_, k)| k).collect()
    }

    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        let i = *self.index.get(&name.replace('\\', "/").to_ascii_lowercase())?;
        let f = self.zip.by_index(i).ok()?;
        let mut buf = Vec::new();
        f.take(MAX_MEMBER).read_to_end(&mut buf).ok()?;
        Some(buf)
    }

    pub fn read_str(&mut self, name: &str) -> Option<String> {
        let bytes = self.read(name)?;
        Some(crate::textenc::decode(&bytes).0)
    }
}

/// Resolves `target` relative to the folder of `base` (`word/document.xml` + `media/a.png` → `word/media/a.png`).
pub fn resolve(base: &str, target: &str) -> String {
    let target = target.split('#').next().unwrap_or(target);
    if let Some(rest) = target.strip_prefix('/') {
        return rest.to_owned();
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(resolve("word/document.xml", "media/a.png"), "word/media/a.png");
        assert_eq!(resolve("OEBPS/text/ch1.xhtml", "../img/a.jpg#x"), "OEBPS/img/a.jpg");
        assert_eq!(resolve("a/b.xml", "/c/d.png"), "c/d.png");
    }
}
