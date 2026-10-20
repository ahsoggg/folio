//! Markdown to blocks.

use crate::html::ImageLoader;
use crate::model::*;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::sync::Arc;

pub fn convert(src: &str, flow: &mut Flow, loader: ImageLoader) {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut cur = Rich::default();
    let mut style = Style::default();
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut marker: Option<String> = None;
    let mut quote = 0u32;
    let mut code: Option<(Option<String>, String)> = None;
    let mut heading: Option<u8> = None;
    let mut table: Option<Table> = None;
    let mut row: Vec<Cell> = Vec::new();
    let mut in_head = false;
    let mut meta = false;
    let mut image: Option<(String, String)> = None;

    let flush = |cur: &mut Rich, marker: &mut Option<String>, lists: &[Option<u64>], quote: u32, flow: &mut Flow| {
        let mut rich = std::mem::take(cur);
        rich.trim_end();
        let m = marker.take();
        if rich.is_blank() {
            return;
        }
        if let Some(marker) = m {
            flow.blocks.push(Block::ListItem {
                depth: lists.len().saturating_sub(1).min(8) as u8,
                marker,
                text: rich,
            });
        } else if quote > 0 {
            flow.blocks.push(Block::Quote(rich));
        } else {
            flow.blocks.push(Block::para(rich));
        }
    };

    for ev in Parser::new_ext(src, opts) {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush(&mut cur, &mut marker, &lists, quote, flow);
                    heading = Some(match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    });
                }
                Tag::Paragraph => {}
                Tag::BlockQuote(_) => {
                    flush(&mut cur, &mut marker, &lists, quote, flow);
                    quote += 1;
                }
                Tag::CodeBlock(kind) => {
                    flush(&mut cur, &mut marker, &lists, quote, flow);
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => l.split_whitespace().next().map(str::to_owned),
                        CodeBlockKind::Indented => None,
                    };
                    code = Some((lang.filter(|l| !l.is_empty()), String::new()));
                }
                Tag::List(start) => {
                    flush(&mut cur, &mut marker, &lists, quote, flow);
                    lists.push(start.map(|s| s.saturating_sub(1)));
                }
                Tag::Item => {
                    flush(&mut cur, &mut marker, &lists, quote, flow);
                    marker = Some(match lists.last_mut() {
                        Some(Some(n)) => {
                            *n += 1;
                            format!("{n}.")
                        }
                        _ => "•".to_owned(),
                    });
                }
                Tag::Table(_) => {
                    flush(&mut cur, &mut marker, &lists, quote, flow);
                    table = Some(Table::default());
                }
                Tag::TableHead => in_head = true,
                Tag::TableRow => row = Vec::new(),
                Tag::TableCell => cur = Rich::default(),
                Tag::Emphasis => style.italic = true,
                Tag::Strong => style.bold = true,
                Tag::Strikethrough => style.strike = true,
                Tag::Link { dest_url, .. } => {
                    if dest_url.starts_with("http") || dest_url.starts_with("mailto:") {
                        style.link = Some(Arc::from(&*dest_url));
                        style.underline = true;
                    }
                }
                Tag::Image { dest_url, .. } => image = Some((dest_url.to_string(), String::new())),
                Tag::MetadataBlock(_) => meta = true,
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    let mut rich = std::mem::take(&mut cur);
                    rich.trim_end();
                    if let (Some(level), false) = (heading.take(), rich.is_blank()) {
                        flow.blocks.push(Block::Heading { level, text: rich });
                    }
                }
                TagEnd::Paragraph => flush(&mut cur, &mut marker, &lists, quote, flow),
                TagEnd::BlockQuote(_) => {
                    flush(&mut cur, &mut marker, &lists, quote, flow);
                    quote = quote.saturating_sub(1);
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, text)) = code.take() {
                        let text = text.trim_end_matches('\n').to_owned();
                        if !text.is_empty() {
                            flow.blocks.push(Block::Code { lang, text, line: 0 });
                        }
                    }
                }
                TagEnd::List(_) => {
                    flush(&mut cur, &mut marker, &lists, quote, flow);
                    lists.pop();
                }
                TagEnd::Item => flush(&mut cur, &mut marker, &lists, quote, flow),
                TagEnd::TableCell => {
                    let mut text = std::mem::take(&mut cur);
                    text.trim_end();
                    row.push(Cell { text, colspan: 1 });
                }
                TagEnd::TableRow | TagEnd::TableHead => {
                    if let Some(t) = table.as_mut() {
                        let finished = std::mem::take(&mut row);
                        if !finished.is_empty() {
                            t.rows.push(finished);
                        }
                        if matches!(tag, TagEnd::TableHead) {
                            t.header_rows = t.rows.len();
                        }
                    }
                    if matches!(tag, TagEnd::TableHead) {
                        in_head = false;
                    }
                }
                TagEnd::Table => {
                    if let Some(t) = table.take()
                        && !t.rows.is_empty()
                    {
                        flow.blocks.push(Block::Table(t));
                    }
                }
                TagEnd::Emphasis => style.italic = false,
                TagEnd::Strong => style.bold = false,
                TagEnd::Strikethrough => style.strike = false,
                TagEnd::Link => {
                    style.link = None;
                    style.underline = false;
                }
                TagEnd::Image => {
                    if let Some((src, alt)) = image.take() {
                        let bytes = if src.starts_with("http") { None } else { loader(&src) };
                        flush(&mut cur, &mut marker, &lists, quote, flow);
                        match bytes {
                            Some(b) => {
                                let id = flow.add_image(b);
                                flow.blocks.push(Block::Image { id, alt });
                            }
                            None if !alt.is_empty() => flow.blocks.push(Block::para(Rich::plain(format!("[{alt}]")))),
                            None => {}
                        }
                    }
                }
                TagEnd::MetadataBlock(_) => meta = false,
                _ => {}
            },
            Event::Text(t) => {
                if meta {
                    continue;
                }
                if let Some((_, buf)) = code.as_mut() {
                    buf.push_str(&t);
                } else if let Some((_, alt)) = image.as_mut() {
                    alt.push_str(&t);
                } else {
                    cur.push(&t, &style);
                }
            }
            Event::Code(t) => {
                let mut s = style.clone();
                s.mono = true;
                cur.push(&t, &s);
            }
            Event::SoftBreak => cur.push(" ", &style),
            Event::HardBreak => cur.push("\n", &style),
            Event::Rule => {
                flush(&mut cur, &mut marker, &lists, quote, flow);
                flow.blocks.push(Block::Rule);
            }
            Event::TaskListMarker(done) => cur.push(if done { "☑ " } else { "☐ " }, &style),
            Event::FootnoteReference(n) => {
                let mut s = style.clone();
                s.sup = true;
                cur.push(&format!("[{n}]"), &s);
            }
            _ => {}
        }
    }
    let _ = in_head;
    flush(&mut cur, &mut marker, &lists, quote, flow);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_blocks() {
        let mut flow = Flow::default();
        let mut none = |_: &str| None;
        convert(
            "# Заголовок\n\nТекст с **жирным**, *курсивом* и `кодом` и [ссылкой](https://a.b).\n\n- один\n- два\n\n1. раз\n2. два\n\n```rust\nfn x() {}\n```\n\n> цитата\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n",
            &mut flow,
            &mut none,
        );
        let b = &flow.blocks;
        assert!(matches!(&b[0], Block::Heading { level: 1, text } if text.text() == "Заголовок"));
        let Block::Paragraph { text, .. } = &b[1] else {
            panic!("{:?}", b[1])
        };
        assert!(text.spans.iter().any(|s| s.style.bold && s.text == "жирным"));
        assert!(text.spans.iter().any(|s| s.style.mono && s.text == "кодом"));
        assert!(text.spans.iter().any(|s| s.style.link.is_some() && s.text == "ссылкой"));
        assert!(matches!(&b[2], Block::ListItem { marker, .. } if marker == "•"));
        assert!(matches!(&b[4], Block::ListItem { marker, .. } if marker == "1."));
        assert!(matches!(&b[6], Block::Code { lang: Some(l), text, .. } if l == "rust" && text == "fn x() {}"));
        assert!(matches!(&b[7], Block::Quote(q) if q.text() == "цитата"));
        let Block::Table(t) = &b[8] else { panic!() };
        assert_eq!(
            (t.rows.len(), t.header_rows, t.rows[1][1].text.text()),
            (2, 1, "2".to_owned())
        );
        assert!(matches!(b[9], Block::Rule));
    }
}
