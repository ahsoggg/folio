//! folio-cli: text and information from any document Folio can read.

use folio_core::{Body, Document, Kind, pdf};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "\
folio-cli — read documents from the command line

  folio-cli text FILE [--out FILE]   print the text of a document (PDF needs pdfium.dll: FOLIO_PDFIUM or next to the exe)
  folio-cli info FILE                kind, title, author, size, words, pages
  folio-cli formats                  list the supported extensions
  folio-cli --version
";

fn pdfium_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FOLIO_PDFIUM") {
        return Some(PathBuf::from(p));
    }
    let beside = std::env::current_exe().ok()?.with_file_name("pdfium.dll");
    beside.exists().then_some(beside)
}

fn pdf_text(path: &Path) -> Result<(String, usize), String> {
    let dll = pdfium_path().ok_or("pdfium.dll not found: set FOLIO_PDFIUM")?;
    let engine = pdf::Engine::start(&dll)?;
    let (doc, info) = engine.open(path, None).map_err(|e| match e {
        pdf::OpenError::NeedPassword => "the PDF is password protected".to_owned(),
        pdf::OpenError::Failed(m) => m,
    })?;
    let mut out = String::new();
    for page in 0..info.pages.len() {
        out.push_str(&engine.page_text(doc, page));
        out.push_str("\n\n");
    }
    Ok((out, info.pages.len()))
}

fn load(path: &Path) -> Result<(Document, String, Option<usize>), String> {
    let doc = folio_core::open(path)?;
    if matches!(doc.body, Body::Pdf) {
        let (text, pages) = pdf_text(path)?;
        return Ok((doc, text, Some(pages)));
    }
    let text = doc.plain_text();
    let pages = doc.meta.pages;
    Ok((doc, text, pages))
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("text") => {
            let file = args.get(1).ok_or("usage: folio-cli text FILE")?;
            let (_, text, _) = load(Path::new(file))?;
            match args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)) {
                Some(out) => std::fs::write(out, text).map_err(|e| e.to_string())?,
                None => print!("{text}"),
            }
            Ok(())
        }
        Some("info") => {
            let file = args.get(1).ok_or("usage: folio-cli info FILE")?;
            let (doc, text, pages) = load(Path::new(file))?;
            println!("kind:   {}", doc.kind.label());
            println!("title:  {}", doc.title);
            if let Some(a) = &doc.meta.author {
                println!("author: {a}");
            }
            println!("size:   {} bytes", doc.size_bytes);
            println!("words:  {}", text.split_whitespace().count());
            if let Some(p) = pages {
                println!("pages:  {p}");
            }
            if matches!(doc.kind, Kind::Sheet | Kind::Csv)
                && let Body::Sheets(s) = &doc.body
            {
                for sh in s {
                    println!("sheet:  {} ({} rows x {} columns)", sh.name, sh.rows.len(), sh.cols);
                }
            }
            Ok(())
        }
        Some("formats") => {
            for (group, exts) in folio_core::supported_groups() {
                println!("{group}: {}", exts.join(" "));
            }
            Ok(())
        }
        Some("--version") => {
            println!("folio-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            print!("{HELP}");
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("folio-cli: {e}");
            ExitCode::FAILURE
        }
    }
}
