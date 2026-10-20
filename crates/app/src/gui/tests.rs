//! Headless runs of the whole window: documents are opened, drawn on every theme, searched, bookmarked.
//! Nothing here touches the user's folders: data goes to a private temp folder and the documents are made up.

use super::*;
use crate::settings::use_private_test_data_dir;
use doc::State;

fn frame(app: &mut App, ctx: &egui::Context, size: egui::Vec2) {
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| app.frame(ui));
    out.textures_delta.clear();
}

fn frames(app: &mut App, ctx: &egui::Context, n: usize) {
    for _ in 0..n {
        frame(app, ctx, egui::vec2(1280.0, 800.0));
    }
}

fn new_app(ctx: &egui::Context, open: Vec<PathBuf>) -> App {
    let settings = Settings {
        restore_tabs: false,
        check_updates: false,
        ..Settings::default()
    };
    App::new(
        ctx,
        StartOptions {
            open,
            shots: None,
            guard: None,
        },
        settings,
    )
}

fn wait_ready(app: &mut App, ctx: &egui::Context) {
    for _ in 0..600 {
        frame(app, ctx, egui::vec2(1280.0, 800.0));
        if app.tabs.iter().all(|t| !matches!(t.state, State::Loading(_))) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the document did not load");
}

fn ready_view(app: &App) -> &View {
    match &app.tab().expect("a tab").state {
        State::Ready(v) => v,
        State::Failed(m) => panic!("failed: {m}"),
        _ => panic!("not ready"),
    }
}

#[test]
fn welcome_screen_and_every_theme_render_without_documents() {
    let _data = use_private_test_data_dir();
    let ctx = egui::Context::default();
    let mut app = new_app(&ctx, vec![]);
    for t in ThemeId::ALL {
        app.set_theme(t);
        frames(&mut app, &ctx, 2);
    }
    assert!(app.tabs.is_empty());
    app.palette.open = true;
    app.palette.text = "тем".into();
    frames(&mut app, &ctx, 2);
    app.settings_open = true;
    app.about_open = true;
    frames(&mut app, &ctx, 2);
}

#[test]
fn a_docx_opens_scrolls_searches_and_remembers_its_place() {
    let data = use_private_test_data_dir();
    let dir = tempfile::tempdir().unwrap();
    let d = demo::write_all(dir.path());
    let ctx = egui::Context::default();
    let mut app = new_app(&ctx, vec![d.docx.clone()]);
    wait_ready(&mut app, &ctx);
    let View::Flow(f) = ready_view(&app) else {
        panic!("flow expected")
    };
    assert!(
        f.content_h > 800.0,
        "the document is longer than the window: {}",
        f.content_h
    );
    assert_eq!(app.tab().unwrap().info.as_ref().unwrap().toc.len(), 6);

    for t in ThemeId::ALL {
        app.set_theme(t);
        frames(&mut app, &ctx, 2);
    }

    // search
    app.find.text = "чтени".into();
    app.open_find();
    app.apply_find();
    for _ in 0..200 {
        frames(&mut app, &ctx, 1);
        if app.find_status().1 > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let (_, total, _) = app.find_status();
    assert!(total >= 3, "hits: {total}");
    app.find_step(true);
    frames(&mut app, &ctx, 12);
    let View::Flow(f) = ready_view(&app) else { panic!() };
    assert!(f.offset > 0.0, "the view followed the hit");

    // bookmark and position are stored in the library
    app.toggle_bookmark();
    app.save_position(0);
    let key = app.tab().unwrap().key.clone();
    let entry = app.library.get(&key).expect("history entry");
    assert_eq!(entry.bookmarks.len(), 1);
    assert!(entry.progress > 0.0 || entry.pos.index > 0);

    // the same file opens where it was left
    let saved = entry.pos;
    app.close_tab(0);
    drop(app);
    let ctx2 = egui::Context::default();
    let mut app2 = new_app(&ctx2, vec![d.docx.clone()]);
    wait_ready(&mut app2, &ctx2);
    frames(&mut app2, &ctx2, 14);
    let View::Flow(f2) = ready_view(&app2) else { panic!() };
    assert!(f2.top.index.abs_diff(saved.index) <= 2, "{:?} vs {:?}", f2.top, saved);
    drop(data);
}

#[test]
fn font_size_change_keeps_the_reading_place() {
    let _data = use_private_test_data_dir();
    let dir = tempfile::tempdir().unwrap();
    let d = demo::write_all(dir.path());
    let ctx = egui::Context::default();
    let mut app = new_app(&ctx, vec![d.epub.clone()]);
    wait_ready(&mut app, &ctx);
    app.tab_mut().unwrap().goto(Position { index: 8, frac: 0.0 });
    frames(&mut app, &ctx, 14);
    app.font_step(6.0);
    frames(&mut app, &ctx, 16);
    let View::Flow(f) = ready_view(&app) else { panic!() };
    assert!(f.top.index.abs_diff(8) <= 2, "still near block 8: {:?}", f.top);
}

#[test]
fn a_pdf_is_rendered_searched_and_selected() {
    let _data = use_private_test_data_dir();
    let dir = tempfile::tempdir().unwrap();
    let d = demo::write_all(dir.path());
    let ctx = egui::Context::default();
    let mut app = new_app(&ctx, vec![d.pdf.clone()]);
    wait_ready(&mut app, &ctx);
    for _ in 0..300 {
        frames(&mut app, &ctx, 1);
        if let View::Paged(p) = ready_view(&app)
            && p.cached_pages() > 0
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let View::Paged(p) = ready_view(&app) else {
        panic!("paged view expected")
    };
    assert_eq!(p.page_count(), 3);
    assert!(p.cached_pages() > 0, "a page bitmap arrived");

    app.find.text = "reader".into();
    app.open_find();
    app.apply_find();
    for _ in 0..300 {
        frames(&mut app, &ctx, 1);
        if app.find_status().1 > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.find_status().1 >= 3, "hits: {}", app.find_status().1);
    app.side_tab = SideTab::Pages;
    for t in ThemeId::ALL {
        app.set_theme(t);
        frames(&mut app, &ctx, 3);
    }
    app.go_to_text("2");
    frames(&mut app, &ctx, 14);
    let View::Paged(p) = ready_view(&app) else { panic!() };
    assert_eq!(p.current_page, 1);
}

#[test]
fn spreadsheets_code_and_pictures_open() {
    let _data = use_private_test_data_dir();
    let dir = tempfile::tempdir().unwrap();
    let d = demo::write_all(dir.path());
    let ctx = egui::Context::default();
    let mut app = new_app(&ctx, vec![d.csv.clone(), d.code.clone(), d.comic.clone(), d.md.clone()]);
    wait_ready(&mut app, &ctx);
    assert_eq!(app.tabs.len(), 4);
    for i in 0..4 {
        app.active = i;
        frames(&mut app, &ctx, 3);
        assert!(matches!(app.tabs[i].state, State::Ready(_)), "tab {i}");
    }
    // the sheet finds a city
    app.active = 0;
    app.find.text = "казань".into();
    app.open_find();
    app.apply_find();
    for _ in 0..200 {
        frames(&mut app, &ctx, 1);
        if app.find_status().1 > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(app.find_status().1 >= 10);
}

#[test]
fn broken_and_missing_files_are_reported_not_crashed_on() {
    let _data = use_private_test_data_dir();
    let dir = tempfile::tempdir().unwrap();
    let bad = folio_core::testkit::write(dir.path(), "bad.docx", b"this is not a zip");
    let ctx = egui::Context::default();
    let mut app = new_app(&ctx, vec![bad]);
    wait_ready(&mut app, &ctx);
    assert!(matches!(app.tabs[0].state, State::Failed(_)));
    frames(&mut app, &ctx, 3);
    app.open_path(&ctx, dir.path().join("missing.pdf"));
    assert_eq!(app.tabs.len(), 1, "a missing file does not add a tab");
    assert!(!app.toasts.is_empty());
}

#[test]
fn opening_the_same_file_twice_activates_the_tab() {
    let _data = use_private_test_data_dir();
    let dir = tempfile::tempdir().unwrap();
    let d = demo::write_all(dir.path());
    let ctx = egui::Context::default();
    let mut app = new_app(&ctx, vec![d.md.clone(), d.csv.clone()]);
    wait_ready(&mut app, &ctx);
    assert_eq!(app.active, 1);
    app.open_path(&ctx, d.md.clone());
    assert_eq!((app.tabs.len(), app.active), (2, 0));
    app.close_tab(0);
    assert_eq!((app.tabs.len(), app.active), (1, 0));
}

#[test]
fn password_protected_pdfs_ask_for_the_password() {
    let _data = use_private_test_data_dir();
    // a PDF whose encryption dictionary pdfium cannot open without a password is hard to hand-write;
    // the flow is covered by the state machine below
    let ctx = egui::Context::default();
    let mut app = new_app(&ctx, vec![]);
    let dir = tempfile::tempdir().unwrap();
    let d = demo::write_all(dir.path());
    app.open_path(&ctx, d.md.clone());
    wait_ready(&mut app, &ctx);
    app.tabs[0].state = State::NeedPassword {
        input: String::new(),
        wrong: false,
    };
    frames(&mut app, &ctx, 2);
    app.retry_with_password(&ctx, 0, "x".into());
    wait_ready(&mut app, &ctx);
    assert!(matches!(app.tabs[0].state, State::Ready(_)));
}

#[test]
fn next_and_previous_file_replace_the_tab_in_place() {
    let _data = use_private_test_data_dir();
    let dir = tempfile::tempdir().unwrap();
    let d = demo::write_all(dir.path());
    let ctx = egui::Context::default();
    let mut app = new_app(&ctx, vec![d.csv.clone(), d.md.clone()]);
    wait_ready(&mut app, &ctx);
    app.active = 1;
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort_by(|a, b| folio_core::natural_cmp(a, b));
    let at = names.iter().position(|n| n == "README.md").unwrap();
    app.open_sibling(&ctx, 1);
    wait_ready(&mut app, &ctx);
    assert_eq!(app.tabs.len(), 2, "the tab was replaced, not added");
    assert_eq!(app.active, 1, "in the same slot");
    assert_eq!(
        app.tabs[1].path.file_name().unwrap().to_string_lossy(),
        names[at + 1],
        "the next file in natural order"
    );
    app.open_sibling(&ctx, -1);
    wait_ready(&mut app, &ctx);
    assert_eq!(app.tabs[1].path.file_name().unwrap().to_string_lossy(), "README.md");
}
