//! EPUB: the spine in reading order, converted chapter by chapter, with the book's own table of contents.

use crate::model::*;
use crate::xml::{self, Ev, attr};
use crate::zipx::{self, Archive};
use std::collections::HashMap;
use std::io::{Read, Seek};

struct Item {
    href: String,
    media: String,
    props: String,
}

fn opf_path<R: Read + Seek>(zip: &mut Archive<R>) -> Option<String> {
    let c = zip.read_str("META-INF/container.xml")?;
    xml::events(&c).into_iter().find_map(|e| match e {
        Ev::Start { name, attrs } if name == "rootfile" => attr(&attrs, "full-path").map(str::to_owned),
        _ => None,
    })
}

/// `(level, title, target file)` from an NCX file.
fn parse_ncx(text: &str, base: &str) -> Vec<(u8, String, String)> {
    let mut out = Vec::new();
    let (mut depth, mut in_label, mut title) = (0u8, false, String::new());
    for e in xml::events(text) {
        match e {
            Ev::Start { name, attrs } => match name.as_str() {
                "navPoint" => {
                    depth += 1;
                    title.clear();
                }
                "navLabel" => in_label = true,
                "content" => {
                    if let Some(src) = attr(&attrs, "src") {
                        out.push((depth.max(1), title.trim().to_owned(), zipx::resolve(base, src)));
                    }
                }
                _ => {}
            },
            Ev::End { name } => match name.as_str() {
                "navPoint" => depth = depth.saturating_sub(1),
                "navLabel" => in_label = false,
                _ => {}
            },
            Ev::Text(t) if in_label => title.push_str(&t),
            _ => {}
        }
    }
    out
}

/// EPUB 3 navigation document: `<nav epub:type="toc">` with nested lists.
fn parse_nav(text: &str, base: &str) -> Vec<(u8, String, String)> {
    use scraper::{ElementRef, Html, Selector};
    let doc = Html::parse_document(text);
    let Ok(sel) = Selector::parse("nav") else {
        return Vec::new();
    };
    let Some(nav) = doc
        .select(&sel)
        .find(|n| n.value().attr("epub:type").is_some_and(|t| t.contains("toc")))
        .or_else(|| doc.select(&sel).next())
    else {
        return Vec::new();
    };
    fn walk(el: ElementRef, depth: u8, base: &str, out: &mut Vec<(u8, String, String)>) {
        for c in el.children().filter_map(ElementRef::wrap) {
            match c.value().name() {
                "a" => {
                    let title = c
                        .text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    if let Some(h) = c.value().attr("href") {
                        out.push((depth.max(1), title, zipx::resolve(base, h)));
                    }
                }
                "ol" | "ul" => walk(c, depth + 1, base, out),
                _ => walk(c, depth, base, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(nav, 0, base, &mut out);
    out
}

pub fn read<R: Read + Seek>(zip: &mut Archive<R>) -> Result<(Flow, Meta), String> {
    let opf_name = opf_path(zip).ok_or("META-INF/container.xml does not name the package file")?;
    let opf = zip.read_str(&opf_name).ok_or("the package file is missing")?;
    let mut meta = Meta::default();
    let mut manifest: HashMap<String, Item> = HashMap::new();
    let mut spine: Vec<String> = Vec::new();
    let mut ncx_id = None;
    let mut tag = String::new();
    for e in xml::events(&opf) {
        match e {
            Ev::Start { name, attrs } => {
                match name.as_str() {
                    "item" => {
                        if let (Some(id), Some(href)) = (attr(&attrs, "id"), attr(&attrs, "href")) {
                            manifest.insert(
                                id.to_owned(),
                                Item {
                                    href: zipx::resolve(&opf_name, &percent_decode(href)),
                                    media: attr(&attrs, "media-type").unwrap_or("").to_owned(),
                                    props: attr(&attrs, "properties").unwrap_or("").to_owned(),
                                },
                            );
                        }
                    }
                    "itemref" if attr(&attrs, "linear") != Some("no") => {
                        if let Some(r) = attr(&attrs, "idref") {
                            spine.push(r.to_owned());
                        }
                    }
                    "spine" => ncx_id = attr(&attrs, "toc").map(str::to_owned),
                    _ => {}
                }
                tag = name;
            }
            Ev::Text(t) if !t.trim().is_empty() => match tag.as_str() {
                "title" if meta.title.is_none() => meta.title = Some(t.trim().to_owned()),
                "creator" if meta.author.is_none() => meta.author = Some(t.trim().to_owned()),
                _ => {}
            },
            _ => {}
        }
    }
    if spine.is_empty() {
        return Err("the book has an empty reading order".into());
    }

    let mut flow = Flow::default();
    let mut chapter_start: HashMap<String, usize> = HashMap::new();
    for (n, idref) in spine.iter().enumerate() {
        let Some(item) = manifest.get(idref) else { continue };
        if !(item.media.contains("html") || item.media.contains("xml")) {
            continue;
        }
        let Some(html) = zip.read_str(&item.href) else { continue };
        if n > 0 && !flow.blocks.is_empty() {
            flow.blocks.push(Block::PageBreak);
        }
        chapter_start.insert(item.href.clone(), flow.blocks.len());
        let base = item.href.clone();
        let mut loader = |src: &str| zip.read(&zipx::resolve(&base, &percent_decode(src)));
        crate::html::convert(&html, &mut flow, &mut loader);
    }

    // the book's own contents list, mapped to blocks through the chapter files
    let toc_src: Vec<(u8, String, String)> = {
        let nav = manifest
            .values()
            .find(|i| i.props.split_whitespace().any(|p| p == "nav"));
        let ncx = ncx_id
            .as_ref()
            .and_then(|id| manifest.get(id))
            .or_else(|| manifest.values().find(|i| i.media.contains("ncx")));
        let mut v = Vec::new();
        if let Some(i) = nav
            && let Some(t) = zip.read_str(&i.href)
        {
            v = parse_nav(&t, &i.href);
        }
        if v.is_empty()
            && let Some(i) = ncx
            && let Some(t) = zip.read_str(&i.href)
        {
            v = parse_ncx(&t, &i.href);
        }
        v
    };
    flow.build_toc();
    let mapped: Vec<TocItem> = toc_src
        .into_iter()
        .filter(|(_, title, _)| !title.is_empty())
        .filter_map(|(level, title, target)| {
            let file = percent_decode(target.split('#').next().unwrap_or(""));
            chapter_start.get(&file).map(|&block| TocItem {
                level: level.min(6),
                title,
                block,
            })
        })
        .collect();
    if mapped.len() >= 2 {
        flow.toc = mapped;
    }
    Ok((flow, meta))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{tiny_png, zip_bytes};

    #[test]
    fn spine_toc_and_images() {
        let png = tiny_png();
        let bytes = zip_bytes(&[
            ("META-INF/container.xml", br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#),
            (
                "OEBPS/content.opf",
                r#"<package><metadata xmlns:dc="x"><dc:title>Книга</dc:title><dc:creator>Автор</dc:creator></metadata>
<manifest><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="text/ch%202.xhtml" media-type="application/xhtml+xml"/>
<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="img" href="img/a.png" media-type="image/png"/></manifest>
<spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#.as_bytes(),
            ),
            ("OEBPS/ch1.xhtml", "<html><body><p>Начало</p><img src='img/a.png' alt='a'/></body></html>".as_bytes()),
            ("OEBPS/text/ch 2.xhtml", "<html><body><p>Конец</p></body></html>".as_bytes()),
            (
                "OEBPS/toc.ncx",
                r#"<ncx><navMap><navPoint><navLabel><text>Первая</text></navLabel><content src="ch1.xhtml"/></navPoint>
<navPoint><navLabel><text>Вторая</text></navLabel><content src="text/ch%202.xhtml#x"/></navPoint></navMap></ncx>"#.as_bytes(),
            ),
            ("OEBPS/img/a.png", &png),
        ]);
        let mut zip = Archive::new(std::io::Cursor::new(bytes)).unwrap();
        let (flow, meta) = read(&mut zip).unwrap();
        assert_eq!(
            (meta.title.as_deref(), meta.author.as_deref()),
            (Some("Книга"), Some("Автор"))
        );
        assert!(flow.plain_text().contains("Начало"));
        assert!(flow.plain_text().contains("Конец"));
        assert_eq!(flow.images.len(), 1);
        let titles: Vec<_> = flow.toc.iter().map(|t| (t.title.as_str(), t.block)).collect();
        assert_eq!(titles, vec![("Первая", 0), ("Вторая", 3)]);
    }
}
