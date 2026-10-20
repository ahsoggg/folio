//! The window: tabs, toolbar, sidebar, status bar and the document views.

mod chrome;
pub mod demo;
mod doc;
mod flowview;
mod paged;
mod sheetview;
mod shots;
mod sidebar;
mod textlayout;
pub mod theme;

use crate::i18n::{self, tr, trf};
use crate::library::{Library, Position};
use crate::settings::{ReadingFont, Settings, ThemeId};
use crate::speech::{self, Speaker};
use crate::{diag, single, timeutil, update, winint};
use doc::{State, Tab, View};
use eframe::egui::{self, Key, Modifiers};
use flowview::{FlowEvent, FlowView};
use folio_core::pdf::Tone;
use paged::{PageEvent, PagedView};
use sheetview::SheetView;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use theme::Palette;

pub struct StartOptions {
    pub open: Vec<PathBuf>,
    pub shots: Option<PathBuf>,
    pub guard: Option<single::Guard>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SideTab {
    Contents,
    Pages,
    Search,
    Bookmarks,
    Info,
}

impl SideTab {
    fn code(self) -> &'static str {
        match self {
            SideTab::Contents => "contents",
            SideTab::Pages => "pages",
            SideTab::Search => "search",
            SideTab::Bookmarks => "bookmarks",
            SideTab::Info => "info",
        }
    }

    fn from_code(c: &str) -> SideTab {
        match c {
            "pages" => SideTab::Pages,
            "search" => SideTab::Search,
            "bookmarks" => SideTab::Bookmarks,
            "info" => SideTab::Info,
            _ => SideTab::Contents,
        }
    }
}

#[derive(Default)]
pub struct FindUi {
    pub open: bool,
    pub text: String,
    pub case: bool,
    pub focus: bool,
    pub applied: String,
    pub applied_case: bool,
    pub changed_at: Option<Instant>,
}

#[derive(Default)]
pub struct PaletteUi {
    pub open: bool,
    pub text: String,
    pub sel: usize,
}

pub struct Toast {
    pub text: String,
    pub until: Instant,
}

pub enum UpdateState {
    Idle,
    Checking(std::sync::mpsc::Receiver<Result<Option<update::Release>, String>>),
    UpToDate,
    Available(update::Release),
    Downloading {
        rx: std::sync::mpsc::Receiver<Result<(), String>>,
        done: Arc<AtomicU64>,
        total: Arc<AtomicU64>,
        tag: String,
    },
    Ready(String),
    Failed(String),
}

pub struct Speech {
    pub tab_id: u64,
    pub unit: usize,
    pub russian: bool,
    /// The "no voice for this language" notice has been shown for this reading.
    pub warned: bool,
}

pub struct App {
    pub settings: Settings,
    pub library: Library,
    pub pal: Palette,
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_id: u64,
    pub side_tab: SideTab,
    pub find: FindUi,
    pub palette: PaletteUi,
    pub goto_ui: Option<String>,
    pub settings_open: bool,
    pub about_open: bool,
    pub toasts: Vec<Toast>,
    pub update: UpdateState,
    pub speaker: Speaker,
    pub speech: Option<Speech>,
    pub fullscreen: bool,
    pub shots: Option<shots::Runner>,
    last_save: Instant,
    pub link_confirm: Option<String>,
    cjk_loaded: bool,
    theme_key: Option<(ThemeId, ReadingFont)>,
    lang_key: String,
    _guard: Option<single::Guard>,
    pub sandbox: bool,
    last_size: Option<[f32; 2]>,
    last_title: String,
}

pub fn run(opts: StartOptions) -> eframe::Result {
    let icon = egui::IconData {
        rgba: include_bytes!("../../assets/icon64.rgba").to_vec(),
        width: 64,
        height: 64,
    };
    let shots = opts.shots.is_some();
    let settings = Settings::load();
    let size = settings.window_size.unwrap_or([1180.0, 780.0]);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Folio")
        .with_inner_size(if shots { [1280.0, 800.0] } else { size })
        .with_min_inner_size([720.0, 460.0])
        .with_icon(Arc::new(icon))
        .with_drag_and_drop(true);
    if shots {
        // far outside every monitor, without focus or a taskbar button: nothing shows on the user's desktop
        viewport = viewport
            .with_position([-5000.0, -3000.0])
            .with_taskbar(false)
            .with_active(false)
            .with_resizable(false);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Folio",
        options,
        Box::new(move |cc| {
            if !shots {
                let ctx = cc.egui_ctx.clone();
                single::watch(move || ctx.request_repaint());
            }
            let app = App::new(&cc.egui_ctx, opts, settings);
            Ok(Box::new(app))
        }),
    )
}

impl App {
    pub fn new(ctx: &egui::Context, opts: StartOptions, settings: Settings) -> App {
        i18n::set_lang(i18n::Lang::from_code(&settings.language));
        let pal = theme::palette(settings.theme);
        theme::install(ctx, &pal);
        theme::install_fonts(ctx, false);
        let shot_mode = opts.shots.is_some();
        let library = if shot_mode { Library::default() } else { Library::load() };
        let side_tab = SideTab::from_code(&settings.sidebar_tab);
        let mut app = App {
            theme_key: Some((settings.theme, settings.font)),
            lang_key: settings.language.clone(),
            settings,
            library,
            pal,
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            side_tab,
            find: FindUi::default(),
            palette: PaletteUi::default(),
            goto_ui: None,
            settings_open: false,
            about_open: false,
            toasts: Vec::new(),
            update: UpdateState::Idle,
            speaker: Speaker::new(),
            speech: None,
            fullscreen: false,
            shots: opts.shots.clone().map(shots::Runner::new),
            last_save: Instant::now(),
            link_confirm: None,
            cjk_loaded: false,
            _guard: opts.guard,
            sandbox: shot_mode,
            last_size: None,
            last_title: String::new(),
        };
        let mut to_open = opts.open;
        if to_open.is_empty() && app.settings.restore_tabs && !shot_mode {
            to_open = app
                .settings
                .open_tabs
                .iter()
                .map(PathBuf::from)
                .filter(|p| p.is_file())
                .collect();
        }
        for p in to_open {
            app.open_path(ctx, p);
        }
        if !shot_mode {
            app.maybe_check_updates(ctx);
        }
        app
    }

    // ---- opening and closing ----

    pub fn open_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let path = path
            .to_str()
            .and_then(|s| s.strip_prefix(r"\\?\"))
            .map(PathBuf::from)
            .unwrap_or(path);
        let key = crate::library::key(&path);
        if let Some(i) = self.tabs.iter().position(|t| t.key.eq_ignore_ascii_case(&key)) {
            self.active = i;
            return;
        }
        if !path.is_file() {
            self.toast(trf("Файл не найден: {0}", "File not found: {0}", &[&path.display()]));
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut tab = Tab::new(id, path.clone());
        if let Some(e) = self.library.get(&key) {
            tab.restore = Some((e.pos, e.zoom));
        }
        tab.state = State::Loading(doc::spawn_load(ctx, path, None));
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    pub fn open_dialog(&mut self, ctx: &egui::Context) {
        let mut dlg = rfd::FileDialog::new().set_title(tr("Открыть документ", "Open a document"));
        let all: Vec<&str> = folio_core::supported_groups()
            .into_iter()
            .flat_map(|(_, e)| e)
            .collect();
        dlg = dlg.add_filter(tr("Все документы", "All documents"), &all);
        for (name, exts) in folio_core::supported_groups() {
            dlg = dlg.add_filter(name, &exts);
        }
        if let Some(files) = dlg.pick_files() {
            for f in files {
                self.open_path(ctx, f);
            }
        }
    }

    pub fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.save_position(i);
        if self.speech.as_ref().is_some_and(|s| s.tab_id == self.tabs[i].id) {
            self.stop_speech();
        }
        self.tabs.remove(i);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        } else if i < self.active {
            self.active -= 1;
        }
        self.library.save();
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast {
            text: text.into(),
            until: Instant::now() + Duration::from_secs(4),
        });
    }

    pub fn tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    // ---- loading results ----

    fn poll_loads(&mut self, ctx: &egui::Context) {
        for i in 0..self.tabs.len() {
            let State::Loading(rx) = &self.tabs[i].state else {
                continue;
            };
            let Ok(result) = rx.try_recv() else { continue };
            let size = self.settings.font_size;
            let _ = size;
            match result {
                Ok(ok) => {
                    let tab = &mut self.tabs[i];
                    tab.title = ok.title.clone();
                    tab.kind = Some(ok.info.kind);
                    let id = tab.id;
                    let view = match ok.content {
                        doc::Loaded::Flow(f) => {
                            if !self.cjk_loaded && flow_has_cjk(&f) {
                                self.cjk_loaded = true;
                                theme::install_fonts(ctx, true);
                            }
                            View::Flow(FlowView::new(f, id))
                        }
                        doc::Loaded::Sheets(s) => View::Sheet(SheetView::new(s, id)),
                        doc::Loaded::Images(p) => View::Paged(PagedView::images(p, id)),
                        doc::Loaded::Pdf { engine, doc, info } => {
                            View::Paged(PagedView::pdf(engine, doc, info.pages.clone(), id))
                        }
                    };
                    let tab = &mut self.tabs[i];
                    tab.info = Some(ok.info);
                    tab.state = State::Ready(Box::new(view));
                    if let Some((pos, zoom)) = tab.restore.take() {
                        if let (Some(View::Paged(p)), Some(z)) = (tab.view_mut(), zoom) {
                            p.set_zoom_percent(z);
                        }
                        if pos.index > 0 || pos.frac > 0.0 {
                            tab.goto(pos);
                        }
                    }
                    let (path, title, kind) = {
                        let t = &self.tabs[i];
                        (t.key.clone(), t.title.clone(), t.kind.map(|k| k.label()).unwrap_or(""))
                    };
                    if !self.sandbox {
                        self.library.touch(&path, &title, kind, timeutil::now());
                        self.library.save();
                    }
                    diag::info(&format!("opened {kind}"));
                }
                Err(doc::LoadErr::NeedPassword) => {
                    self.tabs[i].state = State::NeedPassword {
                        input: String::new(),
                        wrong: false,
                    }
                }
                Err(doc::LoadErr::Failed(m)) => {
                    diag::warn(&format!("open failed: {m}"));
                    self.tabs[i].state = State::Failed(m);
                }
            }
        }
    }

    pub fn retry_with_password(&mut self, ctx: &egui::Context, i: usize, password: String) {
        let path = self.tabs[i].path.clone();
        self.tabs[i].state = State::Loading(doc::spawn_load(ctx, path, Some(password)));
    }

    // ---- positions ----

    pub fn save_position(&mut self, i: usize) {
        if self.sandbox {
            return;
        }
        let Some(tab) = self.tabs.get(i) else { return };
        let (Some(pos), Some(progress)) = (tab.position(), tab.progress()) else {
            return;
        };
        let zoom = match tab.view() {
            Some(View::Paged(p)) if p.is_pdf => Some(p.percent()),
            _ => None,
        };
        let key = tab.key.clone();
        if let Some(e) = self.library.entry_mut(&key) {
            e.pos = pos;
            e.progress = progress;
            e.zoom = zoom;
        }
    }

    fn periodic_save(&mut self) {
        if self.last_save.elapsed() < Duration::from_secs(6) {
            return;
        }
        self.last_save = Instant::now();
        if let Some(i) = (!self.tabs.is_empty()).then_some(self.active) {
            self.save_position(i);
        }
        if !self.sandbox {
            self.library.save();
        }
    }

    pub fn toggle_bookmark(&mut self) {
        let Some(tab) = self.tab() else { return };
        let Some(pos) = tab.position() else { return };
        let label = tab.label_at(pos);
        let key = tab.key.clone();
        let near = |a: Position, b: Position| a.index == b.index;
        let added = self.library.toggle_bookmark(&key, pos, &label, timeutil::now(), near);
        self.library.save();
        self.toast(if added {
            tr("Закладка добавлена", "Bookmark added")
        } else {
            tr("Закладка удалена", "Bookmark removed")
        });
    }

    // ---- appearance ----

    pub fn apply_look(&mut self, ctx: &egui::Context) {
        let key = Some((self.settings.theme, self.settings.font));
        if self.theme_key != key {
            self.theme_key = key;
            self.pal = theme::palette(self.settings.theme);
            theme::install(ctx, &self.pal);
            for t in &mut self.tabs {
                if let Some(View::Paged(p)) = t.view_mut() {
                    p.drop_thumbnails();
                }
            }
        }
        if self.lang_key != self.settings.language {
            self.lang_key = self.settings.language.clone();
            i18n::set_lang(i18n::Lang::from_code(&self.settings.language));
        }
    }

    pub fn look(&self) -> textlayout::Look {
        textlayout::Look {
            pal: self.pal,
            base: theme::reading_base(self.settings.font),
            size: self.settings.font_size,
            spacing: self.settings.line_spacing,
            justify: self.settings.justify,
        }
    }

    pub fn pdf_tone(&self) -> Tone {
        if !self.settings.tone_pdf {
            return Tone::Paper;
        }
        match self.settings.theme {
            ThemeId::Mocha | ThemeId::Night => Tone::Dark,
            ThemeId::Sepia => Tone::Sepia,
            ThemeId::Light => Tone::Paper,
        }
    }

    pub fn set_theme(&mut self, t: ThemeId) {
        self.settings.theme = t;
        self.settings.save();
    }

    pub fn font_step(&mut self, delta: f32) {
        self.settings.font_size = (self.settings.font_size + delta).clamp(11.0, 40.0);
        self.settings.save();
    }

    // ---- find ----

    pub fn open_find(&mut self) {
        self.find.open = true;
        self.find.focus = true;
        self.side_tab = SideTab::Search;
    }

    pub fn close_find(&mut self) {
        self.find.open = false;
        self.find.text.clear();
        self.apply_find();
    }

    pub fn apply_find(&mut self) {
        let (q, case) = (self.find.text.clone(), self.find.case);
        self.find.applied = q.clone();
        self.find.applied_case = case;
        self.find.changed_at = None;
        if let Some(tab) = self.tabs.get_mut(self.active) {
            match tab.view_mut() {
                Some(View::Flow(f)) => f.start_search(&q),
                Some(View::Paged(p)) => p.start_search(&q, case),
                Some(View::Sheet(s)) => s.start_search(&q),
                None => {}
            }
        }
    }

    pub fn find_step(&mut self, forward: bool) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            match tab.view_mut() {
                Some(View::Flow(f)) => f.next_hit(forward),
                Some(View::Paged(p)) => p.next_hit(forward),
                Some(View::Sheet(s)) => s.next_hit(forward),
                None => {}
            }
        }
    }

    /// (current, total, still searching) for the active document.
    pub fn find_status(&self) -> (usize, usize, bool) {
        match self.tab().and_then(Tab::view) {
            Some(View::Flow(f)) => (f.search.current, f.search.hits.len(), f.search.is_running()),
            Some(View::Paged(p)) => (p.search.current, p.search.hits.len(), p.is_searching()),
            Some(View::Sheet(s)) => (s.search.current, s.search.hits.len(), s.is_searching()),
            None => (0, 0, false),
        }
    }

    // ---- speech ----

    pub fn can_speak(&self) -> bool {
        matches!(
            self.tab().and_then(Tab::view),
            Some(View::Flow(_)) | Some(View::Paged(_))
        ) && self
            .tab()
            .is_some_and(|t| t.kind != Some(folio_core::Kind::Image) && t.kind != Some(folio_core::Kind::Comic))
    }

    pub fn toggle_speech(&mut self) {
        if self.speech.is_some() {
            if self.speaker.paused {
                self.speaker.resume();
            } else if self.speaker.is_speaking() {
                self.speaker.pause();
            } else {
                self.stop_speech();
            }
            return;
        }
        let Some(tab) = self.tab() else { return };
        let (id, start) = match tab.view() {
            Some(View::Flow(f)) => (tab.id, f.top.index),
            Some(View::Paged(p)) if p.is_pdf => (tab.id, p.current_page),
            _ => return,
        };
        self.speech = Some(Speech {
            tab_id: id,
            unit: start,
            russian: false,
            warned: false,
        });
        self.speak_unit();
    }

    pub fn stop_speech(&mut self) {
        self.speaker.stop();
        self.speech = None;
        for t in &mut self.tabs {
            if let Some(View::Flow(f)) = t.view_mut() {
                f.speaking = None;
            }
        }
    }

    /// Starts speaking the current unit (a block or a page); moves on over units without text.
    fn speak_unit(&mut self) {
        let rate = self.settings.speech_rate;
        let Some(sp) = self.speech.as_mut() else { return };
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == sp.tab_id) else {
            self.speech = None;
            return;
        };
        loop {
            let (text, total, flow_block) = match tab.view_mut() {
                Some(View::Flow(f)) => {
                    let n = f.flow.blocks.len();
                    if sp.unit >= n {
                        (None, n, None)
                    } else {
                        let b = &f.flow.blocks[sp.unit];
                        let speakable = matches!(
                            b,
                            folio_core::Block::Heading { .. }
                                | folio_core::Block::Paragraph { .. }
                                | folio_core::Block::ListItem { .. }
                                | folio_core::Block::Quote(_)
                        );
                        (
                            Some(if speakable { b.plain_text() } else { String::new() }),
                            n,
                            Some(sp.unit),
                        )
                    }
                }
                Some(View::Paged(p)) => {
                    let n = p.page_count();
                    if sp.unit >= n {
                        (None, n, None)
                    } else {
                        (Some(p.page_text(sp.unit)), n, None)
                    }
                }
                _ => (None, 0, None),
            };
            let Some(text) = text else {
                let _ = total;
                self.speech = None;
                self.speaker.stop();
                self.toast(tr("Чтение вслух закончено", "Reading aloud finished"));
                return;
            };
            let clean = speech::clean_for_speech(&text);
            if clean.trim().is_empty() {
                sp.unit += 1;
                continue;
            }
            sp.russian = speech::is_russian(&clean);
            match self.speaker.speak(&clean, sp.russian, rate) {
                Ok(found) => {
                    if !found && !sp.warned {
                        sp.warned = true;
                        let note = if sp.russian {
                            tr(
                                "Русский голос не найден: читает голос по умолчанию. Добавьте его в Параметры → Время и язык → Речь",
                                "No Russian voice found: the default voice reads. Add one in Settings → Time & language → Speech",
                            )
                        } else {
                            tr(
                                "Голос для этого языка не найден: читает голос по умолчанию",
                                "No voice for this language: the default voice reads",
                            )
                        };
                        self.toasts.push(Toast {
                            text: note.to_owned(),
                            until: Instant::now() + Duration::from_secs(7),
                        });
                    }
                }
                Err(e) => {
                    diag::warn(&format!("speech failed: {e}"));
                    self.speech = None;
                    self.toast(tr(
                        "Не удалось запустить синтезатор речи",
                        "The speech engine could not be started",
                    ));
                    return;
                }
            }
            match tab.view_mut() {
                Some(View::Flow(f)) => {
                    f.speaking = flow_block;
                    // keep the spoken block on the screen
                    let unit = sp.unit;
                    let top = f.top.index;
                    if unit < top || unit > top + 12 {
                        f.goto(unit, 0.0);
                    }
                }
                Some(View::Paged(p)) => {
                    if p.current_page != sp.unit {
                        p.go_to(sp.unit, 0.0);
                    }
                }
                _ => {}
            }
            return;
        }
    }

    fn tick_speech(&mut self, ctx: &egui::Context) {
        if self.speech.is_none() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        if self.speaker.paused || self.speaker.is_speaking() {
            return;
        }
        if let Some(sp) = self.speech.as_mut() {
            sp.unit += 1;
        }
        self.speak_unit();
    }

    // ---- updates ----

    pub fn maybe_check_updates(&mut self, ctx: &egui::Context) {
        if !update::due(&self.settings, timeutil::now()) {
            return;
        }
        self.check_updates(ctx);
    }

    pub fn check_updates(&mut self, ctx: &egui::Context) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::check_latest());
            ctx.request_repaint();
        });
        self.update = UpdateState::Checking(rx);
        self.settings.last_update_check = timeutil::now();
        self.settings.save();
    }

    pub fn start_update(&mut self, ctx: &egui::Context, rel: update::Release) {
        let Ok(current) = std::env::current_exe() else {
            self.update = UpdateState::Failed("cannot locate the running executable".into());
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let (done, total) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        let (d2, t2) = (done.clone(), total.clone());
        let tag = rel.tag.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let r = update::download_verified(&rel, &current, &|d, t| {
                d2.store(d, Ordering::Relaxed);
                t2.store(t, Ordering::Relaxed);
                ctx2.request_repaint();
            })
            .and_then(|new| update::apply(&new, &current));
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
        self.update = UpdateState::Downloading { rx, done, total, tag };
    }

    fn poll_update(&mut self, ctx: &egui::Context) {
        match &self.update {
            UpdateState::Checking(rx) => {
                if let Ok(r) = rx.try_recv() {
                    self.update = match r {
                        Ok(Some(rel)) => UpdateState::Available(rel),
                        Ok(None) => UpdateState::UpToDate,
                        Err(e) => {
                            diag::warn(&format!("update check failed: {e}"));
                            UpdateState::Failed(e)
                        }
                    };
                }
            }
            UpdateState::Downloading { rx, tag, .. } => {
                if let Ok(r) = rx.try_recv() {
                    let tag = tag.clone();
                    self.update = match r {
                        Ok(()) => {
                            diag::info(&format!("updated to {tag}"));
                            UpdateState::Ready(tag)
                        }
                        Err(e) => {
                            diag::error(&format!("update failed: {e}"));
                            UpdateState::Failed(e)
                        }
                    };
                } else {
                    ctx.request_repaint_after(Duration::from_millis(200));
                }
            }
            _ => {}
        }
    }

    pub fn restart(&mut self, ctx: &egui::Context) {
        if let Ok(exe) = std::env::current_exe() {
            let files: Vec<String> = self
                .tabs
                .iter()
                .map(|t| t.path.to_string_lossy().into_owned())
                .collect();
            let _ = std::process::Command::new(exe).arg("--restart").args(files).spawn();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // ---- incoming files ----

    fn poll_inbox(&mut self, ctx: &egui::Context) {
        if self.sandbox {
            return;
        }
        if let Some(paths) = single::drain() {
            for p in single::existing(paths) {
                self.open_path(ctx, p);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        for p in dropped {
            if p.is_file() {
                self.open_path(ctx, p);
            }
        }
    }

    // ---- keys ----

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let (cmd, shift) = (Modifiers::COMMAND, Modifiers::SHIFT);
        let key = |ctx: &egui::Context, m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if key(ctx, cmd, Key::O) {
            self.open_dialog(ctx);
        }
        if key(ctx, cmd, Key::K) || key(ctx, cmd | shift, Key::P) {
            self.palette.open = !self.palette.open;
            self.palette.text.clear();
            self.palette.sel = 0;
        }
        if key(ctx, cmd, Key::Comma) {
            self.settings_open = !self.settings_open;
        }
        if key(ctx, cmd, Key::W) {
            self.close_tab(self.active);
        }
        if !self.tabs.is_empty() {
            if key(ctx, Modifiers::CTRL | shift, Key::Tab) {
                self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
            } else if key(ctx, Modifiers::CTRL, Key::Tab) {
                self.active = (self.active + 1) % self.tabs.len();
            }
            for (n, k) in [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ]
            .into_iter()
            .enumerate()
            {
                if key(ctx, Modifiers::ALT, k) && n < self.tabs.len() {
                    self.active = n;
                }
            }
        }
        if key(ctx, cmd, Key::F) {
            self.open_find();
        }
        if key(ctx, Modifiers::NONE, Key::F3) {
            self.find_step(true);
        }
        if key(ctx, shift, Key::F3) {
            self.find_step(false);
        }
        if key(ctx, cmd, Key::G) {
            self.goto_ui = Some(String::new());
        }
        if key(ctx, cmd, Key::B) {
            self.toggle_bookmark();
        }
        if key(ctx, Modifiers::NONE, Key::F11) {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if key(ctx, Modifiers::ALT, Key::ArrowRight) {
            self.open_sibling(ctx, 1);
        }
        if key(ctx, Modifiers::ALT, Key::ArrowLeft) {
            self.open_sibling(ctx, -1);
        }
        if key(ctx, cmd, Key::L) && self.can_speak() {
            self.toggle_speech();
        }
        if key(ctx, cmd, Key::P) {
            if let Some(t) = self.tab() {
                winint::print_file(&t.path);
            }
        }
        if key(ctx, cmd, Key::Backtick) || key(ctx, cmd | shift, Key::T) {
            let next = self.settings.theme.next();
            self.set_theme(next);
        }
        // zoom
        if key(ctx, cmd, Key::Equals) || key(ctx, cmd | shift, Key::Equals) || key(ctx, cmd, Key::Plus) {
            self.zoom_in(1.0);
        }
        if key(ctx, cmd, Key::Minus) {
            self.zoom_in(-1.0);
        }
        if key(ctx, cmd, Key::Num0) {
            self.zoom_reset();
        }
        // text selected in a reflowed document is copied by egui itself: do not take the key away from it
        let own_copy = matches!(
            self.tab().and_then(Tab::view),
            Some(View::Paged(_)) | Some(View::Sheet(_))
        );
        if own_copy && !typing && key(ctx, cmd, Key::C) {
            self.copy_selection(ctx);
        }
        if key(ctx, Modifiers::NONE, Key::Escape) {
            if self.palette.open {
                self.palette.open = false;
            } else if self.goto_ui.is_some() {
                self.goto_ui = None;
            } else if self.link_confirm.is_some() {
                self.link_confirm = None;
            } else if self.settings_open {
                self.settings_open = false;
            } else if self.about_open {
                self.about_open = false;
            } else if self.find.open {
                self.close_find();
            } else if self.fullscreen {
                self.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else if let Some(View::Paged(p)) = self.tab_mut().and_then(Tab::view_mut) {
                p.clear_selection();
            }
        }
        if typing || self.palette.open || self.goto_ui.is_some() {
            return;
        }
        // reading keys
        let page_h = ctx.content_rect().height() * 0.85;
        let mut scroll = 0.0f32;
        let mut to_start = false;
        let mut to_end = false;
        let mut page_step: i32 = 0;
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Space) || i.consume_key(Modifiers::NONE, Key::PageDown) {
                scroll += page_h;
            }
            if i.consume_key(shift, Key::Space) || i.consume_key(Modifiers::NONE, Key::PageUp) {
                scroll -= page_h;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) || i.consume_key(Modifiers::NONE, Key::J) {
                scroll += 70.0;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) || i.consume_key(Modifiers::NONE, Key::K) {
                scroll -= 70.0;
            }
            if i.consume_key(Modifiers::NONE, Key::Home) {
                to_start = true;
            }
            if i.consume_key(Modifiers::NONE, Key::End) {
                to_end = true;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                page_step += 1;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                page_step -= 1;
            }
        });
        if key(ctx, Modifiers::NONE, Key::T) {
            let next = self.settings.theme.next();
            self.set_theme(next);
        }
        if let Some(tab) = self.tab_mut() {
            match tab.view_mut() {
                Some(View::Flow(f)) => {
                    if scroll != 0.0 {
                        f.scroll_by(scroll);
                    }
                    if to_start {
                        f.scroll_to_fraction(0.0);
                    }
                    if to_end {
                        f.scroll_to_fraction(1.0);
                    }
                }
                Some(View::Paged(p)) => {
                    if scroll != 0.0 {
                        p.scroll_by(scroll);
                    }
                    if to_start {
                        p.go_to(0, 0.0);
                    }
                    if to_end {
                        let last = p.page_count().saturating_sub(1);
                        p.go_to(last, 1.0);
                    }
                    if page_step != 0 {
                        let n = (p.current_page as i32 + page_step).clamp(0, p.page_count() as i32 - 1) as usize;
                        p.go_to(n, 0.0);
                    }
                }
                Some(View::Sheet(s)) => {
                    if scroll != 0.0 {
                        let rows = (scroll / 24.0) as i64;
                        s.goto_row((s.top_row as i64 + rows).max(0) as usize);
                    }
                    if to_start {
                        s.goto_row(0);
                    }
                    if to_end {
                        let last = s.sheet().rows.len().saturating_sub(1);
                        s.goto_row(last);
                    }
                }
                None => {}
            }
        }
    }

    pub fn zoom_in(&mut self, dir: f32) {
        let factor = if dir > 0.0 { 1.1 } else { 1.0 / 1.1 };
        let step = if dir > 0.0 { 1.0 } else { -1.0 };
        let font = self.settings.font_size;
        let _ = font;
        if let Some(View::Paged(p)) = self.tab_mut().and_then(Tab::view_mut) {
            p.zoom_step(factor);
        } else {
            self.font_step(step);
        }
    }

    /// Replaces the current tab with the next or previous supported file of the same folder, in natural order.
    pub fn open_sibling(&mut self, ctx: &egui::Context, dir: i32) {
        let Some(tab) = self.tab() else { return };
        let path = tab.path.clone();
        let Some(parent) = path.parent() else { return };
        let known: std::collections::HashSet<String> = folio_core::supported_groups()
            .into_iter()
            .flat_map(|(_, e)| e)
            .map(str::to_owned)
            .collect();
        let mut files: Vec<PathBuf> = std::fs::read_dir(parent)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
                            && p.extension()
                                .is_some_and(|x| known.contains(&x.to_string_lossy().to_ascii_lowercase()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort_by(|a, b| {
            folio_core::natural_cmp(
                &a.file_name().unwrap_or_default().to_string_lossy(),
                &b.file_name().unwrap_or_default().to_string_lossy(),
            )
        });
        let Some(at) = files.iter().position(|p| p == &path) else {
            return;
        };
        let next = at as i64 + dir as i64;
        let Some(target) = (next >= 0).then(|| files.get(next as usize)).flatten().cloned() else {
            self.toast(if dir > 0 {
                tr("Это последний файл в папке", "This is the last file in the folder")
            } else {
                tr("Это первый файл в папке", "This is the first file in the folder")
            });
            return;
        };
        let slot = self.active;
        if let Some(i) = self.tabs.iter().position(|t| t.path == target) {
            // already open elsewhere: switch to it and drop this one
            let id = self.tabs[i].id;
            self.close_tab(slot);
            self.active = self.tabs.iter().position(|t| t.id == id).unwrap_or(0);
            return;
        }
        self.close_tab(slot);
        self.open_path(ctx, target);
        // keep the tab where the old one was
        if let Some(t) = self.tabs.pop() {
            let at = slot.min(self.tabs.len());
            self.tabs.insert(at, t);
            self.active = at;
        }
    }

    /// Saves the current page (PDF or comic) as a PNG picture.
    pub fn save_page_image(&mut self) {
        let Some(tab) = self.tab() else { return };
        let Some(View::Paged(p)) = tab.view() else {
            self.toast(tr("Это работает для PDF и комиксов", "This works for PDFs and comics"));
            return;
        };
        let page = p.current_page;
        let (w_pt, _) = p.sizes[page];
        let name = format!(
            "{}-{}.png",
            tab.path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "page".into()),
            page + 1
        );
        let Some(rendered) = p.render_now(page, ((w_pt * 2.2) as usize).clamp(600, 4000)) else {
            self.toast(tr("Не удалось отрисовать страницу", "The page could not be rendered"));
            return;
        };
        if let Some(dest) = rfd::FileDialog::new()
            .set_file_name(name)
            .add_filter("PNG", &["png"])
            .save_file()
        {
            match image::save_buffer(
                &dest,
                &rendered.rgba,
                rendered.width as u32,
                rendered.height as u32,
                image::ColorType::Rgba8,
            ) {
                Ok(()) => self.toast(trf("Сохранено: {0}", "Saved: {0}", &[&dest.display()])),
                Err(e) => self.toast(trf("Не удалось сохранить: {0}", "Could not save: {0}", &[&e])),
            }
        }
    }

    pub fn zoom_reset(&mut self) {
        if let Some(View::Paged(p)) = self.tab_mut().and_then(Tab::view_mut) {
            p.set_zoom_percent(100.0);
        } else {
            self.settings.font_size = Settings::default().font_size;
            self.settings.save();
        }
    }

    pub fn copy_selection(&mut self, ctx: &egui::Context) {
        let text = match self.tab().and_then(Tab::view) {
            Some(View::Paged(p)) => p.selected_text(),
            Some(View::Sheet(s)) => s.selected_text().unwrap_or_default(),
            _ => String::new(),
        };
        if !text.is_empty() {
            ctx.copy_text(text);
            self.toast(tr("Скопировано", "Copied"));
        }
    }

    pub fn copy_all(&mut self, ctx: &egui::Context) {
        let text = match self.tab().and_then(Tab::view) {
            Some(View::Flow(f)) => f.copy_text(),
            Some(View::Sheet(s)) => s.sheet_tsv(),
            Some(View::Paged(p)) if p.is_pdf => (0..p.page_count())
                .map(|i| p.page_text(i))
                .collect::<Vec<_>>()
                .join("\n\n"),
            _ => String::new(),
        };
        if text.is_empty() {
            self.toast(tr("В этом документе нет текста", "This document has no text"));
        } else {
            ctx.copy_text(text);
            self.toast(tr("Весь текст скопирован", "All text copied"));
        }
    }

    pub fn export_text(&mut self) {
        let Some(tab) = self.tab() else { return };
        let text = match tab.view() {
            Some(View::Flow(f)) => f.copy_text(),
            Some(View::Sheet(s)) => s.sheet_tsv(),
            Some(View::Paged(p)) if p.is_pdf => (0..p.page_count())
                .map(|i| p.page_text(i))
                .collect::<Vec<_>>()
                .join("\n\n"),
            _ => String::new(),
        };
        if text.is_empty() {
            self.toast(tr("В этом документе нет текста", "This document has no text"));
            return;
        }
        let name = format!(
            "{}.txt",
            tab.path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "document".into())
        );
        if let Some(dest) = rfd::FileDialog::new()
            .set_file_name(name)
            .add_filter("Text", &["txt"])
            .save_file()
        {
            match std::fs::write(&dest, text) {
                Ok(()) => self.toast(trf("Сохранено: {0}", "Saved: {0}", &[&dest.display()])),
                Err(e) => self.toast(trf("Не удалось сохранить: {0}", "Could not save: {0}", &[&e])),
            }
        }
    }

    pub fn go_to_text(&mut self, text: &str) {
        let Ok(n) = text.trim().trim_end_matches('%').parse::<f32>() else {
            return;
        };
        let percent_mode = text.trim().ends_with('%');
        if let Some(tab) = self.tab_mut() {
            match tab.view_mut() {
                Some(View::Paged(p)) => {
                    let page = if percent_mode {
                        ((n / 100.0) * p.page_count() as f32) as usize
                    } else {
                        (n as usize).saturating_sub(1)
                    };
                    p.go_to(page, 0.0);
                }
                Some(View::Flow(f)) => f.scroll_to_fraction(n / 100.0),
                Some(View::Sheet(s)) => s.goto_row((n as usize).saturating_sub(1)),
                None => {}
            }
        }
    }

    fn handle_view_events(&mut self, flow: Vec<FlowEvent>, paged: Vec<PageEvent>) {
        for e in flow {
            match e {
                FlowEvent::OpenUrl(u) => self.link_confirm = Some(u),
            }
        }
        for e in paged {
            match e {
                PageEvent::OpenUrl(u) => self.link_confirm = Some(u),
            }
        }
    }

    /// The query the active document's view is currently searching for.
    fn view_query(&self) -> Option<String> {
        match self.tab().and_then(Tab::view)? {
            View::Flow(f) => Some(f.search.query.clone()),
            View::Paged(p) => Some(p.search.query.clone()),
            View::Sheet(s) => Some(s.search.query.clone()),
        }
    }

    fn debounce_find(&mut self, ctx: &egui::Context) {
        // a document that has just loaded or been switched to takes over the search in the box
        if self.find.changed_at.is_none() && self.view_query().is_some_and(|q| q != self.find.applied) {
            let (q, case) = (self.find.applied.clone(), self.find.applied_case);
            if let Some(tab) = self.tabs.get_mut(self.active) {
                match tab.view_mut() {
                    Some(View::Flow(f)) => f.start_search(&q),
                    Some(View::Paged(p)) => p.start_search(&q, case),
                    Some(View::Sheet(s)) => s.start_search(&q),
                    None => {}
                }
            }
        }
        if let Some(t) = self.find.changed_at {
            if t.elapsed() >= Duration::from_millis(220) {
                self.apply_find();
            } else {
                ctx.request_repaint_after(Duration::from_millis(60));
            }
        }
        // results arrive from worker threads
        let mut finished = false;
        if let Some(tab) = self.tabs.get_mut(self.active) {
            match tab.view_mut() {
                Some(View::Flow(f)) => finished = f.poll_search(),
                Some(View::Sheet(s)) => finished = s.poll_search(),
                _ => {}
            }
            if matches!(tab.view(), Some(View::Flow(f)) if f.search.is_running()) {
                ctx.request_repaint_after(Duration::from_millis(60));
            }
        }
        let _ = finished;
    }

    pub fn window_title(&self) -> String {
        match self.tab() {
            Some(t) => format!("{} — Folio", t.title),
            None => "Folio".to_owned(),
        }
    }

    fn save_session(&mut self) {
        if self.sandbox {
            return;
        }
        for i in 0..self.tabs.len() {
            self.save_position(i);
        }
        self.library.save();
        self.settings.open_tabs = self
            .tabs
            .iter()
            .map(|t| t.path.to_string_lossy().into_owned())
            .collect();
        if let Some(sz) = self.last_size {
            self.settings.window_size = Some(sz);
        }
        self.settings.sidebar_tab = self.side_tab.code().to_owned();
        self.settings.save();
    }
}

fn flow_has_cjk(flow: &folio_core::Flow) -> bool {
    flow.blocks.iter().take(400).any(|b| {
        b.plain_text().chars().take(200).any(|c| {
            ('\u{3040}'..='\u{30ff}').contains(&c)
                || ('\u{4e00}'..='\u{9fff}').contains(&c)
                || ('\u{ac00}'..='\u{d7af}').contains(&c)
        })
    })
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}

impl App {
    /// One frame of the whole window; separate from `eframe::App::ui` so tests can drive it.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.apply_look(&ctx);
        self.poll_loads(&ctx);
        self.poll_update(&ctx);
        self.poll_inbox(&ctx);
        self.handle_drops(&ctx);
        self.handle_keys(&ctx);
        self.debounce_find(&ctx);
        self.tick_speech(&ctx);
        ctx.request_repaint_after(Duration::from_millis(400));
        let title = self.window_title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
        let sz = ctx.content_rect().size();
        if !self.fullscreen && sz.x > 300.0 {
            self.last_size = Some([sz.x, sz.y]);
        }

        let zen = self.fullscreen;
        if !zen {
            self.tab_strip(ui);
            self.toolbar(ui);
            if self.find.open {
                self.find_bar(ui);
            }
        }
        if !zen {
            self.status_bar(ui);
        }
        if !zen && self.settings.sidebar_open && self.tab().is_some_and(|t| matches!(t.state, State::Ready(_))) {
            self.sidebar(ui);
        }
        let mut flow_events = Vec::new();
        let mut page_events = Vec::new();
        let paper = self.pal.paper;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(paper))
            .show(ui, |ui| {
                let top = ui.max_rect();
                self.document_area(ui, &mut flow_events, &mut page_events);
                if let Some(p) = self.tab().and_then(Tab::progress) {
                    let bar = egui::Rect::from_min_size(top.min, egui::vec2(top.width() * p, 3.0));
                    ui.painter().rect_filled(bar, 0, self.pal.accent);
                }
            });
        self.handle_view_events(flow_events, page_events);

        self.overlays(&ctx);
        self.periodic_save();
        shots::tick(self, &ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            self.stop_speech();
            self.save_session();
        }
    }
}

#[cfg(test)]
mod tests;
