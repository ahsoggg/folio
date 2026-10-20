//! PDF through pdfium. pdfium is not thread-safe, so one worker thread owns the library and every open document;
//! the rest of the program talks to it with requests and gets answers through channels or callbacks.

use pdfium_render::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

pub type DocId = u32;

/// How the page bitmap is toned: the paper, the sepia of a printed book, or inverted for a dark theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    Paper,
    Sepia,
    Dark,
}

#[derive(Clone, Debug)]
pub struct OutlineItem {
    pub level: u8,
    pub title: String,
    pub page: usize,
}

#[derive(Clone, Debug, Default)]
pub struct PdfInfo {
    /// Size of every page in points (1/72 inch).
    pub pages: Vec<(f32, f32)>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub outline: Vec<OutlineItem>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub page: usize,
    pub rects: Vec<Rect>,
    /// A few words around the match.
    pub snippet: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    Uri(String),
    Page(usize),
}

#[derive(Clone, Debug)]
pub struct Link {
    pub rect: Rect,
    pub target: LinkTarget,
}

#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub rects: Vec<Rect>,
    pub text: String,
}

pub struct Rendered {
    pub page: usize,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub enum OpenError {
    NeedPassword,
    Failed(String),
}

type Reply<T> = Sender<T>;

enum Req {
    Open {
        path: PathBuf,
        password: Option<String>,
        reply: Reply<Result<(DocId, PdfInfo), OpenError>>,
    },
    Close(DocId),
    Render {
        doc: DocId,
        page: usize,
        width: usize,
        tone: Tone,
        done: Box<dyn FnOnce(Option<Rendered>) + Send>,
    },
    Search {
        doc: DocId,
        query: String,
        case: bool,
        from: usize,
        count: usize,
        reply: Reply<Vec<Hit>>,
    },
    Select {
        doc: DocId,
        page: usize,
        from: (f32, f32),
        to: (f32, f32),
        reply: Reply<Selection>,
    },
    Text {
        doc: DocId,
        page: usize,
        reply: Reply<String>,
    },
    Links {
        doc: DocId,
        page: usize,
        reply: Reply<Vec<Link>>,
    },
}

/// A handle to the worker thread. Cheap to clone.
#[derive(Clone)]
pub struct Engine {
    tx: Sender<Req>,
}

impl Engine {
    /// Loads pdfium from `dll` and starts the worker.
    pub fn start(dll: &Path) -> Result<Engine, String> {
        let bindings = Pdfium::bind_to_library(dll).map_err(|e| format!("cannot load pdfium: {e}"))?;
        // pdfium lives as long as the process
        let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("pdfium".into())
            .spawn(move || worker(pdfium, rx))
            .map_err(|e| e.to_string())?;
        Ok(Engine { tx })
    }

    pub fn open(&self, path: &Path, password: Option<&str>) -> Result<(DocId, PdfInfo), OpenError> {
        let (reply, rx) = channel();
        self.tx
            .send(Req::Open {
                path: path.to_path_buf(),
                password: password.map(str::to_owned),
                reply,
            })
            .map_err(|_| OpenError::Failed("the PDF engine has stopped".into()))?;
        rx.recv()
            .unwrap_or_else(|_| Err(OpenError::Failed("the PDF engine has stopped".into())))
    }

    pub fn close(&self, doc: DocId) {
        let _ = self.tx.send(Req::Close(doc));
    }

    /// Renders a page `width` pixels wide; `done` runs on the worker thread with the result.
    pub fn render(
        &self,
        doc: DocId,
        page: usize,
        width: usize,
        tone: Tone,
        done: impl FnOnce(Option<Rendered>) + Send + 'static,
    ) {
        let _ = self.tx.send(Req::Render {
            doc,
            page,
            width,
            tone,
            done: Box::new(done),
        });
    }

    /// Searches pages `from..from + count`; the answer arrives on the returned receiver.
    pub fn search(&self, doc: DocId, query: &str, case: bool, from: usize, count: usize) -> Receiver<Vec<Hit>> {
        let (reply, rx) = channel();
        let _ = self.tx.send(Req::Search {
            doc,
            query: query.to_owned(),
            case,
            from,
            count,
            reply,
        });
        rx
    }

    pub fn select(&self, doc: DocId, page: usize, from: (f32, f32), to: (f32, f32)) -> Selection {
        self.select_rx(doc, page, from, to).recv().unwrap_or_default()
    }

    /// Like `select`, but the answer arrives later on the returned receiver.
    pub fn select_rx(&self, doc: DocId, page: usize, from: (f32, f32), to: (f32, f32)) -> Receiver<Selection> {
        let (reply, rx) = channel();
        let _ = self.tx.send(Req::Select {
            doc,
            page,
            from,
            to,
            reply,
        });
        rx
    }

    pub fn page_text(&self, doc: DocId, page: usize) -> String {
        let (reply, rx) = channel();
        let _ = self.tx.send(Req::Text { doc, page, reply });
        rx.recv().unwrap_or_default()
    }

    pub fn links(&self, doc: DocId, page: usize) -> Vec<Link> {
        self.links_rx(doc, page).recv().unwrap_or_default()
    }

    pub fn links_rx(&self, doc: DocId, page: usize) -> Receiver<Vec<Link>> {
        let (reply, rx) = channel();
        let _ = self.tx.send(Req::Links { doc, page, reply });
        rx
    }
}

struct CharBox {
    ch: char,
    rect: Rect,
}

fn rect_of(r: &PdfRect) -> Rect {
    Rect {
        left: r.left().value,
        bottom: r.bottom().value,
        right: r.right().value,
        top: r.top().value,
    }
}

fn page_chars(page: &PdfPage) -> Vec<CharBox> {
    let Ok(text) = page.text() else { return Vec::new() };
    text.chars()
        .iter()
        .filter_map(|c| {
            let ch = c.unicode_char()?;
            let rect = rect_of(&c.loose_bounds().ok()?);
            Some(CharBox { ch, rect })
        })
        .collect()
}

fn distance(r: &Rect, p: (f32, f32)) -> f32 {
    let dx = (r.left - p.0).max(0.0).max(p.0 - r.right);
    let dy = (r.bottom - p.1).max(0.0).max(p.1 - r.top);
    // lines matter more than columns: a point beside a word belongs to its line
    dx + dy * 3.0
}

fn nearest(chars: &[CharBox], p: (f32, f32)) -> Option<usize> {
    chars
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.ch.is_control() || c.ch == '\n')
        .min_by(|a, b| {
            distance(&a.1.rect, p)
                .partial_cmp(&distance(&b.1.rect, p))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
}

/// Merges the boxes of consecutive characters into one rectangle per line.
fn line_rects(chars: &[CharBox]) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for c in chars {
        if c.ch.is_control() || c.rect.right <= c.rect.left {
            continue;
        }
        if let Some(last) = out.last_mut() {
            let height = (last.top - last.bottom).max(1.0);
            let same_line =
                (c.rect.bottom - last.bottom).abs() < height * 0.6 && (c.rect.top - last.top).abs() < height * 0.6;
            if same_line && c.rect.left <= last.right + height * 2.0 && c.rect.right >= last.left {
                last.left = last.left.min(c.rect.left);
                last.right = last.right.max(c.rect.right);
                last.bottom = last.bottom.min(c.rect.bottom);
                last.top = last.top.max(c.rect.top);
                continue;
            }
        }
        out.push(c.rect);
    }
    out
}

fn chars_to_text(chars: &[CharBox]) -> String {
    let mut s = String::new();
    let mut prev: Option<&Rect> = None;
    for c in chars {
        if c.ch == '\r' {
            continue;
        }
        if c.ch.is_control() && c.ch != '\n' {
            continue;
        }
        if let Some(p) = prev
            && c.ch != '\n'
            && (c.rect.top < p.bottom || c.rect.bottom > p.top)
            && !s.ends_with('\n')
        {
            // moved to another line
            s.push('\n');
        }
        s.push(c.ch);
        prev = Some(&c.rect);
    }
    s
}

fn tone_pixels(rgba: &mut [u8], tone: Tone) {
    match tone {
        Tone::Paper => {}
        Tone::Sepia => {
            // multiply by the paper colour of the sepia theme
            for px in rgba.as_chunks_mut::<4>().0 {
                px[0] = (px[0] as u32 * 244 / 255) as u8;
                px[1] = (px[1] as u32 * 232 / 255) as u8;
                px[2] = (px[2] as u32 * 205 / 255) as u8;
            }
        }
        Tone::Dark => {
            // invert, then map black..white to the dark theme's background..text colours
            for px in rgba.as_chunks_mut::<4>().0 {
                for (i, (lo, hi)) in [(30u32, 205u32), (30, 214), (46, 244)].into_iter().enumerate() {
                    let inv = 255 - px[i] as u32;
                    px[i] = (lo + inv * (hi - lo) / 255) as u8;
                }
            }
        }
    }
}

fn snippet(chars: &[CharBox], start: usize, len: usize) -> String {
    let from = start.saturating_sub(24);
    let to = (start + len + 40).min(chars.len());
    chars[from..to]
        .iter()
        .map(|c| if c.ch.is_control() { ' ' } else { c.ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn outline(doc: &PdfDocument) -> Vec<OutlineItem> {
    fn walk(b: PdfBookmark, level: u8, out: &mut Vec<OutlineItem>) {
        let mut cur = Some(b);
        while let Some(b) = cur {
            let page = b
                .destination()
                .and_then(|d| d.page_index().ok())
                .or_else(|| {
                    b.action().and_then(|a| {
                        a.as_local_destination_action()
                            .and_then(|l| l.destination().ok()?.page_index().ok())
                    })
                })
                .unwrap_or(0);
            if let Some(title) = b.title().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
                && out.len() < 5000
            {
                out.push(OutlineItem {
                    level,
                    title,
                    page: page as usize,
                });
            }
            if level < 6
                && let Some(child) = b.first_child()
            {
                walk(child, level + 1, out);
            }
            cur = b.next_sibling();
        }
    }
    let mut out = Vec::new();
    if let Some(root) = doc.bookmarks().root() {
        walk(root, 1, &mut out);
    }
    out
}

type CharCache = Vec<((DocId, usize), std::rc::Rc<Vec<CharBox>>)>;

fn worker(pdfium: &'static Pdfium, rx: Receiver<Req>) {
    let mut docs: HashMap<DocId, PdfDocument<'static>> = HashMap::new();
    let mut next_id: DocId = 1;
    let mut cache: CharCache = Vec::new();
    let chars_of = |cache: &mut CharCache,
                    id: DocId,
                    doc: &PdfDocument<'static>,
                    page: usize|
     -> Option<std::rc::Rc<Vec<CharBox>>> {
        if let Some((_, c)) = cache.iter().find(|(k, _)| *k == (id, page)) {
            return Some(c.clone());
        }
        let p = doc.pages().get(page as PdfPageIndex).ok()?;
        let c = std::rc::Rc::new(page_chars(&p));
        cache.push(((id, page), c.clone()));
        if cache.len() > 12 {
            cache.remove(0);
        }
        Some(c)
    };
    while let Ok(req) = rx.recv() {
        match req {
            Req::Open { path, password, reply } => {
                let result = match pdfium.load_pdf_from_file(&path, password.as_deref()) {
                    Ok(doc) => {
                        let mut info = PdfInfo::default();
                        for p in doc.pages().iter() {
                            info.pages.push((p.width().value, p.height().value));
                        }
                        let meta = doc.metadata();
                        let tag = |t| {
                            meta.get(t)
                                .map(|m| m.value().trim().to_owned())
                                .filter(|v| !v.is_empty())
                        };
                        info.title = tag(PdfDocumentMetadataTagType::Title);
                        info.author = tag(PdfDocumentMetadataTagType::Author);
                        info.outline = outline(&doc);
                        if info.pages.is_empty() {
                            Err(OpenError::Failed("the document has no pages".into()))
                        } else {
                            let id = next_id;
                            next_id += 1;
                            docs.insert(id, doc);
                            Ok((id, info))
                        }
                    }
                    Err(PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)) => {
                        Err(OpenError::NeedPassword)
                    }
                    Err(e) => Err(OpenError::Failed(e.to_string())),
                };
                let _ = reply.send(result);
            }
            Req::Close(id) => {
                docs.remove(&id);
                cache.retain(|(k, _)| k.0 != id);
            }
            Req::Render {
                doc,
                page,
                width,
                tone,
                done,
            } => {
                let out = docs.get(&doc).and_then(|d| {
                    let p = d.pages().get(page as PdfPageIndex).ok()?;
                    let cfg = PdfRenderConfig::new()
                        .set_target_width(width.clamp(16, 12000) as Pixels)
                        .set_format(PdfBitmapFormat::BGRA)
                        .set_clear_color(PdfColor::WHITE)
                        .render_annotations(true)
                        .render_form_data(true);
                    let bmp = p.render_with_config(&cfg).ok()?;
                    let mut rgba = bmp.as_rgba_bytes();
                    tone_pixels(&mut rgba, tone);
                    Some(Rendered {
                        page,
                        width: bmp.width() as usize,
                        height: bmp.height() as usize,
                        rgba,
                    })
                });
                done(out);
            }
            Req::Search {
                doc,
                query,
                case,
                from,
                count,
                reply,
            } => {
                let mut hits = Vec::new();
                if let Some(d) = docs.get(&doc) {
                    let n = d.pages().len() as usize;
                    for page in from..(from + count).min(n) {
                        let Ok(p) = d.pages().get(page as PdfPageIndex) else {
                            continue;
                        };
                        let Ok(text) = p.text() else { continue };
                        let opts = PdfSearchOptions::new().match_case(case);
                        let Ok(search) = text.search(&query, &opts) else {
                            continue;
                        };
                        let mut page_chars_cache: Option<Vec<CharBox>> = None;
                        for segs in search.iter(PdfSearchDirection::SearchForward) {
                            let rects: Vec<Rect> = segs.iter().map(|s| rect_of(&s.bounds())).collect();
                            if rects.is_empty() {
                                continue;
                            }
                            let chars = page_chars_cache.get_or_insert_with(|| page_chars(&p));
                            let first = &rects[0];
                            let at = chars
                                .iter()
                                .position(|c| {
                                    c.rect.left >= first.left - 0.5
                                        && distance(
                                            &c.rect,
                                            ((first.left + first.right) / 2.0, (first.top + first.bottom) / 2.0),
                                        ) < 1.0
                                })
                                .unwrap_or(0);
                            hits.push(Hit {
                                page,
                                snippet: snippet(chars, at, query.chars().count()),
                                rects,
                            });
                            if hits.len() > 2000 {
                                break;
                            }
                        }
                    }
                }
                let _ = reply.send(hits);
            }
            Req::Select {
                doc,
                page,
                from,
                to,
                reply,
            } => {
                let sel = docs
                    .get(&doc)
                    .and_then(|d| chars_of(&mut cache, doc, d, page))
                    .and_then(|chars| {
                        let a = nearest(&chars, from)?;
                        let b = nearest(&chars, to)?;
                        let (lo, hi) = (a.min(b), a.max(b));
                        let part = &chars[lo..=hi];
                        Some(Selection {
                            rects: line_rects(part),
                            text: chars_to_text(part),
                        })
                    })
                    .unwrap_or_default();
                let _ = reply.send(sel);
            }
            Req::Text { doc, page, reply } => {
                let text = docs
                    .get(&doc)
                    .and_then(|d| d.pages().get(page as PdfPageIndex).ok())
                    .and_then(|p| p.text().ok().map(|t| t.all()))
                    .unwrap_or_default();
                let _ = reply.send(text);
            }
            Req::Links { doc, page, reply } => {
                let mut out = Vec::new();
                if let Some(p) = docs.get(&doc).and_then(|d| d.pages().get(page as PdfPageIndex).ok()) {
                    for l in p.links().iter() {
                        let Ok(rect) = l.rect() else { continue };
                        let target = l
                            .action()
                            .and_then(|a| {
                                a.as_uri_action()
                                    .and_then(|u| u.uri().ok())
                                    .map(LinkTarget::Uri)
                                    .or_else(|| {
                                        a.as_local_destination_action()
                                            .and_then(|l| l.destination().ok()?.page_index().ok())
                                            .map(|i| LinkTarget::Page(i as usize))
                                    })
                            })
                            .or_else(|| {
                                l.destination()
                                    .and_then(|d| d.page_index().ok())
                                    .map(|i| LinkTarget::Page(i as usize))
                            });
                        if let Some(target) = target {
                            out.push(Link {
                                rect: rect_of(&rect),
                                target,
                            });
                        }
                    }
                }
                let _ = reply.send(out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_keep_black_and_white_inside_the_theme() {
        let mut px = vec![255, 255, 255, 255, 0, 0, 0, 255];
        tone_pixels(&mut px, Tone::Dark);
        assert_eq!(&px[..3], &[30, 30, 46]);
        assert_eq!(&px[4..7], &[205, 214, 244]);
        let mut px = vec![255, 255, 255, 255];
        tone_pixels(&mut px, Tone::Sepia);
        assert_eq!(&px[..3], &[244, 232, 205]);
    }

    #[test]
    fn lines_are_merged_into_rectangles() {
        let c = |ch: char, l: f32, b: f32| CharBox {
            ch,
            rect: Rect {
                left: l,
                bottom: b,
                right: l + 5.0,
                top: b + 10.0,
            },
        };
        let chars = vec![c('a', 0.0, 0.0), c('b', 5.0, 0.0), c('c', 10.0, 0.0), c('d', 0.0, 20.0)];
        let r = line_rects(&chars);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].left, r[0].right), (0.0, 15.0));
        assert_eq!(chars_to_text(&chars), "abc\nd");
        assert_eq!(nearest(&chars, (11.0, 5.0)), Some(2));
    }
}
