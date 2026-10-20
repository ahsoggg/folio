//! Made-up documents for the screenshots and the tests: nothing here reads or writes anything of the user's.

use folio_core::testkit::{tiny_png, write, zip_bytes};
use std::path::{Path, PathBuf};

const PROSE: [&str; 10] = [
    "Хорошая читалка не мешает читать. Она открывает файл мгновенно, помнит, на какой строке вы остановились вчера, и не просит ничего устанавливать, кроме самой себя. Всё остальное — шрифты, цвета, поля — подстраивается под глаза, а не под разработчика.",
    "Первые читалки появились вместе с компьютерами, но долго оставались набором компромиссов: один формат — одна программа, один экран — один масштаб. Сегодня документы живут в десятке форматов, и пользователь вправе ожидать, что любой из них откроется одним щелчком.",
    "Текст на экране читается иначе, чем на бумаге. Строка длиннее семидесяти знаков утомляет, слишком плотные абзацы сливаются, а белый фон в темноте слепит. Поэтому ширина колонки, интервал и тема здесь настраиваются, а удачные значения запоминаются.",
    "Когда документ большой, главным становится не оформление, а навигация: оглавление слева, поиск по всему тексту с подсветкой, закладки на важных местах и быстрый переход к нужной странице. Всё это доступно с клавиатуры, не отрывая рук от книги.",
    "Таблицы и презентации читаются не хуже книг. Листы Excel показываются сеткой с буквами столбцов и закреплённой шапкой, слайды PowerPoint превращаются в последовательность заголовков и тезисов, а заметки докладчика не теряются.",
    "Иногда читать глазами неудобно: за рулём, на кухне, вечером после работы. Тогда текст можно просто послушать — голос Windows прочитает документ абзац за абзацем, а подсветка покажет, где он сейчас. Скорость и пауза настраиваются.",
    "Программа ничего не отправляет в сеть, кроме одного запроса в сутки: не вышла ли новая версия. Документы остаются на вашем компьютере, история чтения лежит в обычном файле рядом с настройками, и её можно стереть одной кнопкой.",
    "Если файл не открывается, причина всегда называется прямо: повреждён архив, нужен пароль, формат не поддерживается. Старые двоичные форматы .doc и .ppt лучше сначала пересохранить в современные — это занимает секунду в любом офисном пакете.",
    "Закладка — это не только звёздочка в списке. Она хранит место в тексте, подпись из первых слов абзаца и дату, а при смене размера шрифта находит тот же абзац заново, потому что запоминает не пиксели, а смысл позиции.",
    "В конце концов, лучшая читалка — та, о которой забываешь. Откройте книгу, перелистайте, закройте окно. Завтра она встретит вас на том же месте, в той же теме, с тем же размером букв, будто вы никуда не уходили.",
];

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn docx_para(text: &str) -> String {
    format!(
        "<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
        xml_escape(text)
    )
}

pub fn make_docx() -> Vec<u8> {
    let mut body = String::new();
    body.push_str("<w:p><w:pPr><w:pStyle w:val=\"Title\"/></w:pPr><w:r><w:t>Как читать удобно</w:t></w:r></w:p>");
    body.push_str(&docx_para("Короткое руководство по спокойному чтению с экрана."));
    for (i, chapter) in ["Начало", "Навигация", "Таблицы и слайды", "Послушать вместо чтения"]
        .iter()
        .enumerate()
    {
        body.push_str(&format!(
            "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>{chapter}</w:t></w:r></w:p>"
        ));
        for p in 0..3 {
            let t = PROSE[(i * 3 + p) % PROSE.len()];
            if p == 1 {
                body.push_str(&format!(
                    "<w:p><w:r><w:t xml:space=\"preserve\">{} </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>Важное</w:t></w:r><w:r><w:t xml:space=\"preserve\"> и </w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>курсивом</w:t></w:r><w:r><w:t>.</w:t></w:r></w:p>",
                    xml_escape(t)
                ));
            } else {
                body.push_str(&docx_para(t));
            }
        }
        if i == 1 {
            body.push_str(
                "<w:p><w:pPr><w:pStyle w:val=\"Heading2\"/></w:pPr><w:r><w:t>Горячие клавиши</w:t></w:r></w:p>",
            );
            for item in [
                "Ctrl+F — поиск по тексту",
                "Ctrl+B — закладка на этом месте",
                "Ctrl+L — читать вслух",
                "T — сменить тему",
            ] {
                body.push_str(&format!(
                    "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>{}</w:t></w:r></w:p>",
                    xml_escape(item)
                ));
            }
        }
        if i == 2 {
            body.push_str("<w:tbl><w:tr><w:trPr><w:tblHeader/></w:trPr>");
            for h in ["Формат", "Что показывает", "Поиск"] {
                body.push_str(&format!(
                    "<w:tc><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>{h}</w:t></w:r></w:p></w:tc>"
                ));
            }
            body.push_str("</w:tr>");
            for row in [
                ["PDF", "страницы как в оригинале", "да"],
                ["DOCX", "текст с оформлением", "да"],
                ["XLSX", "лист сеткой", "да"],
                ["EPUB", "книга с оглавлением", "да"],
            ] {
                body.push_str("<w:tr>");
                for c in row {
                    body.push_str(&format!("<w:tc><w:p><w:r><w:t>{c}</w:t></w:r></w:p></w:tc>"));
                }
                body.push_str("</w:tr>");
            }
            body.push_str("</w:tbl>");
        }
    }
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
    );
    let styles = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:styleId="Title"><w:name w:val="Title"/></w:style>
<w:style w:styleId="Heading1"><w:name w:val="heading 1"/></w:style>
<w:style w:styleId="Heading2"><w:name w:val="heading 2"/></w:style></w:styles>"#;
    zip_bytes(&[
        ("word/document.xml", document.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
        ("word/numbering.xml", folio_core::testkit::DOCX_NUMBERING.as_bytes()),
        (
            "docProps/core.xml",
            "<cp:coreProperties xmlns:cp=\"x\" xmlns:dc=\"y\"><dc:title>Как читать удобно</dc:title><dc:creator>Редакция Folio</dc:creator></cp:coreProperties>".as_bytes(),
        ),
    ])
}

pub fn make_epub() -> Vec<u8> {
    let chapter = |n: usize, title: &str| {
        let mut h = format!(
            "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{title}</title></head><body><h1>{title}</h1>"
        );
        for p in 0..6 {
            h.push_str(&format!("<p>{}</p>", xml_escape(PROSE[(n * 2 + p) % PROSE.len()])));
        }
        h.push_str("</body></html>");
        h
    };
    let titles = ["Глава 1. Дорога", "Глава 2. Город", "Глава 3. Возвращение"];
    let chapters: Vec<String> = titles.iter().enumerate().map(|(i, t)| chapter(i, t)).collect();
    let opf = "<package><metadata xmlns:dc=\"x\"><dc:title>Тихое чтение</dc:title><dc:creator>А. Читатель</dc:creator></metadata><manifest><item id=\"c1\" href=\"c1.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"c2\" href=\"c2.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"c3\" href=\"c3.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/></manifest><spine toc=\"ncx\"><itemref idref=\"c1\"/><itemref idref=\"c2\"/><itemref idref=\"c3\"/></spine></package>";
    let ncx = format!(
        "<ncx><navMap>{}</navMap></ncx>",
        titles
            .iter()
            .enumerate()
            .map(|(i, t)| format!(
                "<navPoint><navLabel><text>{t}</text></navLabel><content src=\"c{}.xhtml\"/></navPoint>",
                i + 1
            ))
            .collect::<String>()
    );
    zip_bytes(&[
        ("mimetype", b"application/epub+zip"),
        (
            "META-INF/container.xml",
            br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
        ),
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/toc.ncx", ncx.as_bytes()),
        ("OEBPS/c1.xhtml", chapters[0].as_bytes()),
        ("OEBPS/c2.xhtml", chapters[1].as_bytes()),
        ("OEBPS/c3.xhtml", chapters[2].as_bytes()),
    ])
}

pub fn make_markdown() -> String {
    let mut s = String::from("# Folio\n\nЧиталка документов для Windows.\n\n## Что умеет\n\n");
    for line in [
        "открывает PDF, DOCX, EPUB, FB2, XLSX, PPTX и ещё десяток форматов",
        "ищет по тексту с подсветкой",
        "запоминает место чтения",
        "читает вслух",
    ] {
        s.push_str(&format!("- {line}\n"));
    }
    s.push_str("\n## Пример кода\n\n```rust\nfn main() {\n    let book = folio::open(\"book.epub\")?;\n    println!(\"{} слов\", book.word_count());\n}\n```\n\n> Лучшая читалка — та, о которой забываешь.\n\n");
    s.push_str("| Тема | Для чего |\n|---|---|\n| Mocha | вечером |\n| Сепия | как бумага |\n| Ночь | в темноте |\n");
    s
}

pub fn make_csv() -> String {
    let mut s = String::from("Месяц,Город,Продано,Выручка,Рост\n");
    let cities = ["Москва", "Казань", "Самара", "Омск", "Пермь"];
    for i in 0..60 {
        let sold = 120 + (i * 37) % 210;
        s.push_str(&format!(
            "{},{},{},{},{}%\n",
            ["Январь", "Февраль", "Март", "Апрель", "Май", "Июнь"][i % 6],
            cities[i % cities.len()],
            sold,
            sold * 1450 + (i * 913) % 5000,
            (i * 7) % 23
        ));
    }
    s
}

pub fn make_code() -> String {
    "use std::collections::HashMap;\n\n/// Counts words in a text, case-insensitively.\npub fn count_words(text: &str) -> HashMap<String, usize> {\n    let mut counts = HashMap::new();\n    for word in text.split_whitespace() {\n        let key = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();\n        if key.is_empty() {\n            continue;\n        }\n        *counts.entry(key).or_insert(0) += 1;\n    }\n    counts\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn counts_ignore_case_and_punctuation() {\n        let c = count_words(\"Hello, hello! World\");\n        assert_eq!(c[\"hello\"], 2);\n        assert_eq!(c[\"world\"], 1);\n    }\n}\n"
        .repeat(3)
}

fn pdf_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)")
}

/// A multi-page PDF with English text in Times and Helvetica (the standard fonts need no embedding).
pub fn make_pdf() -> Vec<u8> {
    let paras = [
        "A good reader gets out of the way. It opens the file at once, remembers where you stopped yesterday and asks for nothing except a place to live.",
        "Pages are drawn by pdfium, the engine of a well known browser. Text can be selected and copied, links work, and the search highlights every match.",
        "On a dark theme the page itself turns dark: the bitmap is inverted and mapped to the colours of the theme, so a white page never blinds you at night.",
        "Thumbnails on the left show where you are. A click jumps to the page, the arrow keys turn pages, and the zoom stays where you left it.",
    ];
    let mut objs: Vec<String> = Vec::new();
    let page_count = 3;
    let mut kids = String::new();
    // object layout: 1 catalog, 2 pages, 3 font Times, 4 font Helvetica, then pairs (page, content)
    for p in 0..page_count {
        kids.push_str(&format!("{} 0 R ", 5 + p * 2));
    }
    objs.push("<</Type/Catalog/Pages 2 0 R>>".into());
    objs.push(format!("<</Type/Pages/Kids[{kids}]/Count {page_count}>>"));
    objs.push("<</Type/Font/Subtype/Type1/BaseFont/Times-Roman>>".into());
    objs.push("<</Type/Font/Subtype/Type1/BaseFont/Helvetica-Bold>>".into());
    for p in 0..page_count {
        let mut s = String::new();
        s.push_str(&format!(
            "BT /F2 26 Tf 72 740 Td ({}) Tj ET\n",
            pdf_escape(&format!("Chapter {}: Quiet reading", p + 1))
        ));
        let mut y = 690.0;
        for k in 0..6 {
            let text = paras[(p + k) % paras.len()];
            // break the paragraph into lines of about 78 characters
            let mut line = String::new();
            for word in text.split(' ') {
                if line.len() + word.len() > 78 {
                    s.push_str(&format!("BT /F1 13 Tf 72 {y} Td ({}) Tj ET\n", pdf_escape(&line)));
                    y -= 18.0;
                    line.clear();
                }
                line.push_str(word);
                line.push(' ');
            }
            s.push_str(&format!("BT /F1 13 Tf 72 {y} Td ({}) Tj ET\n", pdf_escape(&line)));
            y -= 30.0;
        }
        objs.push(format!(
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents {} 0 R/Resources<</Font<</F1 3 0 R/F2 4 0 R>>>>>>",
            6 + p * 2
        ));
        objs.push(format!("<</Length {}>>\nstream\n{}endstream", s.len(), s));
    }
    let mut out = String::from("%PDF-1.4\n");
    for (i, o) in objs.iter().enumerate() {
        out.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, o));
    }
    out.push_str(&format!("trailer\n<</Root 1 0 R/Size {}>>\n%%EOF\n", objs.len() + 1));
    out.into_bytes()
}

pub struct Demo {
    pub docx: PathBuf,
    pub epub: PathBuf,
    pub md: PathBuf,
    pub csv: PathBuf,
    pub code: PathBuf,
    pub pdf: PathBuf,
    #[allow(dead_code)]
    pub comic: PathBuf,
}

pub fn write_all(dir: &Path) -> Demo {
    let png = tiny_png();
    let comic = zip_bytes(&[("1.png", &png), ("2.png", &png), ("3.png", &png)]);
    Demo {
        docx: write(dir, "Как читать удобно.docx", &make_docx()),
        epub: write(dir, "Тихое чтение.epub", &make_epub()),
        md: write(dir, "README.md", make_markdown().as_bytes()),
        csv: write(dir, "Продажи.csv", make_csv().as_bytes()),
        code: write(dir, "wordcount.rs", make_code().as_bytes()),
        pdf: write(dir, "Quiet reading.pdf", &make_pdf()),
        comic: write(dir, "comic.cbz", &comic),
    }
}
