//! Page-based documents: PDF (pdfium) and picture collections (comic archives, single images). Pages are laid out
//! top to bottom at the chosen zoom; only the pages near the viewport get a bitmap, rendered on a worker thread.

use super::theme::Palette;
use crate::library::Position;
use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, Rect, ScrollArea, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2,
    pos2, vec2,
};
use folio_core::ImagePage;
use folio_core::pdf::{self, DocId, Engine, Hit, Link, LinkTarget, Rendered, Selection, Tone};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

const PAD: f32 = 24.0;
const GAP: f32 = 14.0;
const SCROLLBAR: f32 = 16.0;
/// Logical pixels per point at 100 %: 96 dpi.
const BASE: f32 = 96.0 / 72.0;
const MAX_CACHED: usize = 22;
const MAX_THUMBS: usize = 220;
const THUMB_W: f32 = 128.0;
const SEARCH_CHUNK: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoomMode {
    FitWidth,
    FitPage,
    Fixed(f32),
}

pub enum PageEvent {
    OpenUrl(String),
}

pub struct ImgReq {
    index: usize,
    width: usize,
    done: Box<dyn FnOnce(Option<Rendered>) + Send>,
}

pub enum Source {
    Pdf { engine: Engine, doc: DocId },
    Images { tx: Sender<ImgReq> },
}

enum Msg {
    Page { page: usize, tone: Tone, r: Rendered },
    Thumb { page: usize, tone: Tone, r: Rendered },
    Failed { page: usize },
}

struct PageTex {
    tex: TextureHandle,
    width_px: usize,
    tone: Tone,
}

#[derive(Default)]
pub struct PdfSearch {
    pub query: String,
    pub case: bool,
    pub hits: Vec<Hit>,
    pub current: usize,
    next_page: usize,
    rx: Option<Receiver<Vec<Hit>>>,
    generation: u64,
    pub done: bool,
}

struct Sel {
    anchor: (usize, f32, f32),
    head: (usize, f32, f32),
    parts: BTreeMap<usize, Selection>,
    waiting: Vec<(usize, Receiver<Selection>)>,
    dirty: bool,
}

pub struct PagedView {
    pub src: Source,
    pub sizes: Vec<(f32, f32)>,
    pub zoom: ZoomMode,
    scale: f32,
    cache: HashMap<usize, PageTex>,
    pending: HashMap<usize, (usize, Tone)>,
    thumbs: HashMap<usize, (TextureHandle, Tone)>,
    thumb_pending: HashSet<usize>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub offset: Vec2,
    pub content: Vec2,
    pub viewport: Vec2,
    pending_offset: Option<Vec2>,
    jump: Option<(usize, f32, u8)>,
    pub current_page: usize,
    pub top: Position,
    pub search: PdfSearch,
    sel: Option<Sel>,
    links: HashMap<usize, Vec<Link>>,
    links_wait: HashMap<usize, Receiver<Vec<Link>>>,
    pub scroll_id: u64,
    pub is_pdf: bool,
    failed: HashSet<usize>,
}

fn spawn_image_worker(pages: Arc<Vec<ImagePage>>) -> Sender<ImgReq> {
    let (tx, rx) = channel::<ImgReq>();
    std::thread::spawn(move || {
        let mut last: Option<(usize, image::DynamicImage)> = None;
        while let Ok(req) = rx.recv() {
            let decoded = match &last {
                Some((i, img)) if *i == req.index => Some(img.clone()),
                _ => pages
                    .get(req.index)
                    .and_then(|p| image::load_from_memory(&p.bytes).ok())
                    .inspect(|img| last = Some((req.index, img.clone()))),
            };
            let out = decoded.map(|img| {
                let w = req.width.clamp(16, 8000) as u32;
                let h = ((img.height() as f32 * w as f32 / img.width().max(1) as f32).round() as u32).max(1);
                let rgba = if w < img.width() {
                    image::imageops::resize(&img.to_rgba8(), w, h, image::imageops::FilterType::Triangle)
                } else {
                    img.to_rgba8()
                };
                Rendered {
                    page: req.index,
                    width: rgba.width() as usize,
                    height: rgba.height() as usize,
                    rgba: rgba.into_raw(),
                }
            });
            (req.done)(out);
        }
    });
    tx
}

/// Natural size in points of every page of a picture collection (1 px = 0.75 pt, so 100 % shows real pixels).
pub fn image_sizes(pages: &[ImagePage]) -> Vec<(f32, f32)> {
    pages
        .iter()
        .map(|p| {
            image::ImageReader::new(std::io::Cursor::new(&p.bytes[..]))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.into_dimensions().ok())
                .map(|(w, h)| (w as f32 * 0.75, h as f32 * 0.75))
                .unwrap_or((600.0, 800.0))
        })
        .collect()
}

impl PagedView {
    pub fn pdf(engine: Engine, doc: DocId, sizes: Vec<(f32, f32)>, scroll_id: u64) -> PagedView {
        Self::build(Source::Pdf { engine, doc }, sizes, scroll_id, true)
    }

    pub fn images(pages: Arc<Vec<ImagePage>>, scroll_id: u64) -> PagedView {
        let sizes = image_sizes(&pages);
        let tx = spawn_image_worker(pages);
        let mut v = Self::build(Source::Images { tx }, sizes, scroll_id, false);
        v.zoom = ZoomMode::FitPage;
        v
    }

    fn build(src: Source, sizes: Vec<(f32, f32)>, scroll_id: u64, is_pdf: bool) -> PagedView {
        let (tx, rx) = channel();
        PagedView {
            src,
            sizes,
            zoom: ZoomMode::FitWidth,
            scale: BASE,
            cache: HashMap::new(),
            pending: HashMap::new(),
            thumbs: HashMap::new(),
            thumb_pending: HashSet::new(),
            tx,
            rx,
            offset: Vec2::ZERO,
            content: Vec2::ZERO,
            viewport: Vec2::ZERO,
            pending_offset: None,
            jump: None,
            current_page: 0,
            top: Position::default(),
            search: PdfSearch::default(),
            sel: None,
            links: HashMap::new(),
            links_wait: HashMap::new(),
            scroll_id,
            is_pdf,
            failed: HashSet::new(),
        }
    }

    #[cfg(test)]
    pub fn cached_pages(&self) -> usize {
        self.cache.len()
    }

    pub fn page_count(&self) -> usize {
        self.sizes.len()
    }

    pub fn percent(&self) -> f32 {
        self.scale / BASE * 100.0
    }

    pub fn progress(&self) -> f32 {
        let range = (self.content.y - self.viewport.y).max(1.0);
        (self.offset.y / range).clamp(0.0, 1.0)
    }

    pub fn go_to(&mut self, page: usize, frac: f32) {
        self.jump = Some((page.min(self.page_count().saturating_sub(1)), frac, 6));
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.pending_offset = Some(vec2(self.offset.x, (self.offset.y + dy).max(0.0)));
    }

    pub fn set_zoom_percent(&mut self, pct: f32) {
        self.zoom = ZoomMode::Fixed((pct / 100.0).clamp(0.1, 8.0));
        self.scale = (pct / 100.0).clamp(0.1, 8.0) * BASE;
    }

    pub fn zoom_step(&mut self, factor: f32) {
        let z = (self.scale / BASE * factor).clamp(0.1, 8.0);
        self.zoom = ZoomMode::Fixed(z);
    }

    fn max_page(&self) -> (f32, f32) {
        self.sizes
            .iter()
            .fold((1.0f32, 1.0f32), |(w, h), (pw, ph)| (w.max(*pw), h.max(*ph)))
    }

    fn effective_scale(&self, avail: Vec2) -> f32 {
        let (mw, mh) = self.max_page();
        match self.zoom {
            ZoomMode::FitWidth => ((avail.x - 2.0 * PAD - SCROLLBAR) / mw).clamp(0.1 * BASE, 8.0 * BASE),
            ZoomMode::FitPage => {
                (((avail.x - 2.0 * PAD - SCROLLBAR) / mw).min((avail.y - 2.0 * PAD) / mh)).clamp(0.1 * BASE, 8.0 * BASE)
            }
            ZoomMode::Fixed(z) => z * BASE,
        }
    }

    fn tops(&self, scale: f32) -> Vec<f32> {
        let mut y = PAD;
        let mut v = Vec::with_capacity(self.sizes.len() + 1);
        for (_, h) in &self.sizes {
            v.push(y);
            y += h * scale + GAP;
        }
        v.push(y - GAP + PAD);
        v
    }

    // ---- results from workers ----

    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Page { page, tone, r } => {
                    self.pending.remove(&page);
                    let img = ColorImage::from_rgba_unmultiplied([r.width, r.height], &r.rgba);
                    let tex = ctx.load_texture(format!("page-{}-{page}", self.scroll_id), img, TextureOptions::LINEAR);
                    self.cache.insert(
                        page,
                        PageTex {
                            tex,
                            width_px: r.width,
                            tone,
                        },
                    );
                }
                Msg::Thumb { page, tone, r } => {
                    self.thumb_pending.remove(&page);
                    let img = ColorImage::from_rgba_unmultiplied([r.width, r.height], &r.rgba);
                    let tex = ctx.load_texture(format!("thumb-{}-{page}", self.scroll_id), img, TextureOptions::LINEAR);
                    self.thumbs.insert(page, (tex, tone));
                }
                Msg::Failed { page } => {
                    self.pending.remove(&page);
                    self.thumb_pending.remove(&page);
                    self.failed.insert(page);
                }
            }
        }
        // selections and links
        if let Some(sel) = self.sel.as_mut() {
            sel.waiting.retain(|(p, rx)| match rx.try_recv() {
                Ok(s) => {
                    sel.parts.insert(*p, s);
                    false
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => true,
                Err(_) => false,
            });
        }
        let done: Vec<usize> = self
            .links_wait
            .iter()
            .filter_map(|(p, rx)| rx.try_recv().ok().map(|_| *p))
            .collect();
        let _ = done;
        let ready: Vec<(usize, Vec<Link>)> = {
            let mut out = Vec::new();
            self.links_wait.retain(|p, rx| match rx.try_recv() {
                Ok(v) => {
                    out.push((*p, v));
                    false
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => true,
                Err(_) => false,
            });
            out
        };
        for (p, v) in ready {
            self.links.insert(p, v);
        }
    }

    fn request(&mut self, ctx: &egui::Context, page: usize, width: usize, tone: Tone, thumb: bool) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let done = move |r: Option<Rendered>| {
            let msg = match r {
                Some(r) if thumb => Msg::Thumb { page, tone, r },
                Some(r) => Msg::Page { page, tone, r },
                None => Msg::Failed { page },
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        };
        match &self.src {
            Source::Pdf { engine, doc } => engine.render(*doc, page, width, tone, done),
            Source::Images { tx, .. } => {
                let _ = tx.send(ImgReq {
                    index: page,
                    width,
                    done: Box::new(done),
                });
            }
        }
    }

    /// A small bitmap of a page for the thumbnail strip; requested on first use.
    pub fn thumbnail(&mut self, ctx: &egui::Context, page: usize, tone: Tone) -> Option<TextureHandle> {
        if let Some((t, tn)) = self.thumbs.get(&page) {
            if *tn == tone {
                return Some(t.clone());
            }
        }
        if !self.thumb_pending.contains(&page) && !self.failed.contains(&page) {
            if self.thumbs.len() > MAX_THUMBS {
                let far: Vec<usize> = self
                    .thumbs
                    .keys()
                    .copied()
                    .filter(|p| p.abs_diff(self.current_page) > 60)
                    .collect();
                for p in far {
                    self.thumbs.remove(&p);
                }
            }
            self.thumb_pending.insert(page);
            let w = (THUMB_W * ctx.pixels_per_point()) as usize;
            self.request(ctx, page, w, tone, true);
        }
        self.thumbs.get(&page).map(|(t, _)| t.clone())
    }

    pub fn drop_thumbnails(&mut self) {
        self.thumbs.clear();
        self.thumb_pending.clear();
    }

    // ---- search (PDF) ----

    pub fn start_search(&mut self, query: &str, case: bool) {
        self.search.generation += 1;
        self.search.query = query.to_owned();
        self.search.case = case;
        self.search.hits.clear();
        self.search.current = 0;
        self.search.next_page = 0;
        self.search.rx = None;
        self.search.done = query.trim().is_empty() || !self.is_pdf;
    }

    fn pump_search(&mut self) {
        let Source::Pdf { engine, doc } = &self.src else { return };
        let (engine, doc) = (engine.clone(), *doc);
        if self.search.done {
            return;
        }
        if let Some(rx) = &self.search.rx {
            match rx.try_recv() {
                Ok(hits) => {
                    let first = self.search.hits.is_empty();
                    self.search.hits.extend(hits);
                    self.search.rx = None;
                    if first && !self.search.hits.is_empty() {
                        // jump to the first hit at or after the current page
                        let from = self.current_page;
                        self.search.current = self.search.hits.iter().position(|h| h.page >= from).unwrap_or(0);
                        self.goto_hit();
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(_) => {
                    self.search.rx = None;
                    self.search.done = true;
                    return;
                }
            }
        }
        if self.search.next_page >= self.sizes.len() || self.search.hits.len() > 2000 {
            self.search.done = true;
            return;
        }
        let from = self.search.next_page;
        self.search.next_page += SEARCH_CHUNK;
        self.search.rx = Some(engine.search(doc, &self.search.query, self.search.case, from, SEARCH_CHUNK));
    }

    pub fn is_searching(&self) -> bool {
        !self.search.done
    }

    pub fn goto_hit(&mut self) {
        if let Some(h) = self.search.hits.get(self.search.current).cloned() {
            let ph = self.sizes[h.page].1.max(1.0);
            let top = h.rects.iter().map(|r| r.top).fold(0.0f32, f32::max);
            let frac = ((ph - top) / ph - 0.08).max(0.0);
            self.go_to(h.page, frac);
        }
    }

    pub fn next_hit(&mut self, forward: bool) {
        let n = self.search.hits.len();
        if n == 0 {
            return;
        }
        self.search.current = if forward {
            (self.search.current + 1) % n
        } else {
            (self.search.current + n - 1) % n
        };
        self.goto_hit();
    }

    // ---- selection ----

    pub fn selected_text(&self) -> String {
        self.sel
            .as_ref()
            .map(|s| s.parts.values().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }

    pub fn clear_selection(&mut self) {
        self.sel = None;
    }

    fn update_selection(&mut self) {
        let Source::Pdf { engine, doc } = &self.src else { return };
        let Some(sel) = self.sel.as_mut() else { return };
        if !sel.dirty {
            return;
        }
        sel.dirty = false;
        let key = |p: &(usize, f32, f32)| (p.0, -(p.2 * 100.0) as i64, (p.1 * 100.0) as i64);
        let (a, b) = if key(&sel.anchor) <= key(&sel.head) {
            (sel.anchor, sel.head)
        } else {
            (sel.head, sel.anchor)
        };
        sel.waiting.clear();
        sel.parts.retain(|p, _| (a.0..=b.0).contains(p));
        for page in a.0..=b.0.min(a.0 + 60) {
            let from = if page == a.0 { (a.1, a.2) } else { (-1.0e6, 1.0e6) };
            let to = if page == b.0 { (b.1, b.2) } else { (1.0e6, -1.0e6) };
            sel.waiting.push((page, engine.select_rx(*doc, page, from, to)));
        }
    }

    // ---- drawing ----

    fn page_at_y(&self, tops: &[f32], y: f32) -> usize {
        match tops[..tops.len() - 1].binary_search_by(|t| t.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Less)) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, pal: &Palette, tone: Tone, events: &mut Vec<PageEvent>) {
        let ctx = ui.ctx().clone();
        self.pump(&ctx);
        self.pump_search();
        self.update_selection();
        let avail = ui.available_size();
        let ppp = ctx.pixels_per_point();

        // zoom with Ctrl + wheel or a pinch, around the pointer
        let inside = ui.rect_contains_pointer(ui.available_rect_before_wrap());
        let zd = if inside { ctx.input(|i| i.zoom_delta()) } else { 1.0 };
        let old_scale = self.scale;
        let mut new_scale = self.effective_scale(avail);
        if (zd - 1.0).abs() > 0.001 {
            new_scale = (old_scale * zd).clamp(0.1 * BASE, 8.0 * BASE);
            self.zoom = ZoomMode::Fixed(new_scale / BASE);
        }
        if (new_scale - old_scale).abs() > 1e-4 {
            // keep the point under the pointer (or the top of the view) where it is
            let rel = if inside {
                ctx.input(|i| i.pointer.hover_pos())
                    .map(|p| p - ui.available_rect_before_wrap().min)
                    .unwrap_or(vec2(avail.x / 2.0, 0.0))
            } else {
                vec2(avail.x / 2.0, 0.0)
            };
            let old_tops = self.tops(old_scale);
            let y = self.offset.y + rel.y;
            let p = self.page_at_y(&old_tops, y);
            let (ph_old, ph_new) = (self.sizes[p].1 * old_scale, self.sizes[p].1 * new_scale);
            let frac = ((y - old_tops[p]) / ph_old.max(1.0)).clamp(-0.2, 1.2);
            let new_tops = self.tops(new_scale);
            let ny = new_tops[p] + frac * ph_new - rel.y;
            let ratio = new_scale / old_scale;
            self.pending_offset = Some(vec2(((self.offset.x + rel.x) * ratio - rel.x).max(0.0), ny.max(0.0)));
        }
        self.scale = new_scale;
        let scale = self.scale;
        let tops = self.tops(scale);

        if let Some((page, frac, frames)) = self.jump {
            let h = self.sizes[page].1 * scale;
            let target = (tops[page] + h * frac - 8.0).max(0.0);
            if (target - self.offset.y).abs() > 1.0 || frames > 3 {
                self.pending_offset = Some(vec2(self.offset.x, target));
            }
            self.jump = if frames <= 1 {
                None
            } else {
                Some((page, frac, frames - 1))
            };
            ctx.request_repaint();
        }

        let mut area = ScrollArea::both()
            .id_salt(("paged", self.scroll_id))
            .auto_shrink([false, false])
            .scroll_source(egui::scroll_area::ScrollSource {
                drag: if self.is_pdf {
                    egui::scroll_area::DragScroll::Never
                } else {
                    egui::scroll_area::DragScroll::Always
                },
                ..Default::default()
            });
        if let Some(o) = self.pending_offset.take() {
            area = area.scroll_offset(o);
            ctx.request_repaint();
        }
        let content_w = (avail.x - SCROLLBAR).max(self.max_page().0 * scale + 2.0 * PAD);
        let sizes = self.sizes.clone();
        let mut hover_link: Option<(LinkTarget, Rect)> = None;
        let mut clicked_link: Option<LinkTarget> = None;
        let mut drag: Option<(u8, usize, f32, f32)> = None;
        let mut visible: Vec<usize> = Vec::new();
        let out = area.show_viewport(ui, |ui, viewport| {
            ui.set_min_size(vec2(content_w, *tops.last().unwrap_or(&0.0)));
            let origin = ui.min_rect().min;
            let first = self.page_at_y(&tops, (viewport.min.y - avail.y * 0.6).max(0.0));
            for page in first..sizes.len() {
                if tops[page] > viewport.max.y + avail.y * 0.6 {
                    break;
                }
                let (pw, ph) = (sizes[page].0 * scale, sizes[page].1 * scale);
                let x = (content_w - pw) / 2.0;
                let rect = Rect::from_min_size(origin + vec2(x, tops[page]), vec2(pw, ph));
                let on_screen =
                    rect.max.y >= origin.y + viewport.min.y - 20.0 && rect.min.y <= origin.y + viewport.max.y + 20.0;
                if on_screen {
                    visible.push(page);
                }
                // drop shadow and paper
                ui.painter().rect_filled(
                    rect.translate(vec2(0.0, 2.0)).expand(1.0),
                    3,
                    Color32::from_black_alpha(if pal.dark { 110 } else { 40 }),
                );
                let want_w = ((pw * ppp).round() as usize).clamp(32, 7000);
                let have = self.cache.get(&page);
                let fresh = have.is_some_and(|c| c.width_px.abs_diff(want_w) <= 2 && c.tone == tone);
                let mut painted = false;
                if let Some(c) = have {
                    ui.painter().image(
                        c.tex.id(),
                        rect,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    painted = true;
                }
                if !painted {
                    let paper = match tone {
                        Tone::Paper => Color32::WHITE,
                        Tone::Sepia => Color32::from_rgb(244, 232, 205),
                        Tone::Dark => Color32::from_rgb(30, 30, 46),
                    };
                    ui.painter().rect_filled(rect, 2, paper);
                }
                if !fresh && !self.failed.contains(&page) && self.pending.get(&page) != Some(&(want_w, tone)) {
                    self.pending.insert(page, (want_w, tone));
                    self.request(&ctx, page, want_w, tone, false);
                }
                if self.failed.contains(&page) {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        crate::i18n::tr("Не удалось показать страницу", "This page could not be shown"),
                        egui::FontId::proportional(14.0),
                        Color32::GRAY,
                    );
                }
                if !on_screen {
                    continue;
                }
                // search hits
                for (k, h) in self.search.hits.iter().enumerate() {
                    if h.page != page {
                        continue;
                    }
                    let col = if k == self.search.current {
                        pal.hit_current
                    } else {
                        pal.hit
                    };
                    for r in &h.rects {
                        let rr = Rect::from_min_max(
                            pos2(
                                rect.min.x + r.left * scale,
                                rect.min.y + (sizes[page].1 - r.top) * scale,
                            ),
                            pos2(
                                rect.min.x + r.right * scale,
                                rect.min.y + (sizes[page].1 - r.bottom) * scale,
                            ),
                        )
                        .expand(1.5);
                        ui.painter().rect_filled(rr, 2, col);
                    }
                }
                // selection
                if let Some(sel) = &self.sel
                    && let Some(part) = sel.parts.get(&page)
                {
                    for r in &part.rects {
                        let rr = Rect::from_min_max(
                            pos2(
                                rect.min.x + r.left * scale,
                                rect.min.y + (sizes[page].1 - r.top) * scale,
                            ),
                            pos2(
                                rect.min.x + r.right * scale,
                                rect.min.y + (sizes[page].1 - r.bottom) * scale,
                            ),
                        );
                        ui.painter().rect_filled(rr, 1, pal.selection);
                    }
                }
                // links
                if self.is_pdf {
                    if !self.links.contains_key(&page) && !self.links_wait.contains_key(&page) {
                        if let Source::Pdf { engine, doc } = &self.src {
                            self.links_wait.insert(page, engine.links_rx(*doc, page));
                        }
                    }
                    if let Some(links) = self.links.get(&page) {
                        let pointer = ctx.input(|i| i.pointer.hover_pos());
                        for l in links {
                            let rr = Rect::from_min_max(
                                pos2(
                                    rect.min.x + l.rect.left * scale,
                                    rect.min.y + (sizes[page].1 - l.rect.top) * scale,
                                ),
                                pos2(
                                    rect.min.x + l.rect.right * scale,
                                    rect.min.y + (sizes[page].1 - l.rect.bottom) * scale,
                                ),
                            );
                            if pointer.is_some_and(|p| rr.contains(p)) {
                                hover_link = Some((l.target.clone(), rr));
                            }
                        }
                    }
                }
                let id = ui.id().with(("page", page));
                let resp = ui.interact(rect, id, Sense::click_and_drag());
                if self.is_pdf {
                    if let Some(p) = resp.interact_pointer_pos().or(resp.hover_pos()) {
                        let (px, py) = ((p.x - rect.min.x) / scale, sizes[page].1 - (p.y - rect.min.y) / scale);
                        if resp.drag_started() {
                            drag = Some((0, page, px, py));
                        } else if resp.dragged() {
                            drag = Some((1, page, px, py));
                        }
                    }
                    if resp.clicked() {
                        clicked_link = hover_link.as_ref().map(|(t, _)| t.clone());
                    }
                }
            }
        });
        self.offset = out.state.offset;
        self.content = out.content_size;
        self.viewport = out.inner_rect.size();

        if let Some((t, rr)) = &hover_link {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
            ui.painter()
                .rect_stroke(*rr, 2, Stroke::new(1.0, pal.accent), egui::StrokeKind::Outside);
            let _ = t;
        } else if self.is_pdf && ui.rect_contains_pointer(out.inner_rect) {
            ctx.set_cursor_icon(CursorIcon::Text);
        }
        if let Some(t) = clicked_link {
            match t {
                LinkTarget::Uri(u) => events.push(PageEvent::OpenUrl(u)),
                LinkTarget::Page(p) => self.go_to(p, 0.0),
            }
        }
        if let Some((kind, page, x, y)) = drag {
            match (kind, self.sel.as_mut()) {
                (0, _) => {
                    self.sel = Some(Sel {
                        anchor: (page, x, y),
                        head: (page, x, y),
                        parts: BTreeMap::new(),
                        waiting: Vec::new(),
                        dirty: false,
                    })
                }
                (_, Some(s)) => {
                    if (s.head.0, s.head.1, s.head.2) != (page, x, y) {
                        s.head = (page, x, y);
                        s.dirty = true;
                    }
                }
                _ => {}
            }
        }
        if self.sel.as_ref().is_some_and(|s| !s.waiting.is_empty() || s.dirty) {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        if self.is_searching() || !self.pending.is_empty() || !self.links_wait.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        // which page is current, and where we are
        let y_mid = self.offset.y + self.viewport.y * 0.3;
        self.current_page = self.page_at_y(&tops, y_mid).min(sizes.len().saturating_sub(1));
        let ph = (sizes[self.current_page].1 * scale).max(1.0);
        self.top = Position {
            index: self.current_page,
            frac: ((y_mid - tops[self.current_page]) / ph).clamp(0.0, 1.0),
        };

        // forget bitmaps far from the view
        if self.cache.len() > MAX_CACHED {
            let keep: HashSet<usize> = visible.iter().copied().collect();
            let mut far: Vec<usize> = self.cache.keys().copied().filter(|p| !keep.contains(p)).collect();
            far.sort_by_key(|p| std::cmp::Reverse(p.abs_diff(self.current_page)));
            for p in far.into_iter().take(self.cache.len() - MAX_CACHED) {
                self.cache.remove(&p);
            }
        }
    }

    /// Renders a page at the given width and waits for it (for "save as picture"); paper tone, never dark.
    pub fn render_now(&self, page: usize, width: usize) -> Option<Rendered> {
        let (tx, rx) = channel();
        let done = move |r: Option<Rendered>| {
            let _ = tx.send(r);
        };
        match &self.src {
            Source::Pdf { engine, doc } => engine.render(*doc, page, width, Tone::Paper, done),
            Source::Images { tx } => {
                let _ = tx.send(ImgReq {
                    index: page,
                    width,
                    done: Box::new(done),
                });
            }
        }
        rx.recv_timeout(std::time::Duration::from_secs(20)).ok().flatten()
    }

    pub fn page_text(&self, page: usize) -> String {
        match &self.src {
            Source::Pdf { engine, doc } => engine.page_text(*doc, page),
            Source::Images { .. } => String::new(),
        }
    }
}

impl Drop for PagedView {
    fn drop(&mut self) {
        if let Source::Pdf { engine, doc } = &self.src {
            engine.close(*doc);
        }
    }
}

#[allow(dead_code)]
fn _types(_: pdf::Tone) {}
