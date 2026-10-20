//! `Folio.exe --screenshots <dir>`: walks through a fixed set of scenes with made-up documents and saves each one as a
//! PNG, for the README and for checking the look without touching the user's desktop (the window is placed outside
//! every monitor and never takes focus).

use super::demo::{self, Demo};
use super::doc::{State, View};
use super::{App, SideTab};
use crate::library::Position;
use crate::settings::ThemeId;
use eframe::egui;
use std::path::PathBuf;

const SCENES: [&str; 10] = [
    "01-welcome",
    "02-docx",
    "03-epub-light",
    "04-pdf-dark",
    "05-find",
    "06-spreadsheet",
    "07-code",
    "08-palette",
    "09-settings",
    "10-markdown",
];
const SETTLE_FRAMES: u32 = 70;

pub struct Runner {
    dir: PathBuf,
    step: usize,
    frames: u32,
    requested: bool,
    _tmp: tempfile::TempDir,
    demo: Demo,
}

impl Runner {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        let tmp = tempfile::tempdir().expect("temp dir");
        let demo = demo::write_all(tmp.path());
        Self {
            dir,
            step: 0,
            frames: 0,
            requested: false,
            _tmp: tmp,
            demo,
        }
    }
}

fn setup(app: &mut App, ctx: &egui::Context, step: usize, d: &Demo) {
    while !app.tabs.is_empty() {
        app.close_tab(0);
    }
    app.palette.open = false;
    app.settings_open = false;
    app.find.open = false;
    app.find.text.clear();
    app.find.applied.clear();
    app.settings.theme = ThemeId::Mocha;
    app.settings.sidebar_open = true;
    app.side_tab = SideTab::Contents;
    match step {
        0 => {
            app.library.entries.clear();
            for (path, title, kind, progress) in [
                (&d.docx, "Как читать удобно", "Word (DOCX)", 0.42),
                (&d.epub, "Тихое чтение", "EPUB", 0.88),
                (&d.pdf, "Quiet reading", "PDF", 0.1),
                (&d.csv, "Продажи", "CSV", 0.0),
            ] {
                let e = app.library.touch(
                    &path.to_string_lossy(),
                    title,
                    kind,
                    crate::timeutil::now() - 86_400 * (step as i64 + 1),
                );
                e.progress = progress;
            }
        }
        1 => app.open_path(ctx, d.docx.clone()),
        2 => {
            app.settings.theme = ThemeId::Light;
            app.open_path(ctx, d.epub.clone());
        }
        3 => {
            app.side_tab = SideTab::Pages;
            app.open_path(ctx, d.pdf.clone());
        }
        4 => {
            app.open_path(ctx, d.docx.clone());
            app.find.text = "чтени".into();
            app.open_find();
            app.apply_find();
        }
        5 => {
            app.settings.theme = ThemeId::Sepia;
            app.open_path(ctx, d.csv.clone());
        }
        6 => {
            app.settings.theme = ThemeId::Night;
            app.open_path(ctx, d.code.clone());
        }
        7 => {
            app.open_path(ctx, d.docx.clone());
            app.palette.open = true;
            app.palette.text = "тем".into();
        }
        8 => {
            app.open_path(ctx, d.docx.clone());
            app.settings_open = true;
        }
        _ => {
            app.settings.theme = ThemeId::Sepia;
            app.side_tab = SideTab::Info;
            app.open_path(ctx, d.md.clone());
        }
    }
}

fn all_ready(app: &App) -> bool {
    app.tabs.iter().all(|t| !matches!(t.state, State::Loading(_)))
}

pub(super) fn tick(app: &mut App, ctx: &egui::Context) {
    let Some(mut r) = app.shots.take() else { return };
    ctx.request_repaint();
    ctx.global_style_mut(|s| s.animation_time = 0.0);
    if r.step >= SCENES.len() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        app.shots = Some(r);
        return;
    }
    if r.frames == 0 {
        setup(app, ctx, r.step, &r.demo);
    }
    r.frames += 1;
    if r.frames == 12 && r.step == 2 {
        // the middle of the book
        if let Some(t) = app.tab_mut() {
            t.goto(Position { index: 9, frac: 0.0 });
        }
    }
    if r.frames == 12 && r.step == 3 {
        if let Some(View::Paged(p)) = app.tab_mut().and_then(|t| t.view_mut()) {
            p.go_to(1, 0.0);
        }
    }
    if r.frames >= SETTLE_FRAMES && !r.requested && all_ready(app) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        r.requested = true;
    }
    let image = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    });
    if let Some(img) = image {
        let [w, h] = img.size;
        let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
        let path = r.dir.join(format!("{}.png", SCENES[r.step]));
        match image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8) {
            Ok(()) => crate::winint::console_print(&format!("saved {}\n", path.display())),
            Err(e) => crate::winint::console_print(&format!("cannot save {}: {e}\n", path.display())),
        }
        r.step += 1;
        r.frames = 0;
        r.requested = false;
    }
    app.shots = Some(r);
}
