//! The left panel: contents, page thumbnails, search results, bookmarks, document information.

use super::chrome::{ICON_DELETE, ICON_INFO, ICON_LIST, ICON_PAGES, ICON_SEARCH, ICON_STAR};
use super::doc::{State, View};
use super::theme::{self, Palette};
use super::{App, SideTab};
use crate::i18n::{duration_text, human_size, tr, trf};
use crate::library::Position;
use eframe::egui::{self, Align2, Color32, Frame, Margin, RichText, Sense, Stroke, Ui, pos2, vec2};

fn clip(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > n {
        format!("{}…", one.chars().take(n - 1).collect::<String>())
    } else {
        one
    }
}

fn row_button(ui: &mut Ui, pal: &Palette, height: f32, selected: bool) -> (egui::Rect, egui::Response) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 6, pal.accent.gamma_multiply(0.2));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 6, pal.card_hover);
    }
    (rect, resp)
}

fn empty_note(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(text).color(pal.dim));
    });
}

impl App {
    pub(super) fn sidebar(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let is_paged = matches!(self.tab().and_then(|t| t.view()), Some(View::Paged(_)));
        let has_search = self.find.open || self.find_status().1 > 0;
        if self.side_tab == SideTab::Pages && !is_paged {
            self.side_tab = SideTab::Contents;
        }
        egui::Panel::left("sidebar")
            .default_size(292.0)
            .size_range(220.0..=520.0)
            .resizable(true)
            .frame(Frame::new().fill(pal.panel).inner_margin(Margin::symmetric(8, 8)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let mut tabs = vec![(SideTab::Contents, ICON_LIST, tr("Содержание", "Contents"))];
                    if is_paged {
                        tabs.push((SideTab::Pages, ICON_PAGES, tr("Страницы", "Pages")));
                    }
                    tabs.push((SideTab::Search, ICON_SEARCH, tr("Результаты поиска", "Search results")));
                    tabs.push((SideTab::Bookmarks, ICON_STAR, tr("Закладки", "Bookmarks")));
                    tabs.push((SideTab::Info, ICON_INFO, tr("О документе", "Document info")));
                    for (t, icon, tip) in tabs {
                        if theme::icon_button(ui, &pal, icon, tip, self.side_tab == t).clicked() {
                            self.side_tab = t;
                            self.settings.sidebar_tab = match t {
                                SideTab::Contents => "contents",
                                SideTab::Pages => "pages",
                                SideTab::Search => "search",
                                SideTab::Bookmarks => "bookmarks",
                                SideTab::Info => "info",
                            }
                            .into();
                        }
                    }
                });
                ui.add_space(4.0);
                let _ = has_search;
                match self.side_tab {
                    SideTab::Contents => self.side_contents(ui),
                    SideTab::Pages => self.side_pages(ui),
                    SideTab::Search => self.side_search(ui),
                    SideTab::Bookmarks => self.side_bookmarks(ui),
                    SideTab::Info => self.side_info(ui),
                }
            });
    }

    fn side_contents(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let Some(tab) = self.tab() else { return };
        let Some(info) = tab.info.as_ref() else { return };
        if info.toc.is_empty() {
            empty_note(
                ui,
                &pal,
                tr("В документе нет оглавления", "This document has no contents list"),
            );
            return;
        }
        let here = tab.position().unwrap_or_default();
        let current = info
            .toc
            .iter()
            .rposition(|e| (e.pos.index, 0) <= (here.index, 0) && (e.pos.index < here.index || here.frac >= 0.0))
            .unwrap_or(0);
        let toc = info.toc.clone();
        let mut go: Option<Position> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 28.0, toc.len(), |ui, range| {
                for i in range {
                    let e = &toc[i];
                    let (rect, resp) = row_button(ui, &pal, 28.0, i == current);
                    let indent = (e.level.saturating_sub(1)) as f32 * 14.0;
                    let color = if i == current {
                        pal.accent
                    } else if e.level == 1 {
                        pal.text
                    } else {
                        pal.dim
                    };
                    ui.painter().text(
                        rect.left_center() + vec2(8.0 + indent, 0.0),
                        Align2::LEFT_CENTER,
                        clip(&e.title, 44 - (e.level as usize) * 2),
                        egui::FontId::proportional(if e.level == 1 { 14.0 } else { 13.0 }),
                        color,
                    );
                    if resp.clicked() {
                        go = Some(e.pos);
                    }
                    resp.on_hover_text(&e.title);
                }
            });
        if let (Some(pos), Some(tab)) = (go, self.tab_mut()) {
            tab.goto(pos);
        }
    }

    fn side_pages(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let tone = self.pdf_tone();
        let ctx = ui.ctx().clone();
        let mut go: Option<usize> = None;
        let Some(View::Paged(p)) = self.tab_mut().and_then(|t| t.view_mut()) else {
            return;
        };
        let n = p.page_count();
        let cur = p.current_page;
        let thumb_w = ((ui.available_width() - 28.0) / 2.0).clamp(90.0, 190.0);
        let aspect = p
            .sizes
            .iter()
            .take(5)
            .map(|s| s.1 / s.0.max(1.0))
            .fold(1.0f32, f32::max)
            .clamp(0.6, 1.9);
        let row_h = thumb_w * aspect + 30.0;
        let rows = n.div_ceil(2);
        let tone = if p.is_pdf { tone } else { folio_core::pdf::Tone::Paper };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, rows, |ui, range| {
                for r in range {
                    ui.horizontal(|ui| {
                        for page in [r * 2, r * 2 + 1] {
                            if page >= n {
                                break;
                            }
                            let (rect, resp) = ui.allocate_exact_size(vec2(thumb_w + 8.0, row_h - 4.0), Sense::click());
                            if page == cur {
                                ui.painter().rect_filled(rect, 8, pal.accent.gamma_multiply(0.25));
                            } else if resp.hovered() {
                                ui.painter().rect_filled(rect, 8, pal.card_hover);
                            }
                            let ph = thumb_w * (p.sizes[page].1 / p.sizes[page].0.max(1.0));
                            let img = egui::Rect::from_min_size(
                                rect.min + vec2(4.0, 4.0),
                                vec2(thumb_w, ph.min(row_h - 30.0)),
                            );
                            match p.thumbnail(&ctx, page, tone) {
                                Some(t) => {
                                    ui.painter().image(
                                        t.id(),
                                        img,
                                        egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                        Color32::WHITE,
                                    );
                                }
                                None => {
                                    ui.painter().rect_filled(img, 2, pal.card);
                                }
                            }
                            ui.painter()
                                .rect_stroke(img, 2, Stroke::new(1.0, pal.border), egui::StrokeKind::Outside);
                            ui.painter().text(
                                pos2(rect.center().x, rect.bottom() - 10.0),
                                Align2::CENTER_CENTER,
                                (page + 1).to_string(),
                                egui::FontId::proportional(12.5),
                                if page == cur { pal.accent } else { pal.dim },
                            );
                            if resp.clicked() {
                                go = Some(page);
                            }
                        }
                    });
                }
            });
        if let Some(pg) = go {
            p.go_to(pg, 0.0);
        }
    }

    fn side_search(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        if self.find.applied.is_empty() {
            empty_note(
                ui,
                &pal,
                tr("Нажмите Ctrl+F и введите слово", "Press Ctrl+F and type a word"),
            );
            return;
        }
        let mut pick: Option<usize> = None;
        let query = super::textlayout::query_chars(&self.find.applied);
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        match tab.view_mut() {
            Some(View::Flow(f)) => {
                let total = f.search.hits.len();
                if total == 0 {
                    empty_note(
                        ui,
                        &pal,
                        if f.search.is_running() {
                            tr("Ищу…", "Searching…")
                        } else {
                            tr("Ничего не найдено", "No matches")
                        },
                    );
                    return;
                }
                let cur = f.search.current;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
                    ui,
                    46.0,
                    total.min(3000),
                    |ui, range| {
                        for i in range {
                            let h = f.search.hits[i];
                            let (rect, resp) = row_button(ui, &pal, 46.0, i == cur);
                            let text: Vec<char> = f.flow.blocks[h.block].plain_text().chars().collect();
                            let at = (h.frac * text.len() as f32) as usize;
                            let from = at.saturating_sub(24);
                            let snippet: String =
                                text[from.min(text.len())..(from + 80).min(text.len())].iter().collect();
                            let snippet = clip(&snippet, 70);
                            ui.painter().text(
                                rect.left_top() + vec2(8.0, 6.0),
                                Align2::LEFT_TOP,
                                format!("{} · {}%", i + 1, (h.block * 100) / f.flow.blocks.len().max(1)),
                                egui::FontId::proportional(11.5),
                                pal.dim,
                            );
                            ui.painter().text(
                                rect.left_top() + vec2(8.0, 23.0),
                                Align2::LEFT_TOP,
                                snippet,
                                egui::FontId::proportional(13.0),
                                pal.text,
                            );
                            if resp.clicked() {
                                pick = Some(i);
                            }
                        }
                    },
                );
                if let Some(i) = pick {
                    f.search.current = i;
                    f.goto_current_hit();
                }
            }
            Some(View::Paged(p)) => {
                let total = p.search.hits.len();
                if total == 0 {
                    empty_note(
                        ui,
                        &pal,
                        if p.is_searching() {
                            tr("Ищу…", "Searching…")
                        } else if !p.is_pdf {
                            tr(
                                "Поиск работает в PDF и текстовых документах",
                                "Search works in PDF and text documents",
                            )
                        } else {
                            tr("Ничего не найдено", "No matches")
                        },
                    );
                    return;
                }
                let cur = p.search.current;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, 46.0, total, |ui, range| {
                        for i in range {
                            let h = &p.search.hits[i];
                            let (rect, resp) = row_button(ui, &pal, 46.0, i == cur);
                            ui.painter().text(
                                rect.left_top() + vec2(8.0, 6.0),
                                Align2::LEFT_TOP,
                                trf("Страница {0}", "Page {0}", &[&(h.page + 1)]),
                                egui::FontId::proportional(11.5),
                                pal.dim,
                            );
                            ui.painter().text(
                                rect.left_top() + vec2(8.0, 23.0),
                                Align2::LEFT_TOP,
                                clip(&h.snippet, 64),
                                egui::FontId::proportional(13.0),
                                pal.text,
                            );
                            if resp.clicked() {
                                pick = Some(i);
                            }
                        }
                    });
                if let Some(i) = pick {
                    p.search.current = i;
                    p.goto_hit();
                }
            }
            Some(View::Sheet(s)) => {
                let total = s.search.hits.len();
                if total == 0 {
                    empty_note(
                        ui,
                        &pal,
                        if s.is_searching() {
                            tr("Ищу…", "Searching…")
                        } else {
                            tr("Ничего не найдено", "No matches")
                        },
                    );
                    return;
                }
                let cur = s.search.current;
                let sheets = s.sheets.clone();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
                    ui,
                    40.0,
                    total.min(3000),
                    |ui, range| {
                        for i in range {
                            let (si, r, c) = s.search.hits[i];
                            let (rect, resp) = row_button(ui, &pal, 40.0, i == cur);
                            let cell = sheets[si]
                                .rows
                                .get(r)
                                .and_then(|x| x.get(c))
                                .cloned()
                                .unwrap_or_default();
                            ui.painter().text(
                                rect.left_top() + vec2(8.0, 5.0),
                                Align2::LEFT_TOP,
                                format!("{} · {}{}", sheets[si].name, folio_core::sheet::column_name(c), r + 1),
                                egui::FontId::proportional(11.5),
                                pal.dim,
                            );
                            ui.painter().text(
                                rect.left_top() + vec2(8.0, 21.0),
                                Align2::LEFT_TOP,
                                clip(&cell, 40),
                                egui::FontId::proportional(13.0),
                                pal.text,
                            );
                            if resp.clicked() {
                                pick = Some(i);
                            }
                        }
                    },
                );
                if let Some(i) = pick {
                    s.search.current = i;
                    s.goto_current();
                }
            }
            None => {}
        }
        let _ = query;
    }

    fn side_bookmarks(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let Some(tab) = self.tab() else { return };
        let key = tab.key.clone();
        let marks = self.library.get(&key).map(|e| e.bookmarks.clone()).unwrap_or_default();
        if marks.is_empty() {
            empty_note(
                ui,
                &pal,
                tr(
                    "Закладок пока нет — Ctrl+B ставит закладку",
                    "No bookmarks yet — Ctrl+B adds one",
                ),
            );
            return;
        }
        let mut go: Option<Position> = None;
        let mut remove: Option<usize> = None;
        let paged = matches!(tab.view(), Some(View::Paged(_)));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, b) in marks.iter().enumerate() {
                let (rect, resp) = row_button(ui, &pal, 44.0, false);
                let title = if paged {
                    trf("Страница {0}", "Page {0}", &[&(b.pos.index + 1)])
                } else {
                    clip(&b.label, 38)
                };
                ui.painter().text(
                    rect.left_top() + vec2(10.0, 6.0),
                    Align2::LEFT_TOP,
                    title,
                    egui::FontId::proportional(14.0),
                    pal.text,
                );
                ui.painter().text(
                    rect.left_top() + vec2(10.0, 25.0),
                    Align2::LEFT_TOP,
                    crate::timeutil::date_text(b.created),
                    egui::FontId::proportional(11.5),
                    pal.dim,
                );
                let x = egui::Rect::from_center_size(rect.right_center() - vec2(16.0, 0.0), vec2(22.0, 22.0));
                let xr = ui.interact(x, egui::Id::new(("bm_del", i)), Sense::click());
                if resp.hovered() || xr.hovered() {
                    ui.painter().text(
                        x.center(),
                        Align2::CENTER_CENTER,
                        ICON_DELETE,
                        theme::icon_font(12.0),
                        if xr.hovered() { pal.warn } else { pal.dim },
                    );
                }
                if xr.clicked() {
                    remove = Some(i);
                } else if resp.clicked() {
                    go = Some(b.pos);
                }
            }
        });
        if let Some(i) = remove {
            if let Some(e) = self.library.entry_mut(&key) {
                e.bookmarks.remove(i);
            }
            self.library.save();
        }
        if let (Some(pos), Some(tab)) = (go, self.tab_mut()) {
            tab.goto(pos);
        }
    }

    fn side_info(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let Some(tab) = self.tab() else { return };
        if !matches!(tab.state, State::Ready(_)) {
            return;
        }
        let Some(info) = tab.info.as_ref() else { return };
        let mut rows: Vec<(String, String)> = vec![
            (tr("Название", "Title").to_owned(), tab.title.clone()),
            (tr("Тип", "Type").to_owned(), info.kind.label().to_owned()),
            (tr("Размер", "Size").to_owned(), human_size(info.size)),
        ];
        if let Some(a) = &info.author {
            rows.push((tr("Автор", "Author").to_owned(), a.clone()));
        }
        if let Some(p) = info.pages {
            rows.push((tr("Страниц", "Pages").to_owned(), p.to_string()));
        }
        if info.words > 0 && !matches!(info.kind, folio_core::Kind::Sheet | folio_core::Kind::Csv) {
            rows.push((tr("Слов", "Words").to_owned(), info.words.to_string()));
            rows.push((
                tr("Время чтения", "Reading time").to_owned(),
                duration_text(info.words.div_ceil(220)),
            ));
        }
        if let Some(p) = tab.progress() {
            rows.push((tr("Прочитано", "Read").to_owned(), format!("{:.0}%", p * 100.0)));
        }
        if let Ok(m) = std::fs::metadata(&tab.path).and_then(|m| m.modified()) {
            rows.push((
                tr("Изменён", "Modified").to_owned(),
                crate::timeutil::date_text(crate::timeutil::system_time_to_unix(m)),
            ));
        }
        let path = tab.path.display().to_string();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("info_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    for (k, v) in rows {
                        ui.label(RichText::new(k).color(pal.dim));
                        ui.add(egui::Label::new(RichText::new(v).color(pal.text)).wrap());
                        ui.end_row();
                    }
                });
            ui.add_space(10.0);
            ui.label(RichText::new(tr("Путь", "Path")).color(pal.dim));
            ui.add(
                egui::Label::new(RichText::new(&path).color(pal.text).size(12.5))
                    .wrap()
                    .selectable(true),
            );
            ui.add_space(6.0);
            if ui.button(tr("Показать в Проводнике", "Show in Explorer")).clicked() {
                crate::winint::show_in_explorer(std::path::Path::new(&path));
            }
        });
    }
}
