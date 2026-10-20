//! Spreadsheets and CSV: a virtual grid with column letters, row numbers, a frozen header row and search.

use super::theme::Palette;
use eframe::egui::{self, Align, Color32, Label, RichText, Sense, Stroke, Ui, vec2};
use egui_extras::{Column, TableBuilder};
use folio_core::Sheet;
use folio_core::sheet::column_name;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

const ROW_H: f32 = 24.0;

#[derive(Default)]
pub struct SheetSearch {
    pub query: String,
    pub hits: Vec<(usize, usize, usize)>,
    pub current: usize,
    pending: Option<Receiver<(u64, Vec<(usize, usize, usize)>)>>,
    generation: u64,
}

pub struct SheetView {
    pub sheets: Arc<Vec<Sheet>>,
    pub active: usize,
    widths: Vec<Vec<f32>>,
    pub header_row: Vec<bool>,
    pub selected: Option<(usize, usize)>,
    pub search: SheetSearch,
    scroll_to: Option<usize>,
    pub top_row: usize,
    pub scroll_id: u64,
}

fn looks_like_number(s: &str) -> bool {
    let t = s.trim().trim_end_matches('%');
    !t.is_empty() && t.replace([' ', '\u{a0}', ','], "").parse::<f64>().is_ok()
}

fn guess_header(sheet: &Sheet) -> bool {
    let (Some(first), Some(second)) = (sheet.rows.first(), sheet.rows.get(1)) else {
        return false;
    };
    let filled = first.iter().filter(|c| !c.is_empty()).count();
    let texts = first.iter().filter(|c| !c.is_empty() && !looks_like_number(c)).count();
    filled > 0 && texts == filled && second.iter().any(|c| looks_like_number(c))
}

fn column_widths(sheet: &Sheet) -> Vec<f32> {
    let mut w = vec![56.0f32; sheet.cols.max(1)];
    for row in sheet.rows.iter().take(300) {
        for (c, text) in row.iter().enumerate() {
            let chars = text.lines().next().map_or(0, |l| l.chars().count()) as f32;
            w[c] = w[c].max((chars * 7.4 + 22.0).min(340.0));
        }
    }
    w
}

impl SheetView {
    pub fn new(sheets: Arc<Vec<Sheet>>, scroll_id: u64) -> SheetView {
        let widths = sheets.iter().map(column_widths).collect();
        let header_row = sheets.iter().map(guess_header).collect();
        SheetView {
            sheets,
            active: 0,
            widths,
            header_row,
            selected: None,
            search: SheetSearch::default(),
            scroll_to: None,
            top_row: 0,
            scroll_id,
        }
    }

    pub fn sheet(&self) -> &Sheet {
        &self.sheets[self.active.min(self.sheets.len() - 1)]
    }

    pub fn goto_row(&mut self, row: usize) {
        self.scroll_to = Some(row);
    }

    pub fn start_search(&mut self, query: &str) {
        self.search.generation += 1;
        self.search.query = query.to_owned();
        self.search.hits.clear();
        self.search.current = 0;
        self.search.pending = None;
        if query.trim().is_empty() {
            return;
        }
        let (tx, rx) = channel();
        let gen_id = self.search.generation;
        let sheets = self.sheets.clone();
        let q = super::textlayout::query_chars(query);
        std::thread::spawn(move || {
            let mut hits = Vec::new();
            'outer: for (si, s) in sheets.iter().enumerate() {
                for (ri, row) in s.rows.iter().enumerate() {
                    for (ci, cell) in row.iter().enumerate() {
                        let chars: Vec<char> = cell.chars().collect();
                        if !super::textlayout::find_all(&chars, &q).is_empty() {
                            hits.push((si, ri, ci));
                            if hits.len() > 20_000 {
                                break 'outer;
                            }
                        }
                    }
                }
            }
            let _ = tx.send((gen_id, hits));
        });
        self.search.pending = Some(rx);
    }

    pub fn poll_search(&mut self) -> bool {
        let Some(rx) = &self.search.pending else { return false };
        match rx.try_recv() {
            Ok((id, hits)) if id == self.search.generation => {
                self.search.hits = hits;
                self.search.pending = None;
                self.search.current = self
                    .search
                    .hits
                    .iter()
                    .position(|h| h.0 == self.active && h.1 >= self.top_row)
                    .or_else(|| self.search.hits.iter().position(|h| h.0 == self.active))
                    .unwrap_or(0);
                self.goto_current();
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

    pub fn is_searching(&self) -> bool {
        self.search.pending.is_some()
    }

    pub fn goto_current(&mut self) {
        if let Some(&(s, r, c)) = self.search.hits.get(self.search.current) {
            self.active = s;
            self.selected = Some((r, c));
            self.scroll_to = Some(r);
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
        self.goto_current();
    }

    pub fn selected_text(&self) -> Option<String> {
        let (r, c) = self.selected?;
        self.sheet().rows.get(r)?.get(c).cloned()
    }

    /// Tab-separated text of the whole active sheet.
    pub fn sheet_tsv(&self) -> String {
        self.sheet()
            .rows
            .iter()
            .map(|r| r.join("\t"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn show(&mut self, ui: &mut Ui, pal: &Palette) {
        // sheet tabs
        if self.sheets.len() > 1 {
            egui::Panel::bottom(egui::Id::new(("sheet_tabs", self.scroll_id)))
                .exact_size(34.0)
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        egui::ScrollArea::horizontal().show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (i, s) in self.sheets.iter().enumerate() {
                                    let sel = i == self.active;
                                    let label = RichText::new(&s.name).color(if sel { pal.accent } else { pal.text });
                                    if ui.selectable_label(sel, label).clicked() {
                                        self.active = i;
                                        self.selected = None;
                                        self.scroll_to = Some(0);
                                    }
                                }
                            });
                        });
                    });
                });
        }
        let si = self.active.min(self.sheets.len() - 1);
        let sheets = self.sheets.clone();
        let sheet = &sheets[si];
        let widths = self.widths[si].clone();
        let header = self.header_row[si];
        let hits: HashSet<(usize, usize)> = self
            .search
            .hits
            .iter()
            .filter(|h| h.0 == si)
            .map(|h| (h.1, h.2))
            .collect();
        let current = self
            .search
            .hits
            .get(self.search.current)
            .filter(|h| h.0 == si)
            .map(|h| (h.1, h.2));
        let first_data = usize::from(header);
        let body_rows = sheet.rows.len().saturating_sub(first_data);
        let mut clicked: Option<(usize, usize)> = None;
        let mut top_row = self.top_row;

        egui::Frame::new().fill(pal.paper).show(ui, |ui| {
            let mut table = TableBuilder::new(ui)
                .id_salt(("sheet", self.scroll_id, si))
                .striped(true)
                .resizable(true)
                .vscroll(true)
                .min_scrolled_height(0.0)
                .max_scroll_height(f32::INFINITY)
                .sense(Sense::click())
                .cell_layout(egui::Layout::left_to_right(Align::Center))
                .column(Column::exact(54.0));
            for w in &widths {
                table = table.column(Column::initial(*w).at_least(40.0).clip(true).resizable(true));
            }
            table = table.column(Column::remainder().clip(true));
            if let Some(row) = self.scroll_to.take() {
                table = table.scroll_to_row(row.saturating_sub(first_data), Some(Align::Center));
            }
            let head_h = if header { 44.0 } else { 24.0 };
            table
                .header(head_h, |mut h| {
                    h.col(|ui| {
                        ui.painter().rect_filled(ui.max_rect(), 0, pal.card);
                    });
                    for c in 0..widths.len() {
                        h.col(|ui| {
                            ui.painter().rect_filled(ui.max_rect(), 0, pal.card);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.label(RichText::new(column_name(c)).small().color(pal.dim));
                                if header {
                                    let t = sheet
                                        .rows
                                        .first()
                                        .and_then(|r| r.get(c))
                                        .map(String::as_str)
                                        .unwrap_or("");
                                    ui.add(Label::new(RichText::new(t).strong().color(pal.text)).truncate());
                                }
                            });
                        });
                    }
                    h.col(|ui| {
                        ui.painter().rect_filled(ui.max_rect(), 0, pal.card);
                    });
                })
                .body(|body| {
                    body.rows(ROW_H, body_rows, |mut row| {
                        let r = row.index() + first_data;
                        if row.index() == 0 {
                            top_row = r;
                        }
                        row.col(|ui| {
                            ui.add(Label::new(RichText::new((r + 1).to_string()).small().color(pal.dim)));
                        });
                        for c in 0..widths.len() {
                            let text = sheet
                                .rows
                                .get(r)
                                .and_then(|x| x.get(c))
                                .map(String::as_str)
                                .unwrap_or("");
                            let is_cur = current == Some((r, c));
                            let is_hit = hits.contains(&(r, c));
                            let is_sel = self.selected == Some((r, c));
                            let (_, resp) = row.col(|ui| {
                                let rect = ui.max_rect();
                                if is_cur {
                                    ui.painter().rect_filled(rect, 0, pal.hit_current);
                                } else if is_hit {
                                    ui.painter().rect_filled(rect, 0, pal.hit);
                                }
                                if is_sel {
                                    ui.painter().rect_stroke(
                                        rect.shrink(0.5),
                                        0,
                                        Stroke::new(1.5, pal.accent),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                let num = looks_like_number(text);
                                let rich = RichText::new(text.lines().next().unwrap_or("")).color(pal.text);
                                if num {
                                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                        ui.add(Label::new(rich).truncate());
                                    });
                                } else {
                                    ui.add(Label::new(rich).truncate());
                                }
                            });
                            if resp.clicked() {
                                clicked = Some((r, c));
                            }
                            if resp.hovered() && text.chars().count() > 40 {
                                resp.on_hover_ui(|ui| {
                                    ui.set_max_width(480.0);
                                    ui.label(text);
                                });
                            }
                        }
                        row.col(|_| {});
                    });
                });
        });
        if let Some(c) = clicked {
            self.selected = Some(c);
        }
        self.top_row = top_row;
        let _ = (Color32::WHITE, vec2(0.0, 0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(rows: &[&[&str]]) -> Sheet {
        let rows: Vec<Vec<String>> = rows
            .iter()
            .map(|r| r.iter().map(|s| (*s).to_owned()).collect())
            .collect();
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        Sheet {
            name: "S".into(),
            rows,
            cols,
            truncated: false,
        }
    }

    #[test]
    fn header_row_is_guessed_from_text_above_numbers() {
        assert!(guess_header(&sheet(&[&["Имя", "Сумма"], &["Аня", "12,5"]])));
        assert!(!guess_header(&sheet(&[&["1", "2"], &["3", "4"]])));
        assert!(!guess_header(&sheet(&[&["a", "b"], &["c", "d"]])));
    }

    #[test]
    fn numbers_are_recognised() {
        assert!(looks_like_number("1 234,5"));
        assert!(looks_like_number("12%"));
        assert!(!looks_like_number("12 кг"));
        assert!(!looks_like_number(""));
    }

    #[test]
    fn widths_follow_the_longest_cell() {
        let w = column_widths(&sheet(&[&["a", "очень длинное содержимое ячейки"], &["bb", "x"]]));
        assert!(w[1] > w[0]);
        assert!(w[0] >= 56.0 && w[1] <= 340.0);
    }
}
