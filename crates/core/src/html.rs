//! HTML/XHTML to blocks. Used for .html files and for the chapters of an EPUB.

use crate::model::*;
use scraper::{ElementRef, Html, Node};
use std::sync::Arc;

const MAX_DEPTH: usize = 150;

pub type ImageLoader<'a> = &'a mut dyn FnMut(&str) -> Option<Vec<u8>>;

struct Conv<'a> {
    flow: &'a mut Flow,
    loader: ImageLoader<'a>,
    cur: Rich,
    style: Style,
    lists: Vec<(bool, u32)>,
    marker: Option<String>,
    quote: u32,
    align: Align,
    depth: usize,
}

fn is_skipped(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "head" | "noscript" | "template" | "svg" | "select" | "button"
    )
}

impl Conv<'_> {
    fn flush(&mut self) {
        let mut rich = std::mem::take(&mut self.cur);
        rich.trim_start();
        rich.trim_end();
        let marker = self.marker.take();
        if rich.is_blank() {
            return;
        }
        if let Some(marker) = marker {
            self.flow.blocks.push(Block::ListItem {
                depth: self.lists.len().saturating_sub(1).min(8) as u8,
                marker,
                text: rich,
            });
        } else if self.quote > 0 {
            self.flow.blocks.push(Block::Quote(rich));
        } else {
            self.flow.blocks.push(Block::Paragraph {
                text: rich,
                align: self.align,
            });
        }
    }

    fn text(&mut self, s: &str) {
        let mut out = String::with_capacity(s.len());
        let mut last_space = self.cur.spans.last().is_none_or(|l| l.text.ends_with([' ', '\n']));
        for c in s.chars() {
            if c.is_whitespace() && c != '\u{a0}' {
                if !last_space {
                    out.push(' ');
                    last_space = true;
                }
            } else {
                out.push(c);
                last_space = false;
            }
        }
        let style = self.style.clone();
        self.cur.push(&out, &style);
    }

    fn inline_rich(&mut self, el: ElementRef) -> Rich {
        let saved = (std::mem::take(&mut self.cur), self.marker.take());
        for child in el.children() {
            self.node(child);
        }
        let mut r = std::mem::replace(&mut self.cur, saved.0);
        self.marker = saved.1;
        r.trim_start();
        r.trim_end();
        r
    }

    fn table(&mut self, el: ElementRef) {
        self.flush();
        let mut t = Table::default();
        let mut stack = vec![el];
        let mut rows: Vec<ElementRef> = Vec::new();
        while let Some(e) = stack.pop() {
            for c in e.children().rev() {
                if let Some(ce) = ElementRef::wrap(c) {
                    match ce.value().name() {
                        "tr" => rows.push(ce),
                        "thead" | "tbody" | "tfoot" => stack.push(ce),
                        _ => {}
                    }
                }
            }
        }
        // `rows` was collected in reverse order of discovery for sections; restore document order
        rows.sort_by_key(|r| r.id());
        for row in rows {
            let mut cells = Vec::new();
            let mut header = true;
            for c in row.children().filter_map(ElementRef::wrap) {
                let name = c.value().name();
                if name != "td" && name != "th" {
                    continue;
                }
                header &= name == "th";
                let colspan = c
                    .value()
                    .attr("colspan")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1usize)
                    .clamp(1, 64);
                cells.push(Cell {
                    text: self.inline_rich(c),
                    colspan,
                });
            }
            if cells.is_empty() {
                continue;
            }
            if header && t.header_rows == t.rows.len() {
                t.header_rows += 1;
            }
            t.rows.push(cells);
        }
        if !t.rows.is_empty() {
            self.flow.blocks.push(Block::Table(t));
        }
    }

    fn image(&mut self, el: ElementRef) {
        self.flush();
        let alt = el.value().attr("alt").unwrap_or("").to_owned();
        let Some(src) = el.value().attr("src").or_else(|| el.value().attr("xlink:href")) else {
            return;
        };
        let bytes = if let Some(data) = src.strip_prefix("data:") {
            use base64::Engine;
            data.split_once("base64,")
                .and_then(|(_, b)| base64::engine::general_purpose::STANDARD.decode(b.trim()).ok())
        } else {
            (self.loader)(src)
        };
        match bytes {
            Some(b) if !b.is_empty() => {
                let id = self.flow.add_image(b);
                self.flow.blocks.push(Block::Image { id, alt });
            }
            _ if !alt.is_empty() => self.flow.blocks.push(Block::para(Rich::plain(format!("[{alt}]")))),
            _ => {}
        }
    }

    fn node(&mut self, n: ego_tree::NodeRef<'_, Node>) {
        match n.value() {
            Node::Text(t) => {
                let t: &str = t;
                self.text(t);
            }
            Node::Element(_) => {
                if let Some(el) = ElementRef::wrap(n) {
                    self.element(el);
                }
            }
            _ => {}
        }
    }

    fn walk_children(&mut self, el: ElementRef) {
        for child in el.children() {
            self.node(child);
        }
    }

    fn with_style(&mut self, el: ElementRef, f: impl FnOnce(&mut Style)) {
        let saved = self.style.clone();
        f(&mut self.style);
        self.walk_children(el);
        self.style = saved;
    }

    fn element(&mut self, el: ElementRef) {
        // hostile files nest tens of thousands of elements deep: stop before the stack does
        if self.depth >= MAX_DEPTH {
            return;
        }
        self.depth += 1;
        self.element_inner(el);
        self.depth -= 1;
    }

    fn element_inner(&mut self, el: ElementRef) {
        let name = el.value().name();
        if is_skipped(name) || el.value().attr("hidden").is_some() {
            return;
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                self.walk_children(el);
                let mut rich = std::mem::take(&mut self.cur);
                rich.trim_start();
                rich.trim_end();
                if !rich.is_blank() {
                    let level = name[1..].parse().unwrap_or(1);
                    self.flow.blocks.push(Block::Heading { level, text: rich });
                }
            }
            "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "figure" | "figcaption" | "aside"
            | "nav" | "address" | "details" | "summary" | "dl" | "dt" | "dd" | "center" | "body" | "html" | "form" => {
                self.flush();
                let saved = self.align;
                let align = el
                    .value()
                    .attr("align")
                    .map(str::to_owned)
                    .or_else(|| el.value().attr("style").map(|s| s.replace(' ', "")));
                if let Some(a) = align {
                    if a.contains("center") {
                        self.align = Align::Center;
                    } else if a.contains("right") {
                        self.align = Align::Right;
                    } else if a.contains("justify") {
                        self.align = Align::Justify;
                    }
                }
                if name == "center" {
                    self.align = Align::Center;
                }
                self.walk_children(el);
                self.flush();
                self.align = saved;
            }
            "br" => {
                let style = self.style.clone();
                self.cur.push("\n", &style);
            }
            "hr" => {
                self.flush();
                self.flow.blocks.push(Block::Rule);
            }
            "ul" | "ol" => {
                self.flush();
                let start = el.value().attr("start").and_then(|v| v.parse().ok()).unwrap_or(1u32);
                self.lists.push((name == "ol", start.saturating_sub(1)));
                self.walk_children(el);
                self.flush();
                self.lists.pop();
            }
            "li" => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some((true, n)) => {
                        *n += 1;
                        format!("{n}.")
                    }
                    _ => "•".to_owned(),
                };
                self.marker = Some(marker);
                self.walk_children(el);
                self.flush();
            }
            "pre" => {
                self.flush();
                let text: String = el.text().collect();
                let lang = el
                    .children()
                    .filter_map(ElementRef::wrap)
                    .find_map(|c| {
                        c.value()
                            .attr("class")?
                            .split_whitespace()
                            .find_map(|k| k.strip_prefix("language-"))
                    })
                    .map(str::to_owned);
                let text = text.trim_matches('\n').to_owned();
                if !text.trim().is_empty() {
                    self.flow.blocks.push(Block::Code { lang, text, line: 0 });
                }
            }
            "blockquote" => {
                self.flush();
                self.quote += 1;
                self.walk_children(el);
                self.flush();
                self.quote -= 1;
            }
            "table" => self.table(el),
            "img" => self.image(el),
            "a" => {
                let href = el
                    .value()
                    .attr("href")
                    .filter(|h| h.starts_with("http") || h.starts_with("mailto:"));
                let link = href.map(Arc::<str>::from);
                self.with_style(el, |s| {
                    if link.is_some() {
                        s.link = link;
                        s.underline = true;
                    }
                });
            }
            "b" | "strong" => self.with_style(el, |s| s.bold = true),
            "i" | "em" | "cite" | "dfn" | "var" => self.with_style(el, |s| s.italic = true),
            "u" | "ins" => self.with_style(el, |s| s.underline = true),
            "s" | "strike" | "del" => self.with_style(el, |s| s.strike = true),
            "code" | "kbd" | "samp" | "tt" => self.with_style(el, |s| s.mono = true),
            "sup" => self.with_style(el, |s| s.sup = true),
            "sub" => self.with_style(el, |s| s.sub = true),
            "mark" => self.with_style(el, |s| s.bg = Some([255, 235, 59])),
            _ => self.walk_children(el),
        }
    }
}

/// Converts `html` and appends the result to `flow`. `loader` fetches images by their `src`.
pub fn convert(html: &str, flow: &mut Flow, loader: ImageLoader) {
    let doc = Html::parse_document(html);
    let mut conv = Conv {
        flow,
        loader,
        cur: Rich::default(),
        style: Style::default(),
        lists: Vec::new(),
        marker: None,
        quote: 0,
        align: Align::Left,
        depth: 0,
    };
    let root = doc.root_element();
    // only the body: <head><title> must not appear as text
    let body = root
        .children()
        .filter_map(ElementRef::wrap)
        .find(|e| e.value().name() == "body")
        .unwrap_or(root);
    conv.element(body);
    conv.flush();
}

pub fn title_of(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let sel = scraper::Selector::parse("title").ok()?;
    let t = doc.select(&sel).next()?.text().collect::<String>();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    (!t.is_empty()).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(html: &str) -> Flow {
        let mut flow = Flow::default();
        let mut none = |_: &str| None;
        convert(html, &mut flow, &mut none);
        flow
    }

    #[test]
    fn blocks_from_html() {
        let f = conv(
            "<html><head><title>T</title></head><body><h1>Заголовок</h1><p>Один  <b>два</b>\n три <a href='https://x.org'>ссылка</a></p>\
             <ul><li>a</li><li>b<ol><li>c</li></ol></li></ul><pre><code class='language-rust'>fn main() {}\n</code></pre>\
             <table><tr><th>H</th></tr><tr><td colspan=2>v</td></tr></table><blockquote>цитата</blockquote><hr></body></html>",
        );
        assert!(matches!(&f.blocks[0], Block::Heading { level: 1, text } if text.text() == "Заголовок"));
        let Block::Paragraph { text, .. } = &f.blocks[1] else {
            panic!("{:?}", f.blocks[1])
        };
        assert_eq!(text.text(), "Один два три ссылка");
        assert!(text.spans.iter().any(|s| s.style.bold && s.text == "два"));
        assert!(
            text.spans
                .iter()
                .any(|s| s.style.link.as_deref() == Some("https://x.org"))
        );
        assert!(matches!(&f.blocks[2], Block::ListItem { marker, depth: 0, .. } if marker == "•"));
        assert!(matches!(&f.blocks[4], Block::ListItem { marker, depth: 1, .. } if marker == "1."));
        assert!(
            matches!(&f.blocks[5], Block::Code { lang: Some(l), text, .. } if l == "rust" && text == "fn main() {}")
        );
        let Block::Table(t) = &f.blocks[6] else { panic!() };
        assert_eq!((t.header_rows, t.rows.len(), t.rows[1][0].colspan), (1, 2, 2));
        assert!(matches!(&f.blocks[7], Block::Quote(q) if q.text() == "цитата"));
        assert!(matches!(f.blocks[8], Block::Rule));
        assert_eq!(f.blocks.len(), 9);
    }

    #[test]
    fn absurdly_deep_nesting_does_not_overflow_the_stack() {
        let html = format!("{}x{}", "<div>".repeat(60_000), "</div>".repeat(60_000));
        let handle = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || conv(&html))
            .unwrap();
        handle.join().expect("no overflow");
    }

    #[test]
    fn title_and_data_images() {
        assert_eq!(
            title_of("<title> Моя  страница </title>").as_deref(),
            Some("Моя страница")
        );
        let png = crate::testkit::tiny_png();
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png);
        let f = conv(&format!(
            "<p>до</p><img src='data:image/png;base64,{b64}' alt='x'><p>после</p>"
        ));
        assert!(matches!(f.blocks[1], Block::Image { id: 0, .. }));
        assert_eq!(f.images.len(), 1);
    }
}
