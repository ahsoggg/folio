//! Window furniture: tab strip, toolbar, find bar, status bar, welcome screen, command palette, dialogs.

use super::doc::{State, Tab, View};
use super::flowview::FlowEvent;
use super::paged::{PageEvent, ZoomMode};
use super::theme::{self, icon_button};
use super::{App, UpdateState};
use crate::i18n::{human_size, plural_ru, tr, trf};
use crate::settings::{ReadingFont, ThemeId};
use crate::{diag, installer, shell, timeutil, update, winint};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Frame, Id, Key, Layout, Margin, RichText, Sense, Stroke, Ui, pos2, vec2,
};

pub const ICON_OPEN: &str = "\u{E8E5}";
pub const ICON_MENU: &str = "\u{E700}";
pub const ICON_SEARCH: &str = "\u{E721}";
pub const ICON_STAR: &str = "\u{E734}";
pub const ICON_STAR_FILLED: &str = "\u{E735}";
pub const ICON_ZOOM_IN: &str = "\u{E8A3}";
pub const ICON_ZOOM_OUT: &str = "\u{E71F}";
pub const ICON_THEME: &str = "\u{E790}";
pub const ICON_SPEAK: &str = "\u{E767}";
pub const ICON_PLAY: &str = "\u{E768}";
pub const ICON_PAUSE: &str = "\u{E769}";
pub const ICON_STOP: &str = "\u{E71A}";
pub const ICON_MORE: &str = "\u{E712}";
pub const ICON_SETTINGS: &str = "\u{E713}";
pub const ICON_PREV: &str = "\u{E76B}";
pub const ICON_NEXT: &str = "\u{E76C}";
pub const ICON_UP: &str = "\u{E70E}";
pub const ICON_DOWN: &str = "\u{E70D}";
pub const ICON_CLOSE: &str = "\u{E711}";
pub const ICON_ADD: &str = "\u{E710}";
pub const ICON_LIST: &str = "\u{E8FD}";
pub const ICON_PAGES: &str = "\u{E8A9}";
pub const ICON_INFO: &str = "\u{E946}";
pub const ICON_FONT: &str = "\u{E8D2}";
pub const ICON_DELETE: &str = "\u{E74D}";

#[derive(Clone, Copy)]
enum Cmd {
    Open,
    ToggleSidebar,
    Theme(ThemeId),
    CycleTheme,
    FontUp,
    FontDown,
    FontKind,
    FitWidth,
    FitPage,
    Zoom100,
    GoTo,
    Bookmark,
    Speak,
    Fullscreen,
    Settings,
    About,
    CheckUpdates,
    CopyAll,
    Export,
    OpenExternal,
    ShowInExplorer,
    Print,
    CloseTab,
    Find,
    NextFile,
    PrevFile,
    SavePage,
}

fn commands() -> Vec<(String, &'static str, Cmd)> {
    let mut v = vec![
        (tr("Открыть файл…", "Open file…").to_owned(), "Ctrl+O", Cmd::Open),
        (
            tr("Найти в документе", "Find in document").to_owned(),
            "Ctrl+F",
            Cmd::Find,
        ),
        (tr("Перейти к странице…", "Go to page…").to_owned(), "Ctrl+G", Cmd::GoTo),
        (
            tr("Добавить или убрать закладку", "Toggle bookmark").to_owned(),
            "Ctrl+B",
            Cmd::Bookmark,
        ),
        (tr("Читать вслух", "Read aloud").to_owned(), "Ctrl+L", Cmd::Speak),
        (
            tr("Показать или скрыть боковую панель", "Toggle sidebar").to_owned(),
            "",
            Cmd::ToggleSidebar,
        ),
        (tr("Сменить тему", "Next theme").to_owned(), "T", Cmd::CycleTheme),
        (
            tr("Тема: Mocha", "Theme: Mocha").to_owned(),
            "",
            Cmd::Theme(ThemeId::Mocha),
        ),
        (
            tr("Тема: светлая", "Theme: Light").to_owned(),
            "",
            Cmd::Theme(ThemeId::Light),
        ),
        (
            tr("Тема: сепия", "Theme: Sepia").to_owned(),
            "",
            Cmd::Theme(ThemeId::Sepia),
        ),
        (
            tr("Тема: ночь (чёрная)", "Theme: Night (black)").to_owned(),
            "",
            Cmd::Theme(ThemeId::Night),
        ),
        (
            tr("Крупнее текст / масштаб", "Larger text / zoom in").to_owned(),
            "Ctrl +",
            Cmd::FontUp,
        ),
        (
            tr("Мельче текст / масштаб", "Smaller text / zoom out").to_owned(),
            "Ctrl −",
            Cmd::FontDown,
        ),
        (
            tr("Шрифт: с засечками или без", "Font: serif or sans").to_owned(),
            "",
            Cmd::FontKind,
        ),
        (tr("По ширине окна", "Fit width").to_owned(), "", Cmd::FitWidth),
        (tr("Страница целиком", "Fit page").to_owned(), "", Cmd::FitPage),
        (tr("Масштаб 100%", "Zoom 100%").to_owned(), "Ctrl 0", Cmd::Zoom100),
        (tr("Во весь экран", "Full screen").to_owned(), "F11", Cmd::Fullscreen),
        (
            tr("Скопировать весь текст", "Copy all text").to_owned(),
            "",
            Cmd::CopyAll,
        ),
        (
            tr("Сохранить текст в .txt…", "Save text as .txt…").to_owned(),
            "",
            Cmd::Export,
        ),
        (
            tr("Открыть в программе по умолчанию", "Open in the default app").to_owned(),
            "",
            Cmd::OpenExternal,
        ),
        (
            tr("Показать в Проводнике", "Show in Explorer").to_owned(),
            "",
            Cmd::ShowInExplorer,
        ),
        (tr("Печать", "Print").to_owned(), "Ctrl+P", Cmd::Print),
        (
            tr("Следующий файл в папке", "Next file in the folder").to_owned(),
            "Alt →",
            Cmd::NextFile,
        ),
        (
            tr("Предыдущий файл в папке", "Previous file in the folder").to_owned(),
            "Alt ←",
            Cmd::PrevFile,
        ),
        (
            tr("Сохранить страницу как картинку…", "Save this page as an image…").to_owned(),
            "",
            Cmd::SavePage,
        ),
        (tr("Закрыть вкладку", "Close tab").to_owned(), "Ctrl+W", Cmd::CloseTab),
        (tr("Настройки", "Settings").to_owned(), "Ctrl+,", Cmd::Settings),
        (
            tr("Проверить обновления", "Check for updates").to_owned(),
            "",
            Cmd::CheckUpdates,
        ),
        (tr("О программе", "About").to_owned(), "", Cmd::About),
    ];
    v.shrink_to_fit();
    v
}

/// A subsequence match with a bonus for runs and word starts; `None` when the letters do not all appear in order.
pub fn fuzzy(hay: &str, needle: &str) -> Option<i32> {
    let hay: Vec<char> = hay.to_lowercase().chars().collect();
    let mut score = 0;
    let mut at = 0usize;
    let mut last: Option<usize> = None;
    for n in needle.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let found = (at..hay.len()).find(|&i| hay[i] == n)?;
        score += 10;
        if last.is_some_and(|l| l + 1 == found) {
            score += 15;
        }
        if found == 0 || !hay[found - 1].is_alphanumeric() {
            score += 12;
        }
        score -= (found - at) as i32;
        last = Some(found);
        at = found + 1;
    }
    Some(score)
}

fn kind_color(kind: &str, pal: &theme::Palette) -> Color32 {
    let k = kind.to_lowercase();
    if k.contains("pdf") {
        Color32::from_rgb(243, 139, 168)
    } else if k.contains("word") || k.contains("rtf") || k.contains("text") && k.contains("open") {
        Color32::from_rgb(137, 180, 250)
    } else if k.contains("epub") || k.contains("fb2") || k.contains("comic") {
        Color32::from_rgb(166, 227, 161)
    } else if k.contains("sheet") || k.contains("csv") {
        Color32::from_rgb(148, 226, 213)
    } else if k.contains("power") || k.contains("presentation") {
        Color32::from_rgb(250, 179, 135)
    } else {
        pal.accent
    }
}

impl App {
    // ---- tab strip ----

    pub(super) fn tab_strip(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let mut close: Option<usize> = None;
        let mut activate: Option<usize> = None;
        let mut menu_action: Option<(usize, u8)> = None;
        egui::Panel::top("tabs")
            .exact_size(38.0)
            .frame(Frame::new().fill(pal.bg).inner_margin(Margin::symmetric(8, 4)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    egui::ScrollArea::horizontal()
                        .id_salt("tabs_scroll")
                        .max_width(ui.available_width() - 44.0)
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for (i, t) in self.tabs.iter().enumerate() {
                                    let active = i == self.active;
                                    let label: String = {
                                        let n = t.title.chars().count();
                                        if n > 24 {
                                            format!("{}…", t.title.chars().take(23).collect::<String>())
                                        } else {
                                            t.title.clone()
                                        }
                                    };
                                    let font = egui::FontId::proportional(13.5);
                                    let galley = ui.painter().layout_no_wrap(
                                        label,
                                        font,
                                        if active { pal.text } else { pal.dim },
                                    );
                                    let w = galley.size().x + 56.0;
                                    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(110.0), 30.0), Sense::click());
                                    let fill = if active {
                                        pal.panel
                                    } else if resp.hovered() {
                                        pal.card.gamma_multiply(0.7)
                                    } else {
                                        Color32::TRANSPARENT
                                    };
                                    ui.painter().rect_filled(
                                        rect,
                                        CornerRadius {
                                            nw: 8,
                                            ne: 8,
                                            sw: 0,
                                            se: 0,
                                        },
                                        fill,
                                    );
                                    if active {
                                        ui.painter().rect_filled(
                                            egui::Rect::from_min_size(
                                                rect.left_bottom() - vec2(0.0, 2.0),
                                                vec2(rect.width(), 2.0),
                                            ),
                                            0,
                                            pal.accent,
                                        );
                                    }
                                    let dot = t.kind.map(|k| kind_color(k.label(), &pal)).unwrap_or(pal.dim);
                                    let dot_c = if matches!(t.state, State::Loading(_)) {
                                        pal.warn
                                    } else {
                                        dot
                                    };
                                    ui.painter()
                                        .circle_filled(rect.left_center() + vec2(14.0, 0.0), 4.0, dot_c);
                                    ui.painter().galley(
                                        pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0),
                                        galley,
                                        pal.text,
                                    );
                                    let close_rect = egui::Rect::from_center_size(
                                        rect.right_center() - vec2(16.0, 0.0),
                                        vec2(20.0, 20.0),
                                    );
                                    let close_resp =
                                        ui.interact(close_rect, Id::new(("tab_close", t.id)), Sense::click());
                                    if resp.hovered() || active {
                                        if close_resp.hovered() {
                                            ui.painter().rect_filled(close_rect, 5, pal.card_hover);
                                        }
                                        ui.painter().text(
                                            close_rect.center(),
                                            Align2::CENTER_CENTER,
                                            ICON_CLOSE,
                                            theme::icon_font(10.0),
                                            pal.dim,
                                        );
                                    }
                                    if close_resp.clicked() || resp.middle_clicked() {
                                        close = Some(i);
                                    } else if resp.clicked() {
                                        activate = Some(i);
                                    }
                                    resp.context_menu(|ui| {
                                        if ui.button(tr("Закрыть", "Close")).clicked() {
                                            menu_action = Some((i, 0));
                                            ui.close();
                                        }
                                        if ui.button(tr("Закрыть остальные", "Close others")).clicked()
                                        {
                                            menu_action = Some((i, 1));
                                            ui.close();
                                        }
                                        if ui.button(tr("Показать в Проводнике", "Show in Explorer")).clicked()
                                        {
                                            menu_action = Some((i, 2));
                                            ui.close();
                                        }
                                        if ui.button(tr("Копировать путь", "Copy path")).clicked() {
                                            menu_action = Some((i, 3));
                                            ui.close();
                                        }
                                    });
                                    resp.on_hover_text(t.path.display().to_string());
                                }
                            });
                        });
                    if icon_button(
                        ui,
                        &pal,
                        ICON_ADD,
                        tr("Открыть файл (Ctrl+O)", "Open file (Ctrl+O)"),
                        false,
                    )
                    .clicked()
                    {
                        self.open_dialog(&ui.ctx().clone());
                    }
                });
            });
        if let Some(i) = activate {
            self.active = i;
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
        if let Some((i, a)) = menu_action {
            match a {
                0 => self.close_tab(i),
                1 => {
                    let keep = self.tabs.get(i).map(|t| t.id);
                    for j in (0..self.tabs.len()).rev() {
                        if Some(self.tabs[j].id) != keep {
                            self.close_tab(j);
                        }
                    }
                    self.active = 0;
                }
                2 => {
                    if let Some(t) = self.tabs.get(i) {
                        winint::show_in_explorer(&t.path);
                    }
                }
                _ => {
                    if let Some(t) = self.tabs.get(i) {
                        ui.ctx().copy_text(t.path.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }

    // ---- toolbar ----

    pub(super) fn toolbar(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let has_doc = self.tab().is_some_and(|t| matches!(t.state, State::Ready(_)));
        let mut action: Option<Cmd> = None;
        let mut zoom_pick: Option<ZoomMode> = None;
        let mut page_jump: Option<usize> = None;
        let mut page_box: Option<String> = None;
        egui::Panel::top("toolbar")
            .exact_size(44.0)
            .frame(Frame::new().fill(pal.panel).inner_margin(Margin::symmetric(10, 5)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    // flat buttons in the toolbar: the menu buttons must look like the icon buttons
                    let w = &mut ui.visuals_mut().widgets;
                    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
                    w.inactive.bg_stroke = Stroke::NONE;
                    if icon_button(
                        ui,
                        &pal,
                        ICON_MENU,
                        tr("Боковая панель", "Sidebar"),
                        self.settings.sidebar_open && has_doc,
                    )
                    .clicked()
                    {
                        action = Some(Cmd::ToggleSidebar);
                    }
                    if icon_button(ui, &pal, ICON_OPEN, tr("Открыть (Ctrl+O)", "Open (Ctrl+O)"), false).clicked()
                    {
                        action = Some(Cmd::Open);
                    }
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    if has_doc {
                        if let Some(tab) = self.tabs.get(self.active) {
                            match tab.view() {
                                Some(View::Paged(p)) => {
                                    let cur = p.current_page;
                                    let n = p.page_count();
                                    if icon_button(
                                        ui,
                                        &pal,
                                        ICON_PREV,
                                        tr("Предыдущая страница", "Previous page"),
                                        false,
                                    )
                                    .clicked()
                                        && cur > 0
                                    {
                                        page_jump = Some(cur - 1);
                                    }
                                    let mut text = (cur + 1).to_string();
                                    let r = ui.add(
                                        egui::TextEdit::singleline(&mut text)
                                            .desired_width(44.0)
                                            .horizontal_align(Align::Center)
                                            .id(Id::new("page_box")),
                                    );
                                    if r.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) {
                                        page_box = Some(text);
                                    }
                                    ui.label(RichText::new(format!("/ {n}")).color(pal.dim));
                                    if icon_button(ui, &pal, ICON_NEXT, tr("Следующая страница", "Next page"), false)
                                        .clicked()
                                        && cur + 1 < n
                                    {
                                        page_jump = Some(cur + 1);
                                    }
                                    ui.add_space(8.0);
                                    if icon_button(
                                        ui,
                                        &pal,
                                        ICON_ZOOM_OUT,
                                        tr("Уменьшить (Ctrl −)", "Zoom out (Ctrl −)"),
                                        false,
                                    )
                                    .clicked()
                                    {
                                        action = Some(Cmd::FontDown);
                                    }
                                    ui.menu_button(format!("{:.0}%", p.percent()), |ui| {
                                        if ui.button(tr("По ширине окна", "Fit width")).clicked() {
                                            zoom_pick = Some(ZoomMode::FitWidth);
                                            ui.close();
                                        }
                                        if ui.button(tr("Страница целиком", "Fit page")).clicked() {
                                            zoom_pick = Some(ZoomMode::FitPage);
                                            ui.close();
                                        }
                                        ui.separator();
                                        for z in [50, 75, 100, 125, 150, 200, 300] {
                                            if ui.button(format!("{z}%")).clicked() {
                                                zoom_pick = Some(ZoomMode::Fixed(z as f32 / 100.0));
                                                ui.close();
                                            }
                                        }
                                    });
                                    if icon_button(
                                        ui,
                                        &pal,
                                        ICON_ZOOM_IN,
                                        tr("Увеличить (Ctrl +)", "Zoom in (Ctrl +)"),
                                        false,
                                    )
                                    .clicked()
                                    {
                                        action = Some(Cmd::FontUp);
                                    }
                                }
                                Some(View::Flow(_)) => {
                                    if icon_button(
                                        ui,
                                        &pal,
                                        ICON_ZOOM_OUT,
                                        tr("Мельче текст (Ctrl −)", "Smaller text (Ctrl −)"),
                                        false,
                                    )
                                    .clicked()
                                    {
                                        action = Some(Cmd::FontDown);
                                    }
                                    ui.label(RichText::new(format!("{:.0}", self.settings.font_size)).color(pal.dim));
                                    if icon_button(
                                        ui,
                                        &pal,
                                        ICON_ZOOM_IN,
                                        tr("Крупнее текст (Ctrl +)", "Larger text (Ctrl +)"),
                                        false,
                                    )
                                    .clicked()
                                    {
                                        action = Some(Cmd::FontUp);
                                    }
                                    if icon_button(
                                        ui,
                                        &pal,
                                        ICON_FONT,
                                        tr("Шрифт: с засечками / без", "Font: serif / sans"),
                                        false,
                                    )
                                    .clicked()
                                    {
                                        action = Some(Cmd::FontKind);
                                    }
                                }
                                Some(View::Sheet(_)) => {}
                                None => {}
                            }
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.menu_button(RichText::new(ICON_MORE).font(theme::icon_font(16.0)), |ui| {
                            ui.set_min_width(240.0);
                            if ui
                                .add_enabled(
                                    has_doc,
                                    egui::Button::new(tr("Скопировать весь текст", "Copy all text")),
                                )
                                .clicked()
                            {
                                action = Some(Cmd::CopyAll);
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    has_doc,
                                    egui::Button::new(tr("Сохранить текст в .txt…", "Save text as .txt…")),
                                )
                                .clicked()
                            {
                                action = Some(Cmd::Export);
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .add_enabled(
                                    has_doc,
                                    egui::Button::new(tr(
                                        "Открыть в программе по умолчанию",
                                        "Open in the default app",
                                    )),
                                )
                                .clicked()
                            {
                                action = Some(Cmd::OpenExternal);
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    has_doc,
                                    egui::Button::new(tr("Показать в Проводнике", "Show in Explorer")),
                                )
                                .clicked()
                            {
                                action = Some(Cmd::ShowInExplorer);
                                ui.close();
                            }
                            if ui
                                .add_enabled(has_doc, egui::Button::new(tr("Печать… (Ctrl+P)", "Print… (Ctrl+P)")))
                                .clicked()
                            {
                                action = Some(Cmd::Print);
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(tr("Во весь экран (F11)", "Full screen (F11)")).clicked() {
                                action = Some(Cmd::Fullscreen);
                                ui.close();
                            }
                            if ui.button(tr("Настройки (Ctrl+,)", "Settings (Ctrl+,)")).clicked() {
                                action = Some(Cmd::Settings);
                                ui.close();
                            }
                            if ui.button(tr("О программе", "About")).clicked() {
                                action = Some(Cmd::About);
                                ui.close();
                            }
                        });
                        if icon_button(
                            ui,
                            &pal,
                            ICON_SETTINGS,
                            tr("Настройки (Ctrl+,)", "Settings (Ctrl+,)"),
                            self.settings_open,
                        )
                        .clicked()
                        {
                            action = Some(Cmd::Settings);
                        }
                        ui.menu_button(RichText::new(ICON_THEME).font(theme::icon_font(16.0)), |ui| {
                            for t in ThemeId::ALL {
                                let sel = self.settings.theme == t;
                                let p = theme::palette(t);
                                let (r, _) = ui.allocate_exact_size(vec2(0.0, 0.0), Sense::hover());
                                let _ = r;
                                ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                                    ui.painter().rect_filled(rect, 9, p.paper);
                                    ui.painter().rect_stroke(
                                        rect,
                                        9,
                                        Stroke::new(1.5, p.accent),
                                        egui::StrokeKind::Inside,
                                    );
                                    if ui.selectable_label(sel, theme::theme_name(t)).clicked() {
                                        action = Some(Cmd::Theme(t));
                                        ui.close();
                                    }
                                });
                            }
                        });
                        if self.can_speak() {
                            let speaking = self.speech.is_some();
                            let paused = self.speaker.paused;
                            if speaking {
                                if icon_button(ui, &pal, ICON_STOP, tr("Остановить чтение", "Stop reading"), false)
                                    .clicked()
                                {
                                    self.stop_speech();
                                }
                            }
                            let glyph = if speaking && !paused {
                                ICON_PAUSE
                            } else if speaking {
                                ICON_PLAY
                            } else {
                                ICON_SPEAK
                            };
                            let tip = if speaking && !paused {
                                tr("Пауза", "Pause")
                            } else {
                                tr("Читать вслух (Ctrl+L)", "Read aloud (Ctrl+L)")
                            };
                            if icon_button(ui, &pal, glyph, tip, speaking).clicked() {
                                action = Some(Cmd::Speak);
                            }
                        }
                        let marked = self.tab().and_then(|t| {
                            let pos = t.position()?;
                            let e = self.library.get(&t.key)?;
                            Some(e.bookmarks.iter().any(|b| t.same_place(b.pos, pos)))
                        });
                        if has_doc
                            && icon_button(
                                ui,
                                &pal,
                                if marked == Some(true) {
                                    ICON_STAR_FILLED
                                } else {
                                    ICON_STAR
                                },
                                tr("Закладка (Ctrl+B)", "Bookmark (Ctrl+B)"),
                                marked == Some(true),
                            )
                            .clicked()
                        {
                            action = Some(Cmd::Bookmark);
                        }
                        if has_doc
                            && icon_button(
                                ui,
                                &pal,
                                ICON_SEARCH,
                                tr("Поиск (Ctrl+F)", "Find (Ctrl+F)"),
                                self.find.open,
                            )
                            .clicked()
                        {
                            action = Some(Cmd::Find);
                        }
                        if icon_button(
                            ui,
                            &pal,
                            "\u{E756}",
                            tr("Команды (Ctrl+K)", "Commands (Ctrl+K)"),
                            self.palette.open,
                        )
                        .clicked()
                        {
                            self.palette.open = true;
                            self.palette.text.clear();
                            self.palette.sel = 0;
                        }
                    });
                });
            });
        if let Some(mode) = zoom_pick
            && let Some(View::Paged(p)) = self.tab_mut().and_then(Tab::view_mut)
        {
            p.zoom = mode;
        }
        if let Some(p) = page_jump
            && let Some(View::Paged(v)) = self.tab_mut().and_then(Tab::view_mut)
        {
            v.go_to(p, 0.0);
        }
        if let Some(t) = page_box {
            self.go_to_text(&t);
        }
        if let Some(a) = action {
            self.run_command(&ctx, a);
        }
    }

    fn run_command(&mut self, ctx: &egui::Context, c: Cmd) {
        match c {
            Cmd::Open => self.open_dialog(ctx),
            Cmd::ToggleSidebar => {
                self.settings.sidebar_open = !self.settings.sidebar_open;
                self.settings.save();
            }
            Cmd::Theme(t) => self.set_theme(t),
            Cmd::CycleTheme => {
                let t = self.settings.theme.next();
                self.set_theme(t);
            }
            Cmd::FontUp => self.zoom_in(1.0),
            Cmd::FontDown => self.zoom_in(-1.0),
            Cmd::FontKind => {
                self.settings.font = match self.settings.font {
                    ReadingFont::Serif => ReadingFont::Sans,
                    ReadingFont::Sans => ReadingFont::Serif,
                };
                self.settings.save();
            }
            Cmd::FitWidth => {
                if let Some(View::Paged(p)) = self.tab_mut().and_then(Tab::view_mut) {
                    p.zoom = ZoomMode::FitWidth;
                }
            }
            Cmd::FitPage => {
                if let Some(View::Paged(p)) = self.tab_mut().and_then(Tab::view_mut) {
                    p.zoom = ZoomMode::FitPage;
                }
            }
            Cmd::Zoom100 => self.zoom_reset(),
            Cmd::GoTo => self.goto_ui = Some(String::new()),
            Cmd::Bookmark => self.toggle_bookmark(),
            Cmd::Speak => {
                if self.can_speak() {
                    self.toggle_speech();
                }
            }
            Cmd::Fullscreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Cmd::Settings => self.settings_open = !self.settings_open,
            Cmd::About => self.about_open = true,
            Cmd::CheckUpdates => self.check_updates(ctx),
            Cmd::CopyAll => self.copy_all(ctx),
            Cmd::Export => self.export_text(),
            Cmd::OpenExternal => {
                if let Some(t) = self.tab() {
                    winint::open_with_default(&t.path);
                }
            }
            Cmd::ShowInExplorer => {
                if let Some(t) = self.tab() {
                    winint::show_in_explorer(&t.path);
                }
            }
            Cmd::Print => {
                if let Some(t) = self.tab() {
                    if !winint::print_file(&t.path) {
                        self.toast(tr(
                            "Для этого типа файлов нет программы печати",
                            "No program can print this file type",
                        ));
                    }
                }
            }
            Cmd::CloseTab => self.close_tab(self.active),
            Cmd::Find => self.open_find(),
            Cmd::NextFile => self.open_sibling(ctx, 1),
            Cmd::PrevFile => self.open_sibling(ctx, -1),
            Cmd::SavePage => self.save_page_image(),
        }
    }

    // ---- find bar ----

    pub(super) fn find_bar(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let (cur, total, running) = self.find_status();
        let mut step: Option<bool> = None;
        let mut close = false;
        let mut changed = false;
        egui::Panel::top("findbar")
            .exact_size(42.0)
            .frame(Frame::new().fill(pal.card).inner_margin(Margin::symmetric(12, 5)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(ICON_SEARCH).font(theme::icon_font(14.0)).color(pal.dim));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.find.text)
                            .hint_text(tr("Найти в документе…", "Find in document…"))
                            .desired_width(320.0)
                            .id(Id::new("find_box")),
                    );
                    if self.find.focus {
                        r.request_focus();
                        self.find.focus = false;
                    }
                    if r.changed() {
                        changed = true;
                    }
                    if r.has_focus() || r.lost_focus() {
                        let enter = ui.input(|i| i.key_pressed(Key::Enter));
                        if enter {
                            if self.find.changed_at.is_some() || self.find.applied != self.find.text {
                                changed = true;
                                self.find.changed_at = None;
                            } else {
                                step = Some(!ui.input(|i| i.modifiers.shift));
                            }
                            r.request_focus();
                        }
                    }
                    if ui
                        .toggle_value(&mut self.find.case, "Aa")
                        .on_hover_text(tr("Учитывать регистр (PDF)", "Match case (PDF)"))
                        .changed()
                    {
                        changed = true;
                    }
                    let label = if running {
                        tr("поиск…", "searching…").to_owned()
                    } else if self.find.applied.is_empty() {
                        String::new()
                    } else if total == 0 {
                        tr("ничего не найдено", "no matches").to_owned()
                    } else {
                        format!("{} / {}{}", cur + 1, total, if total >= 2000 { "+" } else { "" })
                    };
                    ui.label(
                        RichText::new(label).color(if total == 0 && !self.find.applied.is_empty() && !running {
                            pal.warn
                        } else {
                            pal.dim
                        }),
                    );
                    if icon_button(
                        ui,
                        &pal,
                        ICON_UP,
                        tr("Предыдущее (Shift+F3)", "Previous (Shift+F3)"),
                        false,
                    )
                    .clicked()
                    {
                        step = Some(false);
                    }
                    if icon_button(ui, &pal, ICON_DOWN, tr("Следующее (F3)", "Next (F3)"), false).clicked() {
                        step = Some(true);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, &pal, ICON_CLOSE, tr("Закрыть (Esc)", "Close (Esc)"), false).clicked()
                        {
                            close = true;
                        }
                    });
                });
            });
        if changed {
            self.find.changed_at = Some(std::time::Instant::now());
        }
        if let Some(fwd) = step {
            self.find_step(fwd);
        }
        if close {
            self.close_find();
        }
    }

    // ---- status bar ----

    pub(super) fn status_bar(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let mut restart = false;
        let mut start_update: Option<update::Release> = None;
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .frame(Frame::new().fill(pal.panel).inner_margin(Margin::symmetric(12, 3)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let dim = |t: String| RichText::new(t).color(pal.dim).size(12.5);
                    if let Some(tab) = self.tab() {
                        match &tab.state {
                            State::Loading(_) => {
                                ui.add(egui::Spinner::new().size(12.0));
                                ui.label(dim(tr("Открываю…", "Opening…").to_owned()));
                            }
                            State::Ready(v) => {
                                let info = tab.info.as_ref();
                                match &**v {
                                    View::Paged(p) => {
                                        ui.label(dim(trf(
                                            "Страница {0} из {1}",
                                            "Page {0} of {1}",
                                            &[&(p.current_page + 1), &p.page_count()],
                                        )));
                                        ui.label(dim(format!("{:.0}%", p.percent())));
                                    }
                                    View::Flow(f) => {
                                        ui.label(dim(format!("{:.0}%", f.progress() * 100.0)));
                                        if let Some(m) = tab.minutes_left() {
                                            ui.label(dim(trf(
                                                "осталось ≈ {0}",
                                                "≈ {0} left",
                                                &[&crate::i18n::duration_text(m)],
                                            )));
                                        }
                                    }
                                    View::Sheet(s) => {
                                        let sh = s.sheet();
                                        ui.label(dim(trf(
                                            "{0} строк × {1} столбцов",
                                            "{0} rows × {1} columns",
                                            &[&sh.rows.len(), &sh.cols],
                                        )));
                                        if sh.truncated {
                                            ui.label(
                                                RichText::new(tr("показаны не все строки", "not all rows shown"))
                                                    .color(pal.warn)
                                                    .size(12.5),
                                            );
                                        }
                                    }
                                }
                                if let Some(i) = info {
                                    if i.words > 0 && !matches!(&**v, View::Sheet(_)) {
                                        let w = i.words;
                                        ui.label(dim(format!(
                                            "{} {}",
                                            w,
                                            plural_ru_or(w, "слово", "слова", "слов", "words")
                                        )));
                                    }
                                    ui.label(dim(format!("{} · {}", i.kind.label(), human_size(i.size))));
                                }
                            }
                            State::Failed(_) => {
                                ui.label(
                                    RichText::new(tr("Не удалось открыть", "Could not open"))
                                        .color(pal.warn)
                                        .size(12.5),
                                );
                            }
                            State::NeedPassword { .. } => {
                                ui.label(dim(tr(
                                    "Документ защищён паролем",
                                    "The document is password protected",
                                )
                                .to_owned()));
                            }
                        }
                    } else {
                        ui.label(dim(tr(
                            "Перетащите файл в окно или нажмите Ctrl+O",
                            "Drop a file here or press Ctrl+O",
                        )
                        .to_owned()));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match &self.update {
                            UpdateState::Available(rel) => {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(trf("Обновить до {0}", "Update to {0}", &[&rel.tag]))
                                                .size(12.5)
                                                .color(pal.on_accent),
                                        )
                                        .fill(pal.accent)
                                        .small(),
                                    )
                                    .clicked()
                                {
                                    start_update = Some(rel.clone());
                                }
                            }
                            UpdateState::Downloading { done, total, tag, .. } => {
                                let (d, t) = (
                                    done.load(std::sync::atomic::Ordering::Relaxed),
                                    total.load(std::sync::atomic::Ordering::Relaxed),
                                );
                                let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
                                ui.add(egui::ProgressBar::new(frac).desired_width(110.0).desired_height(10.0));
                                ui.label(dim(trf("Загрузка {0}…", "Downloading {0}…", &[tag])));
                            }
                            UpdateState::Ready(tag) => {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(trf(
                                                "{0} готова — перезапустить",
                                                "{0} is ready — restart",
                                                &[tag],
                                            ))
                                            .size(12.5)
                                            .color(pal.on_accent),
                                        )
                                        .fill(pal.accent)
                                        .small(),
                                    )
                                    .clicked()
                                {
                                    restart = true;
                                }
                            }
                            _ => {}
                        }
                        if self.speech.is_some() {
                            ui.label(
                                RichText::new(tr("● идёт чтение вслух", "● reading aloud"))
                                    .color(pal.accent)
                                    .size(12.5),
                            );
                        }
                    });
                });
            });
        if let Some(rel) = start_update {
            self.start_update(&ctx, rel);
        }
        if restart {
            self.restart(&ctx);
        }
    }

    // ---- the document area ----

    pub(super) fn document_area(
        &mut self,
        ui: &mut Ui,
        flow_events: &mut Vec<FlowEvent>,
        page_events: &mut Vec<PageEvent>,
    ) {
        let pal = self.pal;
        let look = self.look();
        let tone = self.pdf_tone();
        let column = self.settings.column_width;
        let ctx = ui.ctx().clone();
        let active = self.active;
        if self.tabs.is_empty() {
            self.welcome(ui);
            return;
        }
        enum Next {
            None,
            Password(String),
            Forget,
            External,
            Retry,
        }
        let mut next = Next::None;
        let tab = &mut self.tabs[active];
        let kind = tab.kind;
        match &mut tab.state {
            State::Loading(_) => {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 2.0 - 40.0);
                        ui.add(egui::Spinner::new().size(34.0).color(pal.accent));
                        ui.add_space(10.0);
                        ui.label(RichText::new(tr("Открываю документ…", "Opening the document…")).color(pal.dim));
                    });
                });
            }
            State::Failed(msg) => {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.28);
                    ui.label(RichText::new("\u{E783}").font(theme::icon_font(42.0)).color(pal.warn));
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(tr("Не получилось открыть этот файл", "This file could not be opened"))
                            .size(20.0)
                            .strong()
                            .color(pal.ink),
                    );
                    ui.add_space(4.0);
                    ui.label(RichText::new(msg.as_str()).color(pal.ink_dim));
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 330.0).max(0.0) / 2.0);
                        if ui
                            .button(tr("Открыть в другой программе", "Open in another app"))
                            .clicked()
                        {
                            next = Next::External;
                        }
                        if ui.button(tr("Повторить", "Try again")).clicked() {
                            next = Next::Retry;
                        }
                        if ui.button(tr("Закрыть вкладку", "Close tab")).clicked() {
                            next = Next::Forget;
                        }
                    });
                });
            }
            State::NeedPassword { input, wrong } => {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.3);
                    ui.label(RichText::new("\u{E72E}").font(theme::icon_font(40.0)).color(pal.accent));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(tr("Документ защищён паролем", "This document is password protected"))
                            .size(20.0)
                            .strong()
                            .color(pal.ink),
                    );
                    ui.add_space(10.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(input)
                            .password(true)
                            .desired_width(260.0)
                            .hint_text(tr("Пароль", "Password")),
                    );
                    r.request_focus();
                    if *wrong {
                        ui.label(RichText::new(tr("Неверный пароль", "Wrong password")).color(pal.warn));
                    }
                    ui.add_space(8.0);
                    let go = theme::primary(ui, &pal, tr("Открыть", "Open")).clicked()
                        || (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)));
                    if go && !input.is_empty() {
                        next = Next::Password(input.clone());
                    }
                });
            }
            State::Ready(view) => match &mut **view {
                View::Flow(f) => {
                    let wide = matches!(
                        kind,
                        Some(folio_core::Kind::Code | folio_core::Kind::Json | folio_core::Kind::Xml)
                    );
                    let col = if wide { column.max(1100.0) } else { column };
                    f.show(ui, &look, col, flow_events);
                }
                View::Paged(p) => {
                    let t = if p.is_pdf { tone } else { folio_core::pdf::Tone::Paper };
                    let bg = pal.bg;
                    egui::Frame::new().fill(bg).show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        p.show(ui, &pal, t, page_events);
                    });
                }
                View::Sheet(s) => s.show(ui, &pal),
            },
        }
        match next {
            Next::None => {}
            Next::Password(p) => {
                if let Some(State::NeedPassword { wrong, .. }) = self.tabs.get_mut(active).map(|t| &mut t.state) {
                    *wrong = true;
                }
                self.retry_with_password(&ctx, active, p);
            }
            Next::Forget => {
                let key = self.tabs[active].key.clone();
                self.library.forget(&key);
                self.close_tab(active);
            }
            Next::External => {
                winint::open_with_default(&self.tabs[active].path);
            }
            Next::Retry => {
                let path = self.tabs[active].path.clone();
                self.tabs[active].state = State::Loading(super::doc::spawn_load(&ctx, path, None));
            }
        }
    }

    fn welcome(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let mut open: Option<String> = None;
        let mut forget: Option<String> = None;
        let mut act: Option<Cmd> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.set_max_width(780.0);
                ui.label(RichText::new("Folio").size(46.0).strong().color(pal.ink));
                ui.label(
                    RichText::new(tr(
                        "Читалка для PDF, Word, книг, таблиц и многого другого",
                        "A reader for PDF, Word, books, spreadsheets and more",
                    ))
                    .size(16.0)
                    .color(pal.ink_dim),
                );
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 330.0).max(0.0) / 2.0);
                    if theme::primary(ui, &pal, &format!("  {}  ", tr("Открыть файл", "Open a file"))).clicked() {
                        act = Some(Cmd::Open);
                    }
                    if ui.button(format!("  {}  ", tr("Команды  Ctrl+K", "Commands  Ctrl+K"))).clicked() {
                        self.palette.open = true;
                    }
                });
                ui.add_space(8.0);
                ui.label(RichText::new(tr("…или перетащите файл в это окно", "…or drop a file onto this window")).color(pal.ink_dim));
                ui.add_space(26.0);
            });
            let recent: Vec<_> = self.library.entries.iter().filter(|e| std::path::Path::new(&e.path).is_file()).take(8).cloned().collect();
            let width = ui.available_width().min(760.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(width);
                if !recent.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr("Недавние", "Recent")).size(17.0).strong().color(pal.ink));
                    });
                    ui.add_space(6.0);
                    for e in &recent {
                        let (rect, resp) = ui.allocate_exact_size(vec2(width, 58.0), Sense::click());
                        let fill = if resp.hovered() { pal.card_hover } else { pal.card };
                        ui.painter().rect_filled(rect, 10, fill);
                        ui.painter().rect_stroke(rect, 10, Stroke::new(1.0, pal.border.gamma_multiply(0.7)), egui::StrokeKind::Inside);
                        let c = kind_color(&e.kind, &pal);
                        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(5.0, 38.0)), 3, c);
                        ui.painter().text(rect.min + vec2(26.0, 12.0), Align2::LEFT_TOP, &e.title, egui::FontId::proportional(15.5), pal.text);
                        let sub = format!(
                            "{} · {} · {}",
                            e.kind,
                            timeutil::date_text(e.opened),
                            std::path::Path::new(&e.path).parent().map(|p| p.display().to_string()).unwrap_or_default()
                        );
                        let sub: String = if sub.chars().count() > 96 { format!("{}…", sub.chars().take(95).collect::<String>()) } else { sub };
                        ui.painter().text(rect.min + vec2(26.0, 34.0), Align2::LEFT_TOP, sub, egui::FontId::proportional(12.0), pal.dim);
                        if e.progress > 0.005 {
                            let bar = egui::Rect::from_min_size(pos2(rect.right() - 130.0, rect.center().y - 3.0), vec2(90.0, 6.0));
                            ui.painter().rect_filled(bar, 3, pal.border);
                            ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, vec2(bar.width() * e.progress.min(1.0), 6.0)), 3, pal.accent);
                            ui.painter().text(
                                pos2(bar.right() + 8.0, bar.center().y),
                                Align2::LEFT_CENTER,
                                format!("{:.0}%", e.progress * 100.0),
                                egui::FontId::proportional(11.5),
                                pal.dim,
                            );
                        }
                        let x_rect = egui::Rect::from_center_size(rect.right_top() + vec2(-14.0, 14.0), vec2(18.0, 18.0));
                        let x_resp = ui.interact(x_rect, Id::new(("forget", &e.path)), Sense::click());
                        if resp.hovered() {
                            ui.painter().text(x_rect.center(), Align2::CENTER_CENTER, ICON_CLOSE, theme::icon_font(9.0), if x_resp.hovered() { pal.warn } else { pal.dim });
                        }
                        if x_resp.clicked() {
                            forget = Some(e.path.clone());
                        } else if resp.clicked() {
                            open = Some(e.path.clone());
                        }
                        ui.add_space(6.0);
                    }
                } else {
                    ui.add_space(6.0);
                    let tips = [
                        (tr("Откроет почти всё", "Opens almost anything"), tr("PDF, DOCX, ODT, RTF, EPUB, FB2, MD, HTML, TXT, XLSX, CSV, PPTX, комиксы CBZ и картинки", "PDF, DOCX, ODT, RTF, EPUB, FB2, MD, HTML, TXT, XLSX, CSV, PPTX, CBZ comics and images")),
                        (tr("Запоминает место", "Remembers your place"), tr("Закройте книгу — завтра она откроется на той же строке", "Close a book and it reopens on the same line tomorrow")),
                        (tr("Читает вслух", "Reads aloud"), tr("Ctrl+L — голос Windows озвучит документ", "Ctrl+L — a Windows voice reads the document to you")),
                        (tr("Всё под рукой", "Everything at hand"), tr("Ctrl+K — поиск по командам, Ctrl+F — по тексту, T — смена темы", "Ctrl+K searches commands, Ctrl+F the text, T changes the theme")),
                    ];
                    for (title, body) in tips {
                        theme::card(&pal).show(ui, |ui| {
                            ui.set_width(width - 34.0);
                            ui.label(RichText::new(title).strong().size(15.5).color(pal.text));
                            ui.label(RichText::new(body).color(pal.dim));
                        });
                        ui.add_space(6.0);
                    }
                }
            });
        });
        if let Some(c) = act {
            self.run_command(&ctx, c);
        }
        if let Some(p) = forget {
            self.library.forget(&p);
            self.library.save();
        }
        if let Some(p) = open {
            self.open_path(&ctx, p.into());
        }
    }

    // ---- overlays ----

    pub(super) fn overlays(&mut self, ctx: &egui::Context) {
        self.command_palette(ctx);
        self.goto_dialog(ctx);
        self.link_dialog(ctx);
        self.settings_window(ctx);
        self.about_window(ctx);
        self.toasts_ui(ctx);
        // files hovering over the window
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let pal = self.pal;
            let screen = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("drop_overlay")));
            painter.rect_filled(screen, 0, Color32::from_black_alpha(150));
            let r = screen.shrink(24.0);
            painter.rect_stroke(r, 18, Stroke::new(3.0, pal.accent), egui::StrokeKind::Inside);
            painter.text(
                screen.center(),
                Align2::CENTER_CENTER,
                tr("Отпустите, чтобы открыть", "Drop to open"),
                egui::FontId::proportional(28.0),
                Color32::WHITE,
            );
        }
    }

    fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.palette.open {
            return;
        }
        let pal = self.pal;
        let query = self.palette.text.clone();
        enum Item {
            Command(Cmd),
            Recent(String),
            Tab(usize),
        }
        let mut items: Vec<(i32, String, String, Item)> = Vec::new();
        for (label, hint, cmd) in commands() {
            let score = if query.trim().is_empty() {
                Some(0)
            } else {
                fuzzy(&label, &query)
            };
            if let Some(s) = score {
                items.push((s, label, hint.to_owned(), Item::Command(cmd)));
            }
        }
        for (i, t) in self.tabs.iter().enumerate() {
            let label = format!("{}  {}", tr("Вкладка:", "Tab:"), t.title);
            if let Some(s) = (!query.trim().is_empty()).then(|| fuzzy(&label, &query)).flatten() {
                items.push((s + 5, label, String::new(), Item::Tab(i)));
            }
        }
        for e in self.library.entries.iter().take(30) {
            let label = format!("{}  {}", tr("Недавнее:", "Recent:"), e.title);
            let score = if query.trim().is_empty() {
                None
            } else {
                fuzzy(&label, &query)
            };
            if let Some(s) = score {
                items.push((s, label, e.path.clone(), Item::Recent(e.path.clone())));
            }
        }
        if !query.trim().is_empty() {
            items.sort_by_key(|i| std::cmp::Reverse(i.0));
        }
        items.truncate(12);
        self.palette.sel = self.palette.sel.min(items.len().saturating_sub(1));
        let (down, up, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
            )
        });
        if down && !items.is_empty() {
            self.palette.sel = (self.palette.sel + 1) % items.len();
        }
        if up && !items.is_empty() {
            self.palette.sel = (self.palette.sel + items.len() - 1) % items.len();
        }
        let mut chosen: Option<usize> = enter.then_some(self.palette.sel).filter(|i| *i < items.len());
        let screen = ctx.content_rect();
        let backdrop = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, Id::new("palette_dim")));
        backdrop.rect_filled(screen, 0, Color32::from_black_alpha(90));
        egui::Area::new(Id::new("palette"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 80.0))
            .show(ctx, |ui| {
                theme::card(&pal).show(ui, |ui| {
                    ui.set_width(560.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.palette.text)
                            .hint_text(tr("Введите команду или имя файла…", "Type a command or a file name…"))
                            .desired_width(f32::INFINITY)
                            .font(egui::FontId::proportional(17.0)),
                    );
                    r.request_focus();
                    if r.changed() {
                        self.palette.sel = 0;
                    }
                    ui.add_space(6.0);
                    for (i, (_, label, hint, _)) in items.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                        if i == self.palette.sel {
                            ui.painter().rect_filled(rect, 6, pal.accent.gamma_multiply(0.22));
                        } else if resp.hovered() {
                            ui.painter().rect_filled(rect, 6, pal.card_hover);
                        }
                        ui.painter().text(
                            rect.left_center() + vec2(10.0, 0.0),
                            Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(14.5),
                            pal.text,
                        );
                        if !hint.is_empty() && hint.chars().count() < 14 {
                            ui.painter().text(
                                rect.right_center() - vec2(10.0, 0.0),
                                Align2::RIGHT_CENTER,
                                hint,
                                egui::FontId::proportional(12.0),
                                pal.dim,
                            );
                        }
                        if resp.clicked() {
                            chosen = Some(i);
                        }
                    }
                    if items.is_empty() {
                        ui.label(RichText::new(tr("Ничего не найдено", "Nothing found")).color(pal.dim));
                    }
                });
            });
        if let Some(i) = chosen {
            self.palette.open = false;
            match items.swap_remove(i).3 {
                Item::Command(c) => self.run_command(ctx, c),
                Item::Recent(p) => self.open_path(ctx, p.into()),
                Item::Tab(t) => self.active = t,
            }
        }
        // a click outside closes it
        if ctx.input(|i| i.pointer.any_pressed()) {
            let inside = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| {
                let w = 600.0;
                egui::Rect::from_min_size(pos2(screen.center().x - w / 2.0, 70.0), vec2(w, 440.0)).contains(p)
            });
            if !inside {
                self.palette.open = false;
            }
        }
    }

    fn goto_dialog(&mut self, ctx: &egui::Context) {
        let Some(text) = self.goto_ui.clone() else { return };
        let pal = self.pal;
        let hint = match self.tab().and_then(Tab::view) {
            Some(View::Paged(p)) => trf(
                "Страница (1–{0}) или процент, например 40%",
                "Page (1–{0}) or a percentage like 40%",
                &[&p.page_count()],
            ),
            Some(View::Sheet(_)) => tr("Номер строки", "Row number").to_owned(),
            _ => tr("Процент документа, например 40", "Percentage of the document, like 40").to_owned(),
        };
        let mut value = text;
        let mut go = false;
        let mut close = false;
        egui::Window::new(tr("Перейти", "Go to"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 110.0))
            .frame(theme::card(&pal))
            .show(ctx, |ui| {
                ui.label(RichText::new(hint).color(pal.dim));
                let r = ui.add(egui::TextEdit::singleline(&mut value).desired_width(260.0));
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    go = true;
                }
                ui.horizontal(|ui| {
                    if theme::primary(ui, &pal, tr("Перейти", "Go")).clicked() {
                        go = true;
                    }
                    if ui.button(tr("Отмена", "Cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if go {
            self.go_to_text(&value);
            self.goto_ui = None;
        } else if close {
            self.goto_ui = None;
        } else {
            self.goto_ui = Some(value);
        }
    }

    fn link_dialog(&mut self, ctx: &egui::Context) {
        let Some(url) = self.link_confirm.clone() else { return };
        let pal = self.pal;
        let mut decision: Option<bool> = None;
        egui::Window::new(tr("Открыть ссылку?", "Open this link?"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(theme::card(&pal))
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.label(
                    RichText::new(tr(
                        "Документ ведёт на внешний адрес:",
                        "The document points to an external address:",
                    ))
                    .color(pal.dim),
                );
                ui.add(egui::Label::new(RichText::new(&url).color(pal.link).strong()).wrap());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::primary(ui, &pal, tr("Открыть в браузере", "Open in the browser")).clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button(tr("Копировать", "Copy")).clicked() {
                        ctx.copy_text(url.clone());
                        decision = Some(false);
                    }
                    if ui.button(tr("Отмена", "Cancel")).clicked() {
                        decision = Some(false);
                    }
                });
            });
        match decision {
            Some(true) => {
                if !winint::open_url(&url) {
                    self.toast(tr("Эту ссылку открыть нельзя", "This link cannot be opened"));
                }
                self.link_confirm = None;
            }
            Some(false) => self.link_confirm = None,
            None => {}
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|t| t.until > std::time::Instant::now());
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
        let pal = self.pal;
        egui::Area::new(Id::new("toasts"))
            .order(egui::Order::Tooltip)
            .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -42.0))
            .show(ctx, |ui| {
                for t in &self.toasts {
                    Frame::new()
                        .fill(pal.card)
                        .stroke(Stroke::new(1.0, pal.accent.gamma_multiply(0.7)))
                        .corner_radius(10)
                        .inner_margin(Margin::symmetric(14, 9))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&t.text).color(pal.text));
                        });
                    ui.add_space(6.0);
                }
            });
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let pal = self.pal;
        let mut open = true;
        egui::Window::new(tr("О программе", "About"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(theme::card(&pal))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Folio").size(30.0).strong().color(pal.text));
                    ui.label(
                        RichText::new(format!("{} {}", tr("версия", "version"), env!("CARGO_PKG_VERSION")))
                            .color(pal.dim),
                    );
                    ui.add_space(6.0);
                    ui.label(tr(
                        "Читалка документов: PDF, Word, EPUB, FB2, таблицы, презентации, код.",
                        "A document reader: PDF, Word, EPUB, FB2, spreadsheets, presentations, code.",
                    ));
                    ui.label(
                        RichText::new(tr(
                            "Рендер PDF — pdfium (Google), лицензия BSD-3.",
                            "PDF rendering by pdfium (Google), BSD-3 licence.",
                        ))
                        .color(pal.dim)
                        .size(12.5),
                    );
                    ui.add_space(8.0);
                    ui.hyperlink_to("github.com/ahsoggg/folio", "https://github.com/ahsoggg/folio");
                });
            });
        self.about_open = open;
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let pal = self.pal;
        let mut open = true;
        let mut changed = false;
        let mut do_check = false;
        let mut do_register: Option<(bool, bool)> = None;
        let mut do_unregister = false;
        let mut do_install = false;
        let mut do_uninstall = false;
        let mut clear_history = false;
        let mut copy_diag = false;
        let env = installer::Env::real();
        let state = shell::state(&env);
        let running_installed = std::env::current_exe()
            .map(|e| installer::is_installed_copy(&env, &e))
            .unwrap_or(false);
        egui::Window::new(tr("Настройки", "Settings"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(theme::card(&pal))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(ctx.content_rect().height() - 140.0).show(ui, |ui| {
                    ui.set_width(450.0);
                    let heading = |ui: &mut Ui, t: &str| {
                        ui.add_space(6.0);
                        ui.label(RichText::new(t).strong().size(15.5).color(pal.accent));
                    };
                    heading(ui, tr("Внешний вид", "Appearance"));
                    egui::Grid::new("set_look").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                        ui.label(tr("Язык", "Language"));
                        ui.horizontal(|ui| {
                            for (code, name) in [("ru", "Русский"), ("en", "English")] {
                                if ui.selectable_label(self.settings.language == code, name).clicked() {
                                    self.settings.language = code.into();
                                    changed = true;
                                }
                            }
                        });
                        ui.end_row();
                        ui.label(tr("Тема", "Theme"));
                        ui.horizontal(|ui| {
                            for t in ThemeId::ALL {
                                if ui.selectable_label(self.settings.theme == t, theme::theme_name(t)).clicked() {
                                    self.settings.theme = t;
                                    changed = true;
                                }
                            }
                        });
                        ui.end_row();
                        ui.label(tr("Шрифт чтения", "Reading font"));
                        ui.horizontal(|ui| {
                            if ui.selectable_label(self.settings.font == ReadingFont::Serif, tr("С засечками (Georgia)", "Serif (Georgia)")).clicked() {
                                self.settings.font = ReadingFont::Serif;
                                changed = true;
                            }
                            if ui.selectable_label(self.settings.font == ReadingFont::Sans, tr("Без засечек (Segoe UI)", "Sans (Segoe UI)")).clicked() {
                                self.settings.font = ReadingFont::Sans;
                                changed = true;
                            }
                        });
                        ui.end_row();
                        ui.label(tr("Размер текста", "Text size"));
                        changed |= ui.add(egui::Slider::new(&mut self.settings.font_size, 12.0..=36.0).suffix(" pt")).changed();
                        ui.end_row();
                        ui.label(tr("Межстрочный интервал", "Line spacing"));
                        changed |= ui.add(egui::Slider::new(&mut self.settings.line_spacing, 1.1..=2.2)).changed();
                        ui.end_row();
                        ui.label(tr("Ширина колонки", "Column width"));
                        changed |= ui.add(egui::Slider::new(&mut self.settings.column_width, 480.0..=1200.0)).changed();
                        ui.end_row();
                    });
                    changed |= ui.checkbox(&mut self.settings.justify, tr("Выравнивать текст по ширине", "Justify text")).changed();
                    changed |= ui
                        .checkbox(&mut self.settings.tone_pdf, tr("Подстраивать страницы PDF под тему (тёмные ночью)", "Tone PDF pages to the theme (dark at night)"))
                        .changed();

                    heading(ui, tr("Поведение", "Behaviour"));
                    changed |= ui.checkbox(&mut self.settings.restore_tabs, tr("Открывать прошлые вкладки при запуске", "Reopen the previous tabs on start")).changed();
                    changed |= ui
                        .checkbox(&mut self.settings.single_window, tr("Одно окно: новые файлы открываются в нём", "One window: new files open in it"))
                        .changed();
                    ui.horizontal(|ui| {
                        ui.label(tr("Скорость чтения вслух", "Reading speed"));
                        changed |= ui.add(egui::Slider::new(&mut self.settings.speech_rate, -6..=8)).changed();
                    });

                    heading(ui, tr("Интеграция с Windows", "Windows integration"));
                    ui.label(
                        RichText::new(tr(
                            "Folio появится в «Открыть с помощью» и в Параметры → Приложения по умолчанию. Назначить программой по умолчанию можно там — Windows не позволяет делать это за вас.",
                            "Folio shows up in “Open with” and in Settings → Default apps. Making it the default is done there — Windows does not let programs do it for you.",
                        ))
                        .color(pal.dim)
                        .size(12.5),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if !state.open_with {
                            if ui.button(tr("Добавить в «Открыть с помощью»", "Add to “Open with”")).clicked() {
                                do_register = Some((true, state.context_menu));
                            }
                        } else if ui.button(tr("Убрать из «Открыть с помощью»", "Remove from “Open with”")).clicked() {
                            do_unregister = true;
                        }
                        if ui.button(tr("Приложения по умолчанию…", "Default apps…")).clicked() {
                            shell::open_default_apps_settings();
                        }
                    });
                    let mut menu = state.context_menu;
                    if ui.checkbox(&mut menu, tr("Пункт «Читать в Folio» в меню правой кнопки", "“Read in Folio” in the right-click menu")).changed() {
                        if menu {
                            do_register = Some((true, true));
                        } else {
                            do_unregister = true;
                            do_register = state.open_with.then_some((true, false));
                        }
                    }
                    ui.horizontal(|ui| {
                        if running_installed {
                            if ui.button(tr("Удалить Folio…", "Uninstall Folio…")).clicked() {
                                do_uninstall = true;
                            }
                        } else if ui.button(tr("Установить для этого пользователя", "Install for this user")).clicked() {
                            do_install = true;
                        }
                    });

                    heading(ui, tr("Обновления", "Updates"));
                    changed |= ui.checkbox(&mut self.settings.check_updates, tr("Раз в сутки проверять новую версию на GitHub", "Check GitHub for a new version once a day")).changed();
                    ui.horizontal(|ui| {
                        ui.label(format!("{} {}", tr("Версия", "Version"), env!("CARGO_PKG_VERSION")));
                        if ui.button(tr("Проверить сейчас", "Check now")).clicked() {
                            do_check = true;
                        }
                        let msg = match &self.update {
                            UpdateState::Checking(_) => tr("проверяю…", "checking…").to_owned(),
                            UpdateState::UpToDate => tr("установлена последняя", "you have the latest").to_owned(),
                            UpdateState::Available(r) => trf("доступна {0}", "{0} is available", &[&r.tag]),
                            UpdateState::Failed(e) => format!("{}: {}", tr("ошибка", "error"), e),
                            _ => String::new(),
                        };
                        ui.label(RichText::new(msg).color(pal.dim));
                    });

                    heading(ui, tr("Данные", "Data"));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(tr("Очистить историю чтения", "Clear reading history")).clicked() {
                            clear_history = true;
                        }
                        if ui.button(tr("Папка с данными", "Data folder")).clicked() {
                            let _ = std::fs::create_dir_all(crate::settings::data_dir());
                            winint::open_with_default(&crate::settings::data_dir());
                        }
                        if ui.button(tr("Скопировать диагностику", "Copy diagnostics")).clicked() {
                            copy_diag = true;
                        }
                    });
                });
            });
        self.settings_open = open;
        if changed {
            self.settings.save();
        }
        if do_check {
            self.check_updates(ctx);
        }
        if do_unregister {
            shell::unregister(&env);
        }
        if let Some((ow, cm)) = do_register {
            match std::env::current_exe() {
                Ok(exe) => {
                    let target = if running_installed { exe } else { env.installed_exe() };
                    let target = if target.exists() {
                        target
                    } else {
                        std::env::current_exe().unwrap_or_default()
                    };
                    match shell::register(&env, &target, ow, cm) {
                        Ok(()) => self.toast(tr("Готово", "Done")),
                        Err(e) => self.toast(format!("{}: {e}", tr("Не удалось", "Failed"))),
                    }
                }
                Err(e) => self.toast(e.to_string()),
            }
        }
        if do_install {
            match std::env::current_exe()
                .map_err(|e| e.to_string())
                .and_then(|exe| installer::install(&env, &exe))
            {
                Ok(p) => self.toast(trf("Установлено: {0}", "Installed: {0}", &[&p.display()])),
                Err(e) => self.toast(format!("{}: {e}", tr("Не удалось установить", "Install failed"))),
            }
        }
        if do_uninstall
            && winint::message_box(
                "Folio",
                tr("Удалить Folio с этого компьютера?", "Remove Folio from this computer?"),
                true,
            )
        {
            let exe = std::env::current_exe().ok();
            match installer::uninstall(&env, exe.as_deref()) {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => self.toast(e),
            }
        }
        if clear_history {
            self.library.entries.clear();
            self.library.save();
            self.toast(tr("История очищена", "History cleared"));
        }
        if copy_diag {
            ctx.copy_text(diag::diagnostic_text(&self.settings));
            self.toast(tr("Скопировано в буфер", "Copied to the clipboard"));
        }
    }
}

fn plural_ru_or(n: usize, one: &'static str, few: &'static str, many: &'static str, en: &'static str) -> &'static str {
    if crate::i18n::lang() == crate::i18n::Lang::Ru {
        plural_ru(n, one, few, many)
    } else {
        en
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matches_subsequences_and_ranks_tighter_matches_higher() {
        assert!(fuzzy("Открыть файл", "откр").is_some());
        assert!(fuzzy("Открыть файл", "ф").is_some());
        assert!(fuzzy("Открыть файл", "zzz").is_none());
        let tight = fuzzy("Theme: Light", "light").unwrap();
        let loose = fuzzy("Toggle bookmark light", "light").unwrap();
        assert!(tight >= loose - 40);
        assert!(fuzzy("Find in document", "fd").unwrap() < fuzzy("Find in document", "find").unwrap());
    }

    #[test]
    fn every_command_has_a_label_in_both_languages() {
        for lang in [crate::i18n::Lang::Ru, crate::i18n::Lang::En] {
            crate::i18n::set_lang(lang);
            for (label, _, _) in commands() {
                assert!(!label.trim().is_empty());
            }
        }
        crate::i18n::set_lang(crate::i18n::Lang::Ru);
    }
}
