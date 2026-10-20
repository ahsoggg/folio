//! FictionBook 2 (.fb2, .fb2.zip): sections, titles, poems, epigraphs, notes and embedded images.

use crate::model::*;
use crate::xml::{self, Ev, attr};
use base64::Engine;
use std::collections::HashMap;
use std::sync::Arc;

struct Conv {
    flow: Flow,
    images: HashMap<String, Vec<u8>>,
    cur: Rich,
    style: Style,
    depth: u8,
    pending_title: bool,
    align: Align,
    cite: u32,
}

impl Conv {
    fn flush(&mut self) {
        let mut rich = std::mem::take(&mut self.cur);
        rich.trim_start();
        rich.trim_end();
        if rich.is_blank() {
            return;
        }
        if std::mem::take(&mut self.pending_title) {
            let level = (self.depth + 1).clamp(1, 6);
            self.flow.blocks.push(Block::Heading { level, text: rich });
        } else if self.cite > 0 {
            self.flow.blocks.push(Block::Quote(rich));
        } else {
            self.flow.blocks.push(Block::Paragraph {
                text: rich,
                align: self.align,
            });
        }
    }

    fn image(&mut self, href: &str) {
        let id = href.trim_start_matches('#');
        if let Some(bytes) = self.images.get(id) {
            let nid = self.flow.add_image(bytes.clone());
            self.flow.blocks.push(Block::Image {
                id: nid,
                alt: String::new(),
            });
        }
    }
}

fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(ch);
            space = false;
        }
    }
    out
}

pub fn read(text: &str) -> Result<(Flow, Meta), String> {
    let evs = xml::events(text);
    if !evs
        .iter()
        .any(|e| matches!(e, Ev::Start { name, .. } if name == "FictionBook"))
    {
        return Err("not a FictionBook file".into());
    }
    let mut c = Conv {
        flow: Flow::default(),
        images: HashMap::new(),
        cur: Rich::default(),
        style: Style::default(),
        depth: 0,
        pending_title: false,
        align: Align::Left,
        cite: 0,
    };
    let mut meta = Meta::default();
    // pass 1: binaries
    let mut bin: Option<(String, String)> = None;
    for e in &evs {
        match e {
            Ev::Start { name, attrs } if name == "binary" => {
                bin = attr(attrs, "id").map(|id| (id.to_owned(), String::new()));
            }
            Ev::Text(t) => {
                if let Some((_, buf)) = bin.as_mut() {
                    buf.push_str(t);
                }
            }
            Ev::End { name } if name == "binary" => {
                if let Some((id, buf)) = bin.take() {
                    let clean: String = buf.chars().filter(|c| !c.is_whitespace()).collect();
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(clean) {
                        c.images.insert(id, bytes);
                    }
                }
            }
            _ => {}
        }
    }
    // pass 2: description and bodies
    let (mut in_desc, mut in_title_info, mut in_author, mut in_book_title) = (false, false, false, false);
    let mut skip_binary = false;
    for e in evs {
        match e {
            Ev::Start { name, attrs } => match name.as_str() {
                "description" => in_desc = true,
                "title-info" if in_desc => in_title_info = true,
                "author" if in_title_info && meta.author.is_none() => in_author = true,
                "book-title" if in_title_info => in_book_title = true,
                "binary" => skip_binary = true,
                "body" => {
                    if attr(&attrs, "name") == Some("notes") {
                        c.flush();
                        c.flow.blocks.push(Block::PageBreak);
                    }
                }
                "section" if !in_desc => {
                    c.flush();
                    c.depth += 1;
                }
                "title" if !in_desc => {
                    c.flush();
                    c.pending_title = true;
                }
                "p" if !in_desc => {}
                "subtitle" => {
                    c.flush();
                    c.style.bold = true;
                    c.align = Align::Center;
                }
                "epigraph" | "cite" => {
                    c.flush();
                    c.cite += 1;
                }
                "poem" | "stanza" | "text-author" => c.flush(),
                "empty-line" => {
                    c.flush();
                }
                "emphasis" => c.style.italic = true,
                "strong" => c.style.bold = true,
                "strikethrough" => c.style.strike = true,
                "sup" => c.style.sup = true,
                "sub" => c.style.sub = true,
                "code" => c.style.mono = true,
                "a" => {
                    let href = attr(&attrs, "href").unwrap_or("");
                    if href.starts_with("http") {
                        c.style.link = Some(Arc::from(href));
                        c.style.underline = true;
                    } else if !href.is_empty() {
                        c.style.sup = true;
                    }
                }
                "image" if !in_desc => {
                    c.flush();
                    if let Some(h) = attr(&attrs, "href") {
                        c.image(h);
                    }
                }
                _ => {}
            },
            Ev::End { name } => match name.as_str() {
                "description" => in_desc = false,
                "title-info" => in_title_info = false,
                "author" => in_author = false,
                "book-title" => in_book_title = false,
                "binary" => skip_binary = false,
                "section" if !in_desc => {
                    c.flush();
                    c.depth = c.depth.saturating_sub(1);
                }
                "title" if !in_desc => {
                    c.flush();
                    c.pending_title = false;
                }
                "p" | "v" | "text-author" | "subtitle" | "stanza" if !in_desc => {
                    c.flush();
                    if name == "subtitle" {
                        c.style.bold = false;
                        c.align = Align::Left;
                    }
                }
                "epigraph" | "cite" => {
                    c.flush();
                    c.cite = c.cite.saturating_sub(1);
                }
                "emphasis" => c.style.italic = false,
                "strong" => c.style.bold = false,
                "strikethrough" => c.style.strike = false,
                "sup" => c.style.sup = false,
                "sub" => c.style.sub = false,
                "code" => c.style.mono = false,
                "a" => {
                    c.style.link = None;
                    c.style.underline = false;
                    c.style.sup = false;
                }
                _ => {}
            },
            Ev::Text(t) => {
                if skip_binary {
                    continue;
                }
                let collapsed = collapse(&t);
                let trimmed = collapsed.trim().to_owned();
                if in_book_title && meta.title.is_none() && !trimmed.is_empty() {
                    meta.title = Some(trimmed);
                } else if in_author && !trimmed.is_empty() {
                    let author = meta.author.get_or_insert_with(String::new);
                    if !author.is_empty() {
                        author.push(' ');
                    }
                    author.push_str(&trimmed);
                } else if !in_desc && (!c.cur.spans.is_empty() || !trimmed.is_empty()) {
                    let style = c.style.clone();
                    c.cur.push(&collapsed, &style);
                }
            }
        }
    }
    c.flush();
    c.flow.build_toc();
    Ok((c.flow, meta))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_titles_and_cover() {
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD.encode(crate::testkit::tiny_png());
        let fb2 = format!(
            r##"<?xml version="1.0" encoding="utf-8"?><FictionBook xmlns:l="http://www.w3.org/1999/xlink"><description><title-info>
<author><first-name>Лев</first-name><last-name>Толстой</last-name></author><book-title>Война и мир</book-title></title-info></description>
<body><title><p>Часть первая</p></title><section><title><p>Глава 1</p></title><p>Первый <emphasis>абзац</emphasis>.</p><empty-line/>
<image l:href="#pic"/><p>Второй.</p></section></body><binary id="pic" content-type="image/png">{png}</binary></FictionBook>"##
        );
        let (flow, meta) = read(&fb2).unwrap();
        assert_eq!(
            (meta.title.as_deref(), meta.author.as_deref()),
            (Some("Война и мир"), Some("Лев Толстой"))
        );
        assert!(matches!(&flow.blocks[0], Block::Heading { level: 1, text } if text.text() == "Часть первая"));
        assert!(matches!(&flow.blocks[1], Block::Heading { level: 2, text } if text.text() == "Глава 1"));
        assert_eq!(flow.blocks[2].plain_text(), "Первый абзац.");
        assert!(matches!(flow.blocks[3], Block::Image { .. }));
        assert_eq!(flow.blocks[4].plain_text(), "Второй.");
        assert_eq!(flow.toc.len(), 2);
    }
}
