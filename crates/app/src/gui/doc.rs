//! Open documents: loading on a worker thread, the per-tab state, positions and the table of contents.

use super::flowview::FlowView;
use super::paged::PagedView;
use super::sheetview::SheetView;
use crate::library::Position;
use eframe::egui;
use folio_core::pdf::{Engine, OpenError, PdfInfo};
use folio_core::{Body, Flow, ImagePage, Kind, Sheet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug)]
pub struct TocEntry {
    pub level: u8,
    pub title: String,
    pub pos: Position,
}

pub struct Info {
    pub kind: Kind,
    pub size: u64,
    pub words: usize,
    pub author: Option<String>,
    pub title: Option<String>,
    pub pages: Option<usize>,
    pub toc: Vec<TocEntry>,
}

pub enum Loaded {
    Flow(Arc<Flow>),
    Sheets(Arc<Vec<Sheet>>),
    Images(Arc<Vec<ImagePage>>),
    Pdf {
        engine: Engine,
        doc: folio_core::pdf::DocId,
        info: PdfInfo,
    },
}

pub struct LoadOk {
    pub info: Info,
    pub title: String,
    pub content: Loaded,
}

pub enum LoadErr {
    NeedPassword,
    Failed(String),
}

pub type LoadResult = Result<LoadOk, LoadErr>;

pub enum View {
    Flow(FlowView),
    Sheet(SheetView),
    Paged(PagedView),
}

pub enum State {
    Loading(Receiver<LoadResult>),
    Failed(String),
    NeedPassword { input: String, wrong: bool },
    Ready(Box<View>),
}

pub struct Tab {
    pub id: u64,
    pub path: PathBuf,
    pub key: String,
    pub title: String,
    pub kind: Option<Kind>,
    pub state: State,
    pub info: Option<Info>,
    /// Where to continue when the document has loaded.
    pub restore: Option<(Position, Option<f32>)>,
}

static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();

/// pdfium, started on first use.
pub fn pdf_engine() -> Result<Engine, String> {
    ENGINE
        .get_or_init(|| {
            let dll = crate::pdfdll::ensure_extracted()?;
            Engine::start(&dll)
        })
        .clone()
}

fn toc_from_flow(flow: &Flow) -> Vec<TocEntry> {
    flow.toc
        .iter()
        .map(|t| TocEntry {
            level: t.level.clamp(1, 6),
            title: t.title.clone(),
            pos: Position {
                index: t.block,
                frac: 0.0,
            },
        })
        .collect()
}

pub fn load(path: &Path, password: Option<String>) -> LoadResult {
    let doc = folio_core::open(path).map_err(LoadErr::Failed)?;
    let size = doc.size_bytes;
    let (author, title) = (doc.meta.author.clone(), doc.meta.title.clone());
    let kind = doc.kind;
    let base = |words: usize, pages: Option<usize>, toc: Vec<TocEntry>| Info {
        kind,
        size,
        words,
        author: author.clone(),
        title: title.clone(),
        pages,
        toc,
    };
    match doc.body {
        Body::Flow(flow) => {
            let words = flow
                .blocks
                .iter()
                .map(|b| b.plain_text().split_whitespace().count())
                .sum();
            let toc = toc_from_flow(&flow);
            Ok(LoadOk {
                info: base(words, None, toc),
                title: doc.title,
                content: Loaded::Flow(Arc::new(flow)),
            })
        }
        Body::Sheets(sheets) => {
            let words = sheets
                .iter()
                .map(|s| s.rows.iter().map(|r| r.len()).sum::<usize>())
                .sum();
            Ok(LoadOk {
                info: base(words, None, Vec::new()),
                title: doc.title,
                content: Loaded::Sheets(Arc::new(sheets)),
            })
        }
        Body::Images(pages) => {
            let n = pages.len();
            let toc = if n > 1 {
                (0..n)
                    .step_by((n / 40).max(1))
                    .map(|i| TocEntry {
                        level: 1,
                        title: pages[i].name.rsplit('/').next().unwrap_or("").to_owned(),
                        pos: Position { index: i, frac: 0.0 },
                    })
                    .collect()
            } else {
                Vec::new()
            };
            Ok(LoadOk {
                info: base(0, Some(n), toc),
                title: doc.title,
                content: Loaded::Images(Arc::new(pages)),
            })
        }
        Body::Pdf => {
            let engine = pdf_engine().map_err(LoadErr::Failed)?;
            let (id, info) = engine.open(path, password.as_deref()).map_err(|e| match e {
                OpenError::NeedPassword => LoadErr::NeedPassword,
                OpenError::Failed(m) => LoadErr::Failed(m),
            })?;
            let toc = info
                .outline
                .iter()
                .map(|o| TocEntry {
                    level: o.level.clamp(1, 6),
                    title: o.title.clone(),
                    pos: Position {
                        index: o.page,
                        frac: 0.0,
                    },
                })
                .collect();
            let mut meta = base(0, Some(info.pages.len()), toc);
            meta.title = info.title.clone().or(meta.title);
            meta.author = info.author.clone().or(meta.author);
            let title = info.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or(doc.title);
            Ok(LoadOk {
                info: meta,
                title,
                content: Loaded::Pdf { engine, doc: id, info },
            })
        }
    }
}

pub fn spawn_load(ctx: &egui::Context, path: PathBuf, password: Option<String>) -> Receiver<LoadResult> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(|| load(&path, password)).unwrap_or_else(|_| {
            Err(LoadErr::Failed(
                crate::i18n::tr(
                    "Файл повреждён или имеет неожиданную структуру",
                    "The file is damaged or has an unexpected structure",
                )
                .into(),
            ))
        });
        let _ = tx.send(r);
        ctx.request_repaint();
    });
    rx
}

impl Tab {
    pub fn new(id: u64, path: PathBuf) -> Tab {
        let key = crate::library::key(&path);
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| key.clone());
        Tab {
            id,
            path,
            key,
            title,
            kind: None,
            state: State::Failed(String::new()),
            info: None,
            restore: None,
        }
    }

    pub fn view(&self) -> Option<&View> {
        match &self.state {
            State::Ready(v) => Some(v),
            _ => None,
        }
    }

    pub fn view_mut(&mut self) -> Option<&mut View> {
        match &mut self.state {
            State::Ready(v) => Some(v),
            _ => None,
        }
    }

    /// Where the reader is: block, page or row, and how far into it.
    pub fn position(&self) -> Option<Position> {
        Some(match self.view()? {
            View::Flow(f) => f.top,
            View::Paged(p) => p.top,
            View::Sheet(s) => Position {
                index: s.top_row,
                frac: 0.0,
            },
        })
    }

    /// 0..=1 through the document.
    pub fn progress(&self) -> Option<f32> {
        Some(match self.view()? {
            View::Flow(f) => f.progress(),
            View::Paged(p) => p.progress(),
            View::Sheet(s) => s.top_row as f32 / s.sheet().rows.len().max(1) as f32,
        })
    }

    pub fn goto(&mut self, pos: Position) {
        match self.view_mut() {
            Some(View::Flow(f)) => f.goto(pos.index, pos.frac),
            Some(View::Paged(p)) => p.go_to(pos.index, pos.frac),
            Some(View::Sheet(s)) => s.goto_row(pos.index),
            None => {}
        }
    }

    /// A short text for bookmarks: the beginning of the block, the page number or the row.
    pub fn label_at(&self, pos: Position) -> String {
        match self.view() {
            Some(View::Flow(f)) => f
                .flow
                .blocks
                .get(pos.index)
                .map(|b| b.plain_text().split_whitespace().collect::<Vec<_>>().join(" "))
                .unwrap_or_default(),
            Some(View::Paged(_)) => crate::i18n::trf("Страница {0}", "Page {0}", &[&(pos.index + 1)]),
            Some(View::Sheet(_)) => crate::i18n::trf("Строка {0}", "Row {0}", &[&(pos.index + 1)]),
            None => String::new(),
        }
    }

    pub fn same_place(&self, a: Position, b: Position) -> bool {
        match self.view() {
            Some(View::Paged(_)) => a.index == b.index,
            Some(View::Flow(_)) => a.index == b.index,
            _ => a.index == b.index,
        }
    }

    /// Minutes of reading left, from the words not yet read.
    pub fn minutes_left(&self) -> Option<usize> {
        let info = self.info.as_ref()?;
        if matches!(
            info.kind,
            Kind::Sheet | Kind::Csv | Kind::Code | Kind::Image | Kind::Json | Kind::Xml
        ) || info.words == 0
        {
            return None;
        }
        let left = (1.0 - self.progress()?) * info.words as f32;
        Some((left / 220.0).ceil() as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use folio_core::testkit;

    #[test]
    fn documents_load_with_info_and_contents() {
        let dir = tempfile::tempdir().unwrap();
        let docx = testkit::write(dir.path(), "a.docx", &testkit::sample_docx());
        let ok = load(&docx, None).ok().expect("loads");
        assert_eq!(ok.title, "Тестовый документ");
        assert_eq!(ok.info.kind, Kind::Docx);
        assert!(ok.info.words > 5);
        assert_eq!(ok.info.toc.len(), 1);
        assert!(matches!(ok.content, Loaded::Flow(_)));

        let csv = testkit::write(dir.path(), "b.csv", b"a,b\n1,2\n");
        assert!(matches!(load(&csv, None).ok().unwrap().content, Loaded::Sheets(_)));

        let junk = testkit::write(dir.path(), "c.bin", &[0, 1, 2, 3]);
        assert!(matches!(load(&junk, None), Err(LoadErr::Failed(_))));
    }

    #[test]
    fn comic_archives_get_a_page_list() {
        let dir = tempfile::tempdir().unwrap();
        let png = testkit::tiny_png();
        let pages: Vec<(String, &[u8])> = (1..=3).map(|i| (format!("{i}.png"), &png[..])).collect();
        let refs: Vec<(&str, &[u8])> = pages.iter().map(|(n, b)| (n.as_str(), *b)).collect();
        let cbz = testkit::write(dir.path(), "c.cbz", &testkit::zip_bytes(&refs));
        let ok = load(&cbz, None).ok().unwrap();
        assert_eq!(ok.info.pages, Some(3));
        assert!(matches!(ok.content, Loaded::Images(p) if p.len() == 3));
    }
}
