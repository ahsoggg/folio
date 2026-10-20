//! Folio core: reads documents of many formats into one model (no UI).

pub mod docx;
pub mod epub;
pub mod fb2;
pub mod html;
pub mod md;
pub mod model;
pub mod odt;
pub mod pdf;
pub mod pptx;
pub mod rtf;
pub mod sheet;
pub mod testkit;
pub mod text;
pub mod textenc;
pub mod xml;
pub mod zipx;

pub use model::*;

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Extensions treated as source code or structured text, shown monospaced with line numbers.
pub const CODE_EXTS: &[&str] = &[
    "rs",
    "py",
    "pyw",
    "js",
    "mjs",
    "cjs",
    "jsx",
    "ts",
    "tsx",
    "c",
    "h",
    "cpp",
    "cc",
    "cxx",
    "hpp",
    "hh",
    "cs",
    "java",
    "kt",
    "kts",
    "go",
    "rb",
    "php",
    "swift",
    "lua",
    "luau",
    "sh",
    "bash",
    "zsh",
    "bat",
    "cmd",
    "ps1",
    "sql",
    "css",
    "scss",
    "less",
    "yml",
    "yaml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "properties",
    "env",
    "log",
    "gitignore",
    "dockerfile",
    "makefile",
    "cmake",
    "gradle",
    "r",
    "pl",
    "scala",
    "dart",
    "vue",
    "svelte",
    "tex",
    "bib",
    "diff",
    "patch",
    "gd",
    "tscn",
    "tres",
    "csproj",
    "sln",
    "props",
    "targets",
    "rc",
    "def",
    "asm",
    "s",
    "vb",
    "fs",
    "ex",
    "exs",
    "erl",
    "hs",
    "ml",
    "clj",
    "nim",
    "zig",
    "v",
    "sv",
    "vhd",
    "proto",
    "graphql",
    "lock",
];

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff", "ico"];

/// Everything the Open dialog offers, grouped for the filter list.
pub fn supported_groups() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("PDF", vec!["pdf"]),
        (
            "Word, OpenDocument, RTF",
            vec!["docx", "docm", "dotx", "odt", "ott", "rtf"],
        ),
        ("Books", vec!["epub", "fb2", "zip", "cbz"]),
        ("Spreadsheets", vec!["xlsx", "xlsm", "xlsb", "xls", "ods", "csv", "tsv"]),
        ("Presentations", vec!["pptx", "ppsx", "potx", "odp"]),
        (
            "Markdown, HTML, text",
            vec![
                "md", "markdown", "html", "htm", "xhtml", "txt", "text", "json", "xml", "svg",
            ],
        ),
        ("Images", IMAGE_EXTS.to_vec()),
        ("Source code", CODE_EXTS.to_vec()),
    ]
}

/// Extensions Folio offers to open from Explorer ("Open with"): the real document formats, not source code.
pub fn document_exts() -> Vec<&'static str> {
    supported_groups()
        .into_iter()
        .take(6)
        .flat_map(|(_, e)| e)
        .filter(|e| !matches!(*e, "zip" | "xml" | "svg"))
        .collect()
}

pub fn kind_from_ext(ext: &str) -> Option<Kind> {
    let e = ext.to_ascii_lowercase();
    let e = e.as_str();
    Some(match e {
        "pdf" => Kind::Pdf,
        "docx" | "docm" | "dotx" => Kind::Docx,
        "odt" | "ott" => Kind::Odt,
        "rtf" => Kind::Rtf,
        "epub" => Kind::Epub,
        "fb2" => Kind::Fb2,
        "md" | "markdown" | "mdown" => Kind::Markdown,
        "html" | "htm" | "xhtml" => Kind::Html,
        "txt" | "text" => Kind::Text,
        "csv" | "tsv" => Kind::Csv,
        "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" => Kind::Sheet,
        "pptx" | "ppsx" | "potx" => Kind::Pptx,
        "odp" => Kind::Odp,
        "json" | "jsonl" | "geojson" => Kind::Json,
        "xml" | "svg" | "rss" | "atom" | "xsd" | "xsl" | "plist" => Kind::Xml,
        "cbz" => Kind::Comic,
        _ if IMAGE_EXTS.contains(&e) => Kind::Image,
        _ if CODE_EXTS.contains(&e) => Kind::Code,
        _ => return None,
    })
}

fn looks_like_text(head: &[u8]) -> bool {
    !head.is_empty() && !head.contains(&0) || head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF])
}

/// Kind by name first, then by the first bytes.
pub fn detect(path: &Path, head: &[u8]) -> Option<Kind> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".fb2.zip") {
        return Some(Kind::Fb2);
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(k) = kind_from_ext(&ext) {
        return Some(k);
    }
    if matches!(
        name.as_str(),
        "makefile" | "dockerfile" | "cmakelists.txt" | "readme" | "license"
    ) {
        return Some(Kind::Code);
    }
    if head.starts_with(b"%PDF-") {
        return Some(Kind::Pdf);
    }
    if head.starts_with(b"{\\rtf") {
        return Some(Kind::Rtf);
    }
    if head.starts_with(b"\x89PNG") || head.starts_with(&[0xFF, 0xD8, 0xFF]) || head.starts_with(b"GIF8") {
        return Some(Kind::Image);
    }
    if looks_like_text(head) {
        return Some(Kind::Text);
    }
    None
}

pub const MAX_TEXT_BYTES: u64 = 256 << 20;

fn read_text_file(path: &Path, declared: bool) -> Result<String, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_TEXT_BYTES {
        return Err(format!("the file is too large to open as text ({} MB)", size >> 20));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(if declared {
        textenc::decode_declared(&bytes)
    } else {
        textenc::decode(&bytes).0
    })
}

pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = String::new();
                    while let Some(&c) = it.peek() {
                        if c.is_ascii_digit() {
                            n.push(c);
                            it.next();
                        } else {
                            break;
                        }
                    }
                    n.trim_start_matches('0').to_owned()
                };
                let (na, nb) = (take(&mut ai), take(&mut bi));
                let ord = na.len().cmp(&nb.len()).then(na.cmp(&nb));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().to_string().cmp(&y.to_lowercase().to_string());
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

fn comic_pages(path: &Path) -> Result<Vec<ImagePage>, String> {
    let mut zip = zipx::Archive::open(path)?;
    let mut names: Vec<String> = zip
        .names()
        .into_iter()
        .filter(|n| {
            let e = n.rsplit('.').next().unwrap_or("");
            IMAGE_EXTS.contains(&e) && !n.contains("__macosx")
        })
        .collect();
    names.sort_by(|a, b| natural_cmp(a, b));
    let pages: Vec<ImagePage> = names
        .into_iter()
        .filter_map(|n| {
            let bytes = zip.read(&n)?;
            Some(ImagePage {
                name: n,
                bytes: Arc::new(bytes),
            })
        })
        .collect();
    if pages.is_empty() {
        return Err("the archive contains no pictures".into());
    }
    Ok(pages)
}

fn local_image_loader(base: PathBuf) -> impl FnMut(&str) -> Option<Vec<u8>> {
    move |src: &str| {
        if src.contains("://") || src.starts_with("data:") {
            return None;
        }
        let rel = src.split(['#', '?']).next().unwrap_or(src).replace("%20", " ");
        let p = base.join(rel);
        let meta = std::fs::metadata(&p).ok()?;
        (meta.len() < (64 << 20)).then(|| std::fs::read(p).ok()).flatten()
    }
}

/// Reads any supported file into a `Document`. PDF is returned as `Body::Pdf` (rendered by `pdf::Engine`).
pub fn open(path: &Path) -> Result<Document, String> {
    use std::io::Read;
    let size_bytes = std::fs::metadata(path)
        .map_err(|e| format!("cannot open the file: {e}"))?
        .len();
    let mut head = vec![0u8; 4096];
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| format!("cannot open the file: {e}"))?;
    head.truncate(n);
    let kind = detect(path, &head).ok_or("this file format is not supported")?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".into());
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    let mut meta = Meta::default();
    let body = match kind {
        Kind::Pdf => Body::Pdf,
        Kind::Docx => {
            let (f, m) = docx::read(&mut zipx::Archive::open(path)?)?;
            meta = m;
            Body::Flow(f)
        }
        Kind::Odt => {
            let (f, m) = odt::read(&mut zipx::Archive::open(path)?)?;
            meta = m;
            Body::Flow(f)
        }
        Kind::Odp => {
            let (f, m) = odt::read_presentation(&mut zipx::Archive::open(path)?)?;
            meta = m;
            Body::Flow(f)
        }
        Kind::Pptx => {
            let (f, m) = pptx::read(&mut zipx::Archive::open(path)?)?;
            meta = m;
            Body::Flow(f)
        }
        Kind::Epub => {
            let (f, m) = epub::read(&mut zipx::Archive::open(path)?)?;
            meta = m;
            Body::Flow(f)
        }
        Kind::Fb2 => {
            let text = if path.to_string_lossy().to_ascii_lowercase().ends_with(".zip") {
                let mut zip = zipx::Archive::open(path)?;
                let name = zip
                    .names()
                    .into_iter()
                    .find(|n| n.ends_with(".fb2"))
                    .ok_or("the archive has no .fb2 file")?;
                textenc::decode_declared(&zip.read(&name).ok_or("cannot read the .fb2 file")?)
            } else {
                read_text_file(path, true)?
            };
            let (f, m) = fb2::read(&text)?;
            meta = m;
            Body::Flow(f)
        }
        Kind::Rtf => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            let mut f = Flow::default();
            rtf::convert(&bytes, &mut f);
            f.build_toc();
            Body::Flow(f)
        }
        Kind::Markdown => {
            let text = read_text_file(path, false)?;
            let mut f = Flow::default();
            md::convert(&text, &mut f, &mut local_image_loader(dir));
            f.build_toc();
            Body::Flow(f)
        }
        Kind::Html => {
            let text = read_text_file(path, true)?;
            meta.title = html::title_of(&text);
            let mut f = Flow::default();
            html::convert(&text, &mut f, &mut local_image_loader(dir));
            f.build_toc();
            Body::Flow(f)
        }
        Kind::Text => {
            let text = read_text_file(path, false)?;
            Body::Flow(text::prose_flow(&text))
        }
        Kind::Code => {
            let text = read_text_file(path, false)?;
            Body::Flow(text::code_flow(&text, (!ext.is_empty()).then_some(ext.as_str())))
        }
        Kind::Json => {
            let text = read_text_file(path, false)?;
            Body::Flow(text::json_flow(&text))
        }
        Kind::Xml => {
            let text = read_text_file(path, true)?;
            Body::Flow(text::code_flow(&text, Some("xml")))
        }
        Kind::Csv => {
            let text = read_text_file(path, false)?;
            let delim = (ext == "tsv").then_some('\t');
            Body::Sheets(vec![sheet::read_csv(&text, delim)])
        }
        Kind::Sheet => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            Body::Sheets(sheet::read_workbook(bytes)?)
        }
        Kind::Image => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            Body::Images(vec![ImagePage {
                name: stem.clone(),
                bytes: Arc::new(bytes),
            }])
        }
        Kind::Comic => Body::Images(comic_pages(path)?),
    };
    let pages = match &body {
        Body::Images(p) => Some(p.len()),
        _ => None,
    };
    meta.pages = meta.pages.or(pages);
    let title = meta.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or(stem);
    Ok(Document {
        kind,
        title,
        meta,
        body,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_by_name_and_content() {
        assert_eq!(detect(Path::new("a.DOCX"), b""), Some(Kind::Docx));
        assert_eq!(detect(Path::new("a.fb2.zip"), b"PK"), Some(Kind::Fb2));
        assert_eq!(detect(Path::new("noext"), b"%PDF-1.7"), Some(Kind::Pdf));
        assert_eq!(detect(Path::new("noext"), b"hello world"), Some(Kind::Text));
        assert_eq!(detect(Path::new("noext"), &[0, 1, 2, 3]), None);
        assert_eq!(detect(Path::new("main.RS"), b""), Some(Kind::Code));
    }

    #[test]
    fn natural_sort_orders_pages() {
        let mut v = vec!["p10.png", "p2.png", "p1.png", "P03.png"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["p1.png", "p2.png", "P03.png", "p10.png"]);
    }

    #[test]
    fn opens_files_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let docx = testkit::write(dir.path(), "a.docx", &testkit::sample_docx());
        let doc = open(&docx).unwrap();
        assert_eq!(doc.kind, Kind::Docx);
        assert_eq!(doc.title, "Тестовый документ");
        assert!(doc.plain_text().contains("Вторая страница"));
        assert!(doc.word_count() > 5);

        let txt = testkit::write(dir.path(), "b.txt", "Привет\nмир".as_bytes());
        assert_eq!(open(&txt).unwrap().plain_text(), "Привет\nмир\n");

        let csv = testkit::write(dir.path(), "c.csv", b"a,b\n1,2\n");
        let Body::Sheets(s) = open(&csv).unwrap().body else {
            panic!()
        };
        assert_eq!(s[0].rows.len(), 2);

        let png = testkit::tiny_png();
        let cbz = testkit::zip_bytes(&[("2.png", &png), ("10.png", &png), ("1.png", &png)]);
        let cbz = testkit::write(dir.path(), "d.cbz", &cbz);
        let Body::Images(p) = open(&cbz).unwrap().body else {
            panic!()
        };
        assert_eq!(
            p.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            vec!["1.png", "2.png", "10.png"]
        );

        let junk = testkit::write(dir.path(), "e.bin", &[0, 1, 2, 0, 5]);
        assert!(open(&junk).is_err());
        assert!(open(&dir.path().join("missing.pdf")).is_err());
    }
}
