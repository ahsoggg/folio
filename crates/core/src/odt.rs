//! OpenDocument text (.odt) and presentations (.odp): paragraphs, headings, lists, tables, images, links, formatting.

use crate::model::*;
use crate::xml::{self, Ev, attr};
use crate::zipx::Archive;
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::sync::Arc;

#[derive(Clone, Default)]
struct Props {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    mono: bool,
    sup: bool,
    sub: bool,
    color: Option<[u8; 3]>,
    bg: Option<[u8; 3]>,
    center: Option<Align>,
    page_break: bool,
}

#[derive(Default)]
struct StyleDef {
    parent: Option<String>,
    props: Props,
}

fn color(v: &str) -> Option<[u8; 3]> {
    let h = v.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

fn parse_styles(text: &str, defs: &mut HashMap<String, StyleDef>, numbered_lists: &mut HashMap<String, bool>) {
    let mut cur: Option<String> = None;
    let mut list_style: Option<String> = None;
    for e in xml::events(text) {
        match e {
            Ev::Start { name, attrs } => match name.as_str() {
                "style" => {
                    cur = attr(&attrs, "name").map(str::to_owned);
                    if let Some(n) = &cur {
                        let def = defs.entry(n.clone()).or_default();
                        def.parent = attr(&attrs, "parent-style-name").map(str::to_owned);
                    }
                }
                "text-properties" | "paragraph-properties" => {
                    let Some(def) = cur.as_ref().and_then(|c| defs.get_mut(c)) else {
                        continue;
                    };
                    let p = &mut def.props;
                    for (k, v) in &attrs {
                        match (k.rsplit(':').next().unwrap_or(k), v.as_str()) {
                            ("font-weight", w) => p.bold = w == "bold" || w.parse::<u32>().is_ok_and(|n| n >= 600),
                            ("font-style", "italic" | "oblique") => p.italic = true,
                            ("text-underline-style", s) => p.underline = s != "none",
                            ("text-line-through-style", s) => p.strike = s != "none",
                            ("color", c) => p.color = color(c),
                            ("background-color", c) => p.bg = color(c),
                            ("text-position", s) => {
                                if s.starts_with("super") || s.starts_with('+') {
                                    p.sup = true;
                                } else if s.starts_with("sub") || s.starts_with('-') {
                                    p.sub = true;
                                }
                            }
                            ("font-name", f) | ("font-family", f) => {
                                let l = f.to_lowercase();
                                p.mono = ["courier", "consolas", "mono", "lucida console"]
                                    .iter()
                                    .any(|m| l.contains(m));
                            }
                            ("text-align", "center") => p.center = Some(Align::Center),
                            ("text-align", "end" | "right") => p.center = Some(Align::Right),
                            ("text-align", "justify") => p.center = Some(Align::Justify),
                            ("break-before", "page") => p.page_break = true,
                            _ => {}
                        }
                    }
                }
                "list-style" => {
                    list_style = attr(&attrs, "name").map(str::to_owned);
                    if let Some(n) = &list_style {
                        numbered_lists.entry(n.clone()).or_insert(false);
                    }
                }
                "list-level-style-number" => {
                    if let Some(n) = &list_style {
                        numbered_lists.insert(n.clone(), true);
                    }
                }
                _ => {}
            },
            Ev::End { name } => match name.as_str() {
                "style" => cur = None,
                "list-style" => list_style = None,
                _ => {}
            },
            _ => {}
        }
    }
}

struct Walker<'a> {
    defs: HashMap<String, StyleDef>,
    numbered: HashMap<String, bool>,
    flow: Flow,
    load: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>,
}

impl Walker<'_> {
    fn props(&self, name: &str) -> Props {
        // parents first, so the style's own settings win
        let mut chain = Vec::new();
        let mut cur = Some(name.to_owned());
        while let Some(n) = cur {
            if chain.len() > 8 {
                break;
            }
            cur = self.defs.get(&n).and_then(|d| d.parent.clone());
            chain.push(n);
        }
        let mut merged = Props::default();
        for n in chain.iter().rev() {
            if let Some(d) = self.defs.get(n) {
                let p = &d.props;
                merged.bold |= p.bold;
                merged.italic |= p.italic;
                merged.underline |= p.underline;
                merged.strike |= p.strike;
                merged.mono |= p.mono;
                merged.sup |= p.sup;
                merged.sub |= p.sub;
                merged.color = p.color.or(merged.color);
                merged.bg = p.bg.or(merged.bg);
                merged.center = p.center.or(merged.center);
                merged.page_break |= p.page_break;
            }
        }
        merged
    }

    fn heading_of(&self, style: &str) -> Option<u8> {
        let mut cur = Some(style.to_owned());
        for _ in 0..8 {
            let n = cur?;
            let norm = n.replace("_20_", " ").replace('_', " ").to_lowercase();
            if let Some(l) = norm.strip_prefix("heading ")
                && let Ok(l) = l.trim().parse::<u8>()
            {
                return Some(l.clamp(1, 6));
            }
            if norm == "title" {
                return Some(1);
            }
            if norm == "subtitle" {
                return Some(2);
            }
            cur = self.defs.get(&n).and_then(|d| d.parent.clone());
        }
        None
    }

    fn style_from(&self, base: &Style, name: &str) -> Style {
        let p = self.props(name);
        let mut s = base.clone();
        s.bold |= p.bold;
        s.italic |= p.italic;
        s.underline |= p.underline;
        s.strike |= p.strike;
        s.mono |= p.mono;
        s.sup |= p.sup;
        s.sub |= p.sub;
        s.color = p.color.or(s.color);
        s.bg = p.bg.or(s.bg);
        s
    }

    fn walk(&mut self, evs: &[Ev]) {
        let mut cur = Rich::default();
        let mut styles: Vec<Style> = vec![Style::default()];
        let mut para: Option<(Option<u8>, Align, bool)> = None;
        let mut lists: Vec<(bool, u32)> = Vec::new();
        let mut marker: Option<String> = None;
        let mut table_depth = 0u32;
        let (mut rows, mut row, mut cell): (Vec<Vec<Cell>>, Vec<Cell>, Option<Cell>) = (Vec::new(), Vec::new(), None);
        let mut note_depth = 0u32;
        let mut pending_images: Vec<usize> = Vec::new();
        let mut slide = 0usize;
        let mut link_stack: Vec<bool> = Vec::new();
        for e in evs {
            match e {
                Ev::Start { name, attrs } => {
                    if note_depth > 0 {
                        if name == "note" {
                            note_depth += 1;
                        }
                        continue;
                    }
                    match name.as_str() {
                        "note" => note_depth = 1,
                        "page" if attr(attrs, "master-page-name").is_some() => {
                            slide += 1;
                            if slide > 1 {
                                self.flow.blocks.push(Block::PageBreak);
                            }
                            self.flow.blocks.push(Block::Heading {
                                level: 2,
                                text: Rich::plain(format!("Slide {slide}")),
                            });
                        }
                        "h" | "p" => {
                            let style_name = attr(attrs, "style-name").unwrap_or("");
                            let props = self.props(style_name);
                            let level = if name == "h" {
                                Some(
                                    attr(attrs, "outline-level")
                                        .and_then(|v| v.parse().ok())
                                        .unwrap_or(1u8)
                                        .clamp(1, 6),
                                )
                            } else {
                                self.heading_of(style_name)
                            };
                            let base = self.style_from(&Style::default(), style_name);
                            styles = vec![base];
                            para = Some((level, props.center.unwrap_or_default(), props.page_break));
                            cur = Rich::default();
                        }
                        "span" => {
                            let top = styles.last().cloned().unwrap_or_default();
                            styles.push(self.style_from(&top, attr(attrs, "style-name").unwrap_or("")));
                        }
                        "a" => {
                            let mut top = styles.last().cloned().unwrap_or_default();
                            let href = attr(attrs, "href").unwrap_or("");
                            let external = href.starts_with("http") || href.starts_with("mailto:");
                            if external {
                                top.link = Some(Arc::from(href));
                                top.underline = true;
                            }
                            styles.push(top);
                            link_stack.push(external);
                        }
                        "s" => {
                            let n = attr(attrs, "c").and_then(|v| v.parse().ok()).unwrap_or(1usize).min(64);
                            let st = styles.last().cloned().unwrap_or_default();
                            cur.push(&" ".repeat(n), &st);
                        }
                        "tab" => {
                            let st = styles.last().cloned().unwrap_or_default();
                            cur.push("    ", &st);
                        }
                        "line-break" => {
                            let st = styles.last().cloned().unwrap_or_default();
                            cur.push("\n", &st);
                        }
                        "list" => {
                            let numbered = attr(attrs, "style-name")
                                .and_then(|n| self.numbered.get(n).copied())
                                .unwrap_or_else(|| lists.last().is_some_and(|l| l.0));
                            lists.push((numbered, 0));
                        }
                        "list-item" => {
                            marker = Some(match lists.last_mut() {
                                Some((true, n)) => {
                                    *n += 1;
                                    format!("{n}.")
                                }
                                _ => "•".to_owned(),
                            });
                        }
                        "table" => {
                            table_depth += 1;
                            if table_depth == 1 {
                                rows.clear();
                            }
                        }
                        "table-row" if table_depth == 1 => row = Vec::new(),
                        "table-cell" if table_depth == 1 => {
                            cur = Rich::default();
                            cell = Some(Cell {
                                text: Rich::default(),
                                colspan: attr(attrs, "number-columns-spanned")
                                    .and_then(|v| v.parse().ok())
                                    .unwrap_or(1usize)
                                    .clamp(1, 64),
                            });
                        }
                        "image" => {
                            if let Some(h) = attr(attrs, "href")
                                && let Some(bytes) = (self.load)(h)
                                && image::guess_format(&bytes).is_ok()
                            {
                                pending_images.push(self.flow.add_image(bytes));
                            }
                        }
                        _ => {}
                    }
                }
                Ev::End { name } => {
                    if note_depth > 0 {
                        if name == "note" {
                            note_depth -= 1;
                        }
                        continue;
                    }
                    match name.as_str() {
                        "span" => {
                            if styles.len() > 1 {
                                styles.pop();
                            }
                        }
                        "a" => {
                            link_stack.pop();
                            if styles.len() > 1 {
                                styles.pop();
                            }
                        }
                        "h" | "p" => {
                            if let Some((level, align, page_break)) = para.take() {
                                if page_break && table_depth == 0 {
                                    self.flow.blocks.push(Block::PageBreak);
                                }
                                if table_depth > 0 {
                                    if let Some(c) = cell.as_mut() {
                                        if !c.text.spans.is_empty() && !cur.is_blank() {
                                            c.text.push("\n", &Style::default());
                                        }
                                        for s in std::mem::take(&mut cur).spans {
                                            c.text.push(&s.text, &s.style);
                                        }
                                    }
                                } else {
                                    let mut rich = std::mem::take(&mut cur);
                                    rich.trim_end();
                                    if !rich.is_blank() {
                                        if let Some(level) = level {
                                            marker = None;
                                            self.flow.blocks.push(Block::Heading { level, text: rich });
                                        } else if let Some(m) = marker.take() {
                                            self.flow.blocks.push(Block::ListItem {
                                                depth: lists.len().saturating_sub(1).min(8) as u8,
                                                marker: m,
                                                text: rich,
                                            });
                                        } else {
                                            self.flow.blocks.push(Block::Paragraph { text: rich, align });
                                        }
                                    }
                                }
                                for id in pending_images.drain(..) {
                                    self.flow.blocks.push(Block::Image { id, alt: String::new() });
                                }
                            }
                            styles = vec![Style::default()];
                        }
                        "list" => {
                            lists.pop();
                        }
                        "table-cell" if table_depth == 1 => {
                            if let Some(mut c) = cell.take() {
                                c.text.trim_end();
                                row.push(c);
                            }
                        }
                        "table-row" if table_depth == 1 => {
                            if !row.is_empty() {
                                rows.push(std::mem::take(&mut row));
                            }
                        }
                        "table" => {
                            table_depth = table_depth.saturating_sub(1);
                            if table_depth == 0 && !rows.is_empty() {
                                let header_rows = 0;
                                self.flow.blocks.push(Block::Table(Table {
                                    rows: std::mem::take(&mut rows),
                                    header_rows,
                                }));
                            }
                        }
                        "frame" => {
                            for id in pending_images.drain(..) {
                                self.flow.blocks.push(Block::Image { id, alt: String::new() });
                            }
                        }
                        _ => {}
                    }
                }
                Ev::Text(t) => {
                    if note_depth > 0 || para.is_none() {
                        continue;
                    }
                    let st = styles.last().cloned().unwrap_or_default();
                    // XML whitespace in content is insignificant apart from single spaces
                    let mut collapsed = String::with_capacity(t.len());
                    let mut space = cur.spans.last().is_none_or(|l| l.text.ends_with(' '));
                    for ch in t.chars() {
                        if ch.is_whitespace() {
                            if !space {
                                collapsed.push(' ');
                            }
                            space = true;
                        } else {
                            collapsed.push(ch);
                            space = false;
                        }
                    }
                    cur.push(&collapsed, &st);
                }
            }
        }
    }
}

fn read_inner<R: Read + Seek>(zip: &mut Archive<R>, expect_mime: &str) -> Result<(Flow, Meta), String> {
    let mime = zip.read_str("mimetype").unwrap_or_default();
    let content = zip
        .read_str("content.xml")
        .ok_or("content.xml is missing: not an OpenDocument file")?;
    if !mime.is_empty() && !mime.contains(expect_mime) && !mime.contains("opendocument") {
        return Err(format!("unexpected document type {mime}"));
    }
    let mut defs = HashMap::new();
    let mut numbered = HashMap::new();
    if let Some(s) = zip.read_str("styles.xml") {
        parse_styles(&s, &mut defs, &mut numbered);
    }
    parse_styles(&content, &mut defs, &mut numbered);
    let mut loader = |path: &str| zip.read(path.trim_start_matches("./"));
    let mut w = Walker {
        defs,
        numbered,
        flow: Flow::default(),
        load: &mut loader,
    };
    w.walk(&xml::events(&content));
    let mut flow = w.flow;
    flow.build_toc();
    let meta = zip
        .read_str("meta.xml")
        .map(|t| crate::docx::core_meta(&t))
        .unwrap_or_default();
    Ok((flow, meta))
}

pub fn read<R: Read + Seek>(zip: &mut Archive<R>) -> Result<(Flow, Meta), String> {
    read_inner(zip, "text")
}

pub fn read_presentation<R: Read + Seek>(zip: &mut Archive<R>) -> Result<(Flow, Meta), String> {
    read_inner(zip, "presentation")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{tiny_png, zip_bytes};

    #[test]
    fn odt_headings_lists_tables_images() {
        let content = r##"<office:document-content xmlns:office="o" xmlns:text="t" xmlns:table="b" xmlns:style="s" xmlns:fo="f" xmlns:draw="d" xmlns:xlink="x">
<office:automatic-styles>
<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
<text:list-style style:name="L1"><text:list-level-style-number text:level="1"/></text:list-style>
</office:automatic-styles>
<office:body><office:text>
<text:h text:outline-level="1">Заголовок</text:h>
<text:p>Обычный <text:span text:style-name="T1">жирный</text:span> и <text:a xlink:href="https://x.org">ссылка</text:a>.</text:p>
<text:list text:style-name="L1"><text:list-item><text:p>раз</text:p></text:list-item><text:list-item><text:p>два</text:p></text:list-item></text:list>
<text:list><text:list-item><text:p>маркер</text:p></text:list-item></text:list>
<table:table><table:table-row><table:table-cell><text:p>A</text:p></table:table-cell><table:table-cell><text:p>B</text:p></table:table-cell></table:table-row></table:table>
<text:p><draw:frame><draw:image xlink:href="Pictures/a.png"/></draw:frame></text:p>
</office:text></office:body></office:document-content>"##;
        let png = tiny_png();
        let bytes = zip_bytes(&[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("content.xml", content.as_bytes()),
            ("Pictures/a.png", &png),
            ("meta.xml", r#"<m xmlns:dc="d"><dc:title>ОДТ</dc:title></m>"#.as_bytes()),
        ]);
        let mut zip = Archive::new(std::io::Cursor::new(bytes)).unwrap();
        let (flow, meta) = read(&mut zip).unwrap();
        assert_eq!(meta.title.as_deref(), Some("ОДТ"));
        assert!(matches!(&flow.blocks[0], Block::Heading { level: 1, .. }));
        let Block::Paragraph { text, .. } = &flow.blocks[1] else {
            panic!("{:?}", flow.blocks[1])
        };
        assert_eq!(text.text(), "Обычный жирный и ссылка.");
        assert!(text.spans.iter().any(|s| s.style.bold && s.text == "жирный"));
        assert!(text.spans.iter().any(|s| s.style.link.is_some()));
        assert!(matches!(&flow.blocks[2], Block::ListItem { marker, .. } if marker == "1."));
        assert!(matches!(&flow.blocks[3], Block::ListItem { marker, .. } if marker == "2."));
        assert!(matches!(&flow.blocks[4], Block::ListItem { marker, .. } if marker == "•"));
        let Block::Table(t) = &flow.blocks[5] else {
            panic!("{:?}", flow.blocks[5])
        };
        assert_eq!(t.rows[0][1].text.text(), "B");
        assert!(matches!(flow.blocks[6], Block::Image { .. }));
    }
}
