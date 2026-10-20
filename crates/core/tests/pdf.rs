use folio_core::pdf::{Engine, OpenError, Tone};
use std::path::PathBuf;
use std::sync::mpsc::channel;

fn dll() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/pdfium/pdfium.dll")
}

/// A one-page PDF with the text "Hello Folio reader" (the cross-reference table is left for pdfium to rebuild).
fn tiny_pdf() -> Vec<u8> {
    let stream = "BT /F1 24 Tf 50 700 Td (Hello Folio reader) Tj ET";
    format!(
        "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
         3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 400 800]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>endobj\n\
         4 0 obj<</Length {}>>stream\n{stream}\nendstream endobj\n5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n\
         trailer<</Root 1 0 R/Size 6>>\n%%EOF\n",
        stream.len()
    )
    .into_bytes()
}

#[test]
fn pdf_engine_opens_renders_searches_and_selects() {
    let dll = dll();
    assert!(dll.exists(), "pdfium.dll is part of the repository: {}", dll.display());
    let engine = Engine::start(&dll).expect("pdfium loads");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.pdf");
    std::fs::write(&path, tiny_pdf()).unwrap();

    let (doc, info) = engine.open(&path, None).expect("opens");
    assert_eq!(info.pages, vec![(400.0, 800.0)]);

    let (tx, rx) = channel();
    engine.render(doc, 0, 200, Tone::Paper, move |r| {
        let _ = tx.send(r);
    });
    let page = rx.recv().unwrap().expect("rendered");
    assert_eq!((page.width, page.height), (200, 400));
    assert!(page.rgba.chunks(4).any(|p| p[0] < 128), "some dark text pixels");
    assert!(
        page.rgba.chunks(4).next().unwrap().iter().all(|&b| b == 255),
        "white paper"
    );

    let (tx, rx) = channel();
    engine.render(doc, 0, 200, Tone::Dark, move |r| {
        let _ = tx.send(r);
    });
    let dark = rx.recv().unwrap().unwrap();
    assert!(dark.rgba[0] < 60, "dark theme background");

    let hits = engine.search(doc, "folio", false, 0, 10).recv().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page, 0);
    assert!(hits[0].snippet.contains("Hello Folio"), "snippet: {}", hits[0].snippet);
    let r = hits[0].rects[0];
    assert!(r.left >= 50.0 && r.bottom > 690.0 && r.top < 740.0, "{r:?}");
    assert!(engine.search(doc, "Folio", true, 0, 10).recv().unwrap().len() == 1);
    assert!(engine.search(doc, "folio", true, 0, 10).recv().unwrap().is_empty());

    let sel = engine.select(doc, 0, (52.0, 705.0), (400.0, 705.0));
    assert_eq!(sel.text, "Hello Folio reader");
    assert_eq!(sel.rects.len(), 1);
    assert!(engine.page_text(doc, 0).contains("Folio"));

    engine.close(doc);
    assert!(matches!(
        engine.open(&dir.path().join("none.pdf"), None),
        Err(OpenError::Failed(_))
    ));
}
