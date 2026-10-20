//! Reflowable documents (docx, epub, fb2, markdown, html, text, code ...): the block list is laid out lazily, only
//! the blocks near the viewport are drawn, the rest is a measured or estimated gap.

use super::textlayout::{self, Laid, Look, Marks};
use super::theme::Palette;
use crate::library::Position;
use eframe::egui::{
    self, Align2, Color32, ColorImage, Frame, Label, Margin, Rect, RichText, ScrollArea, Sense, Shape, Stroke,
    TextureHandle, TextureOptions, Ui, pos2, vec2,
};
use folio_core::{Block, Flow};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

const HEADING_SCALE: [f32; 6] = [1.85, 1.55, 1.32, 1.18, 1.08, 1.0];
const TOP_PAD: f32 = 40.0;
const BOTTOM_PAD: f32 = 120.0;
const LOOKAHEAD: f32 = 700.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jump {
    pub block: usize,
    pub frac: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct FlowHit {
    pub block: usize,
    pub occurrence: usize,
    /// Where in the block's text the match starts, 0..1.
    pub frac: f32,
}

pub enum FlowEvent {
    OpenUrl(String),
}

#[derive(Default)]
pub struct FlowSearch {
    pub query: String,
    pub hits: Vec<FlowHit>,
    pub current: usize,
    blocks: HashSet<usize>,
    pending: Option<Receiver<(u64, Vec<FlowHit>)>>,
    generation: u64,
}

impl FlowSearch {
    pub fn is_running(&self) -> bool {
        self.pending.is_some()
    }
}

pub struct FlowView {
    pub flow: Arc<Flow>,
    heights: Vec<f32>,
    layout_key: u64,
    images: HashMap<usize, Option<TextureHandle>>,
    codes: HashMap<usize, (u64, Arc<egui::text::LayoutJob>)>,
    pub offset: f32,
    pub content_h: f32,
    pub viewport_h: f32,
    pending_offset: Option<f32>,
    pub jump: Option<(Jump, u8)>,
    pub top: Position,
    pub search: FlowSearch,
    pub speaking: Option<usize>,
    pub scroll_id: u64,
}

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

impl FlowView {
    pub fn new(flow: Arc<Flow>, scroll_id: u64) -> FlowView {
        let n = flow.blocks.len();
        FlowView {
            flow,
            heights: vec![0.0; n],
            layout_key: 0,
            images: HashMap::new(),
            codes: HashMap::new(),
            offset: 0.0,
            content_h: 0.0,
            viewport_h: 0.0,
            pending_offset: None,
            jump: None,
            top: Position::default(),
            search: FlowSearch::default(),
            speaking: None,
            scroll_id,
        }
    }

    pub fn progress(&self) -> f32 {
        let range = (self.content_h - self.viewport_h).max(1.0);
        (self.offset / range).clamp(0.0, 1.0)
    }

    pub fn goto(&mut self, block: usize, frac: f32) {
        self.jump = Some((
            Jump {
                block: block.min(self.flow.blocks.len().saturating_sub(1)),
                frac,
            },
            6,
        ));
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.pending_offset = Some((self.offset + dy).max(0.0));
    }

    pub fn scroll_to_fraction(&mut self, f: f32) {
        self.pending_offset = Some(f.clamp(0.0, 1.0) * (self.content_h - self.viewport_h).max(0.0));
    }

    // ---- search ----

    pub fn start_search(&mut self, query: &str) {
        self.search.generation += 1;
        self.search.query = query.to_owned();
        self.search.hits.clear();
        self.search.blocks.clear();
        self.search.current = 0;
        self.search.pending = None;
        if query.trim().is_empty() {
            return;
        }
        let (tx, rx) = channel();
        let gen_id = self.search.generation;
        let flow = self.flow.clone();
        let q = textlayout::query_chars(query);
        std::thread::spawn(move || {
            let mut hits = Vec::new();
            for (i, b) in flow.blocks.iter().enumerate() {
                let text: Vec<char> = b.plain_text().chars().collect();
                for (occurrence, at) in textlayout::find_all(&text, &q).into_iter().enumerate() {
                    hits.push(FlowHit {
                        block: i,
                        occurrence,
                        frac: at as f32 / text.len().max(1) as f32,
                    });
                    if hits.len() > 20_000 {
                        let _ = tx.send((gen_id, hits));
                        return;
                    }
                }
            }
            let _ = tx.send((gen_id, hits));
        });
        self.search.pending = Some(rx);
    }

    /// Takes a finished search; returns true when the hit list changed.
    pub fn poll_search(&mut self) -> bool {
        let Some(rx) = &self.search.pending else { return false };
        match rx.try_recv() {
            Ok((id, hits)) if id == self.search.generation => {
                self.search.blocks = hits.iter().map(|h| h.block).collect();
                self.search.hits = hits;
                self.search.pending = None;
                // start from the first hit at or after the current view
                let from = self.top.index;
                self.search.current = self.search.hits.iter().position(|h| h.block >= from).unwrap_or(0);
                self.goto_current_hit();
                true
            }
            Ok(_) => false,
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(_) => {
                self.search.pending = None;
                false
            }
        }
    }

    pub fn goto_current_hit(&mut self) {
        if let Some(h) = self.search.hits.get(self.search.current).copied() {
            self.goto(h.block, (h.frac - 0.02).max(0.0));
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
        self.goto_current_hit();
    }

    // ---- drawing ----

    fn estimate(&self, i: usize, col_w: f32, look: &Look) -> f32 {
        let size = look.size;
        let lines = |chars: usize, width: f32, scale: f32| {
            ((chars as f32 * 0.52 * size * scale / width.max(80.0)).ceil()).max(1.0)
        };
        match &self.flow.blocks[i] {
            Block::Heading { level, text } => {
                let s = HEADING_SCALE[(*level as usize - 1).min(5)];
                lines(text.text().chars().count(), col_w, s) * size * s * look.spacing + size * (s * 0.9 + 0.4)
            }
            Block::Paragraph { text, .. } => {
                lines(text.text().chars().count(), col_w, 1.0) * size * look.spacing + size * 0.6
            }
            Block::ListItem { text, depth, .. } => {
                lines(text.text().chars().count(), col_w - 40.0 - *depth as f32 * 24.0, 1.0) * size * look.spacing
                    + size * 0.35
            }
            Block::Quote(t) => lines(t.text().chars().count(), col_w - 30.0, 1.0) * size * look.spacing + size,
            Block::Code { text, .. } => {
                text.lines().count().max(1) as f32 * size * 0.88 * look.spacing.min(1.35) + size * 1.4
            }
            Block::Table(t) => t.rows.len() as f32 * 38.0 + size,
            Block::Image { .. } => 280.0,
            Block::Rule => 28.0,
            Block::PageBreak => 70.0,
        }
    }

    fn prefix(&self, col_w: f32, look: &Look) -> Vec<f32> {
        let mut y = Vec::with_capacity(self.flow.blocks.len() + 1);
        let mut acc = TOP_PAD;
        for i in 0..self.flow.blocks.len() {
            y.push(acc);
            acc += if self.heights[i] > 0.0 {
                self.heights[i]
            } else {
                self.estimate(i, col_w, look)
            };
        }
        y.push(acc);
        y
    }

    fn texture(&mut self, ctx: &egui::Context, id: usize) -> Option<TextureHandle> {
        if let Some(t) = self.images.get(&id) {
            return t.clone();
        }
        let tex = self.flow.images.get(id).and_then(|bytes| {
            let img = image::load_from_memory(bytes).ok()?;
            let img = if img.width() > 1800 || img.height() > 2400 {
                img.resize(1800, 2400, image::imageops::FilterType::Triangle)
            } else {
                img
            };
            let rgba = img.to_rgba8();
            let color =
                ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
            Some(ctx.load_texture(
                format!("flow-img-{}-{id}", self.scroll_id),
                color,
                TextureOptions::LINEAR,
            ))
        });
        self.images.insert(id, tex.clone());
        tex
    }

    fn code_job(&mut self, i: usize, look: &Look, width: f32) -> Arc<egui::text::LayoutJob> {
        let key = hash_of(&(
            look.size.to_bits(),
            look.pal.dark,
            look.pal.paper.to_array(),
            width.round() as i32,
            look.spacing.to_bits(),
        ));
        if let Some((k, job)) = self.codes.get(&i)
            && *k == key
        {
            return job.clone();
        }
        let Block::Code { text, lang, line } = &self.flow.blocks[i] else {
            return Arc::new(Default::default());
        };
        let job = Arc::new(textlayout::code_job(text, lang.as_deref(), *line, look, width));
        if self.codes.len() > 64 {
            self.codes.clear();
        }
        self.codes.insert(i, (key, job.clone()));
        job
    }

    /// Draws the document in the space of `ui`. `column` is the preferred text width.
    pub fn show(&mut self, ui: &mut Ui, look: &Look, column: f32, events: &mut Vec<FlowEvent>) {
        let avail = ui.available_width();
        let col_w = column.min(avail - 56.0).max(260.0);
        let side = ((avail - col_w) / 2.0).max(0.0);
        let key = hash_of(&(
            col_w.round() as i32,
            look.size.to_bits(),
            look.spacing.to_bits(),
            look.justify,
            look.base,
        ));
        if key != self.layout_key {
            // keep the place in the text when the layout changes
            let keep = self.top;
            self.heights.fill(0.0);
            self.layout_key = key;
            if self.offset > 1.0 {
                self.jump = Some((
                    Jump {
                        block: keep.index,
                        frac: keep.frac,
                    },
                    6,
                ));
            }
        }
        let prefix = self.prefix(col_w, look);

        if let Some((j, frames)) = self.jump {
            let b = j.block.min(self.flow.blocks.len().saturating_sub(1));
            let h = prefix[b + 1] - prefix[b];
            let target = (prefix[b] + h * j.frac - 28.0).max(0.0);
            if (target - self.offset).abs() > 1.0 || frames > 3 {
                self.pending_offset = Some(target);
            }
            self.jump = if frames <= 1 { None } else { Some((j, frames - 1)) };
            ui.ctx().request_repaint();
        }

        let mut area = ScrollArea::vertical()
            .id_salt(("flow", self.scroll_id))
            .auto_shrink([false, false])
            .scroll_source(egui::containers::scroll_area::ScrollSource::ALL);
        if let Some(y) = self.pending_offset.take() {
            area = area.vertical_scroll_offset(y);
            ui.ctx().request_repaint();
        }
        let mut shift = 0.0f32;
        let mut measured: Vec<(usize, f32)> = Vec::new();
        let mut top = self.top;
        let out = area.show_viewport(ui, |ui, viewport| {
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(col_w);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.add_space(TOP_PAD);
                    let lo = viewport.min.y - LOOKAHEAD;
                    let hi = viewport.max.y + LOOKAHEAD;
                    for i in 0..self.flow.blocks.len() {
                        let est = prefix[i + 1] - prefix[i];
                        let y = prefix[i];
                        if y <= viewport.min.y && viewport.min.y < y + est {
                            top = Position {
                                index: i,
                                frac: ((viewport.min.y - y) / est.max(1.0)).clamp(0.0, 1.0),
                            };
                        }
                        if y + est < lo || y > hi {
                            ui.add_space(est);
                            continue;
                        }
                        let before = ui.cursor().min.y;
                        self.draw_block(ui, i, look, col_w, events);
                        let h = ui.cursor().min.y - before;
                        measured.push((i, h));
                        if y + est <= viewport.min.y && (h - est).abs() > 0.5 {
                            shift += h - est;
                        }
                    }
                    ui.add_space(BOTTOM_PAD);
                });
            });
        });
        for (i, h) in measured {
            self.heights[i] = h.max(1.0);
        }
        self.top = top;
        self.offset = out.state.offset.y;
        self.content_h = out.content_size.y;
        self.viewport_h = out.inner_rect.height();
        if shift.abs() > 0.5 {
            self.pending_offset = Some((self.offset + shift).max(0.0));
            ui.ctx().request_repaint();
        }
    }

    fn marks_for<'a>(&self, i: usize, query: &'a [char]) -> Option<Marks<'a>> {
        if query.is_empty() || !self.search.blocks.contains(&i) {
            return None;
        }
        let current = self
            .search
            .hits
            .get(self.search.current)
            .filter(|h| h.block == i)
            .map(|h| h.occurrence);
        Some(Marks { query, current })
    }

    fn label(&self, ui: &mut Ui, laid: &Laid, events: &mut Vec<FlowEvent>) -> egui::Response {
        let resp = ui.add(Label::new(laid.job.clone()).selectable(true).sense(Sense::click()));
        if !laid.links.is_empty()
            && let Some(p) = resp.hover_pos()
        {
            let galley = ui.painter().layout_job(laid.job.clone());
            let idx = galley.cursor_from_pos(p - resp.rect.min).index.0;
            if let Some(url) = textlayout::link_at(laid, idx) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    events.push(FlowEvent::OpenUrl(url.to_string()));
                }
            }
        }
        resp
    }

    fn draw_block(&mut self, ui: &mut Ui, i: usize, look: &Look, col_w: f32, events: &mut Vec<FlowEvent>) {
        let pal = look.pal;
        let size = look.size;
        let query = textlayout::query_chars(&self.search.query);
        let marks = self.marks_for(i, &query);
        let highlight = self.speaking == Some(i);
        let shape_slot = ui.painter().add(Shape::Noop);
        let top_y = ui.cursor().min.y;
        let flow = self.flow.clone();
        let block = &flow.blocks[i];
        match block {
            Block::Heading { level, text } => {
                let s = HEADING_SCALE[(*level as usize - 1).min(5)];
                ui.add_space(size * (s * 0.9));
                let laid = textlayout::rich_job(
                    text,
                    look,
                    s,
                    true,
                    None,
                    marks.as_ref(),
                    col_w,
                    folio_core::Align::Left,
                );
                self.label(ui, &laid, events);
                if *level <= 2 {
                    ui.add_space(size * 0.2);
                    let (r, _) = ui.allocate_exact_size(vec2(col_w, 1.0), Sense::hover());
                    ui.painter().rect_filled(r, 0, pal.ink_dim.gamma_multiply(0.35));
                }
                ui.add_space(size * 0.4);
            }
            Block::Paragraph { text, align } => {
                let laid = textlayout::rich_job(text, look, 1.0, false, None, marks.as_ref(), col_w, *align);
                self.label(ui, &laid, events);
                ui.add_space(size * 0.6);
            }
            Block::ListItem { depth, marker, text } => {
                let indent = 8.0 + *depth as f32 * 26.0;
                let marker_w = 30.0;
                let text_w = col_w - indent - marker_w;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.add_space(indent);
                    let (r, _) = ui.allocate_exact_size(vec2(marker_w, size * look.spacing), Sense::hover());
                    ui.painter().text(
                        pos2(r.right() - 8.0, r.top() + size * look.spacing * 0.5),
                        Align2::RIGHT_CENTER,
                        marker,
                        egui::FontId::new(size, super::theme::family(look.base, false, false)),
                        pal.accent,
                    );
                    ui.vertical(|ui| {
                        ui.set_width(text_w);
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        let laid = textlayout::rich_job(
                            text,
                            look,
                            1.0,
                            false,
                            None,
                            marks.as_ref(),
                            text_w,
                            folio_core::Align::Left,
                        );
                        self.label(ui, &laid, events);
                    });
                });
                ui.add_space(size * 0.3);
            }
            Block::Quote(text) => {
                ui.add_space(size * 0.2);
                let start = ui.cursor().min;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.add_space(18.0);
                    ui.vertical(|ui| {
                        ui.set_width(col_w - 22.0);
                        let laid = textlayout::rich_job(
                            text,
                            look,
                            1.0,
                            false,
                            Some(pal.ink_dim),
                            marks.as_ref(),
                            col_w - 22.0,
                            folio_core::Align::Left,
                        );
                        self.label(ui, &laid, events);
                    });
                });
                let end = ui.cursor().min.y;
                ui.painter().rect_filled(
                    Rect::from_min_max(pos2(start.x + 4.0, start.y), pos2(start.x + 7.0, end)),
                    2,
                    pal.quote_bar,
                );
                ui.add_space(size * 0.7);
            }
            Block::Code { .. } => {
                let inner_w = col_w - 24.0;
                let job = self.code_job(i, look, inner_w);
                ui.add_space(size * 0.2);
                Frame::new()
                    .fill(pal.code_bg)
                    .stroke(Stroke::new(1.0, pal.border.gamma_multiply(0.7)))
                    .corner_radius(8)
                    .inner_margin(Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_width(inner_w);
                        let mut job = (*job).clone();
                        // search marks inside code: a plain overlay on the matching text sections is not worth the
                        // re-layout cost; the block is simply flagged by the sidebar and the scrollbar jump
                        job.wrap.max_width = inner_w;
                        ui.add(Label::new(job).selectable(true));
                    });
                ui.add_space(size * 0.7);
            }
            Block::Table(t) => {
                ui.add_space(size * 0.2);
                self.draw_table(ui, t, look, col_w, marks.as_ref(), events);
                ui.add_space(size * 0.8);
            }
            Block::Image { id, alt } => {
                ui.add_space(size * 0.3);
                match self.texture(ui.ctx(), *id) {
                    Some(tex) => {
                        let natural = tex.size_vec2();
                        let max_h = (ui.ctx().content_rect().height() * 0.85).max(240.0);
                        let scale = (col_w / natural.x).min(max_h / natural.y).clamp(0.05, 1.0);
                        let sz = natural * scale;
                        ui.vertical_centered(|ui| {
                            ui.image((tex.id(), sz));
                            if !alt.is_empty() {
                                ui.label(RichText::new(alt).color(pal.ink_dim).size(size * 0.8).italics());
                            }
                        });
                    }
                    None => {
                        ui.label(
                            RichText::new(format!("[{}]", if alt.is_empty() { "image" } else { alt }))
                                .color(pal.ink_dim),
                        );
                    }
                }
                ui.add_space(size * 0.6);
            }
            Block::Rule => {
                ui.add_space(size * 0.6);
                let (r, _) = ui.allocate_exact_size(vec2(col_w, 1.0), Sense::hover());
                ui.painter().rect_filled(r, 0, pal.ink_dim.gamma_multiply(0.5));
                ui.add_space(size * 0.9);
            }
            Block::PageBreak => {
                ui.add_space(size * 1.2);
                let (r, _) = ui.allocate_exact_size(vec2(col_w, size * 1.2), Sense::hover());
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    "•  •  •",
                    egui::FontId::new(size * 0.9, super::theme::family(look.base, false, false)),
                    pal.ink_dim.gamma_multiply(0.7),
                );
                ui.add_space(size * 1.2);
            }
        }
        if highlight {
            let bottom = ui.cursor().min.y;
            let r = Rect::from_min_max(
                pos2(ui.min_rect().left() - 10.0, top_y),
                pos2(ui.min_rect().left() + col_w + 10.0, bottom),
            );
            ui.painter()
                .set(shape_slot, Shape::rect_filled(r, 6, pal.accent.gamma_multiply(0.14)));
            ui.painter().rect_filled(
                Rect::from_min_max(pos2(r.left() - 2.0, r.top()), pos2(r.left() + 1.0, r.bottom())),
                2,
                pal.accent,
            );
        }
    }

    fn draw_table(
        &mut self,
        ui: &mut Ui,
        t: &folio_core::Table,
        look: &Look,
        col_w: f32,
        marks: Option<&Marks>,
        events: &mut Vec<FlowEvent>,
    ) {
        let pal = look.pal;
        let cols = t
            .rows
            .iter()
            .map(|r| r.iter().map(|c| c.colspan.min(64)).sum::<usize>())
            .max()
            .unwrap_or(1)
            .clamp(1, 256);
        // column widths follow the longest text of the column, within sane bounds
        let mut want = vec![60.0f32; cols];
        for r in &t.rows {
            let mut c = 0;
            for cell in r {
                if cell.colspan == 1 && c < cols {
                    let longest = cell.text.text().lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
                    want[c] = want[c].max((longest * look.size * 0.5 + 24.0).min(420.0));
                }
                c += cell.colspan;
            }
        }
        let total: f32 = want.iter().sum();
        let scale = if total > col_w { col_w / total } else { 1.0 };
        let widths: Vec<f32> = want.iter().map(|w| (w * scale).max(40.0)).collect();
        let origin = ui.cursor().min;
        let mut y = origin.y;
        let mut cell_rects: Vec<Rect> = Vec::new();
        let mut row_bounds: Vec<(f32, f32)> = Vec::new();
        for (ri, row) in t.rows.iter().enumerate() {
            let mut x = origin.x;
            let mut col = 0;
            let mut row_h = 0.0f32;
            let mut placed: Vec<(Rect, f32)> = Vec::new();
            for cell in row {
                let w: f32 = widths[col.min(cols - 1)..(col + cell.colspan).min(cols)].iter().sum();
                let rect = Rect::from_min_size(pos2(x, y), vec2(w, 10.0));
                let bold = ri < t.header_rows;
                let laid = textlayout::rich_job(
                    &cell.text,
                    look,
                    0.92,
                    bold,
                    None,
                    marks,
                    w - 16.0,
                    folio_core::Align::Left,
                );
                let inner = Rect::from_min_size(rect.min + vec2(8.0, 6.0), vec2(w - 16.0, 0.0));
                let resp = ui.put(inner, Label::new(laid.job.clone()).selectable(true));
                let _ = (&events, &laid.links);
                let h = resp.rect.height() + 12.0;
                row_h = row_h.max(h);
                placed.push((Rect::from_min_size(pos2(x, y), vec2(w, 0.0)), h));
                x += w;
                col += cell.colspan;
            }
            for (r, _) in &placed {
                cell_rects.push(Rect::from_min_size(r.min, vec2(r.width(), row_h)));
            }
            row_bounds.push((y, y + row_h));
            y += row_h;
        }
        let total_w: f32 = widths.iter().sum();
        let painter = ui.painter();
        for (ri, (a, b)) in row_bounds.iter().enumerate() {
            if ri < t.header_rows {
                painter.rect_filled(
                    Rect::from_min_max(pos2(origin.x, *a), pos2(origin.x + total_w, *b)),
                    0,
                    pal.accent.gamma_multiply(0.12),
                );
            } else if ri % 2 == 1 {
                painter.rect_filled(
                    Rect::from_min_max(pos2(origin.x, *a), pos2(origin.x + total_w, *b)),
                    0,
                    pal.ink.gamma_multiply(0.04),
                );
            }
        }
        let stroke = Stroke::new(1.0, pal.ink_dim.gamma_multiply(0.45));
        for r in &cell_rects {
            painter.rect_stroke(*r, 0, stroke, egui::StrokeKind::Inside);
        }
        ui.allocate_space(vec2(total_w, y - origin.y));
    }

    pub fn copy_text(&self) -> String {
        self.flow.plain_text()
    }
}

/// Cheap sanity color helper for tests and shots.
#[allow(dead_code)]
pub fn paper(p: &Palette) -> Color32 {
    p.paper
}
