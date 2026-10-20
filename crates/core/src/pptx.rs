//! PowerPoint (.pptx): every slide becomes a titled section; tables, pictures and speaker notes are kept.

use crate::model::*;
use crate::xml::{self, Ev, attr};
use crate::zipx::{self, Archive};
use std::collections::HashMap;
use std::io::{Read, Seek};

fn rels_of<R: Read + Seek>(zip: &mut Archive<R>, part: &str) -> HashMap<String, String> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let path = format!("{dir}/_rels/{file}.rels");
    let Some(text) = zip.read_str(&path) else {
        return HashMap::new();
    };
    xml::events(&text)
        .into_iter()
        .filter_map(|e| match e {
            Ev::Start { name, attrs } if name == "Relationship" => Some((
                attr(&attrs, "Id")?.to_owned(),
                format!(
                    "{}|{}",
                    attr(&attrs, "Type").unwrap_or("").rsplit('/').next().unwrap_or(""),
                    zipx::resolve(part, attr(&attrs, "Target")?)
                ),
            )),
            _ => None,
        })
        .collect()
}

struct Shape {
    title: bool,
    paras: Vec<Rich>,
}

/// Text of a slide or notes part: shapes, tables and pictures in document order.
fn parse_part<R: Read + Seek>(
    zip: &mut Archive<R>,
    part: &str,
    flow: &mut Flow,
    out: &mut Vec<Block>,
    title: &mut Option<Rich>,
) {
    let Some(text) = zip.read_str(part) else { return };
    let rels = rels_of(zip, part);
    let mut shape: Option<Shape> = None;
    let mut cur = Rich::default();
    let mut style = Style::default();
    let (mut in_p, mut in_t) = (false, false);
    let mut table: Option<Table> = None;
    let (mut row, mut cell): (Vec<Cell>, Option<Rich>) = (Vec::new(), None);
    for e in xml::events(&text) {
        match e {
            Ev::Start { name, attrs } => match name.as_str() {
                "sp" => {
                    shape = Some(Shape {
                        title: false,
                        paras: Vec::new(),
                    })
                }
                "ph" => {
                    if let Some(s) = shape.as_mut() {
                        s.title = matches!(attr(&attrs, "type"), Some("title" | "ctrTitle"));
                    }
                }
                "tbl" => table = Some(Table::default()),
                "tr" => row = Vec::new(),
                "tc" => cell = Some(Rich::default()),
                "gridSpan" => {}
                "p" => {
                    in_p = true;
                    cur = Rich::default();
                }
                "rPr" if in_p => {
                    style = Style {
                        bold: attr(&attrs, "b") == Some("1"),
                        italic: attr(&attrs, "i") == Some("1"),
                        underline: attr(&attrs, "u").is_some_and(|u| u != "none"),
                        strike: attr(&attrs, "strike").is_some_and(|u| u != "noStrike"),
                        ..Style::default()
                    }
                }
                "r" => style = Style::default(),
                "t" if in_p => in_t = true,
                "br" if in_p => cur.push("\n", &style),
                "blip" => {
                    if let Some(id) = attr(&attrs, "embed")
                        && let Some(target) = rels.get(id).and_then(|v| v.split_once('|').map(|x| x.1.to_owned()))
                        && let Some(bytes) = zip.read(&target)
                        && image::guess_format(&bytes).is_ok()
                    {
                        let id = flow.add_image(bytes);
                        out.push(Block::Image { id, alt: String::new() });
                    }
                }
                _ => {}
            },
            Ev::End { name } => match name.as_str() {
                "t" => in_t = false,
                "p" => {
                    in_p = false;
                    let mut rich = std::mem::take(&mut cur);
                    rich.trim_end();
                    if let Some(c) = cell.as_mut() {
                        if !c.spans.is_empty() && !rich.is_blank() {
                            c.push("\n", &Style::default());
                        }
                        for s in rich.spans {
                            c.push(&s.text, &s.style);
                        }
                    } else if let Some(s) = shape.as_mut()
                        && !rich.is_blank()
                    {
                        s.paras.push(rich);
                    }
                }
                "tc" => {
                    if let Some(c) = cell.take() {
                        row.push(Cell { text: c, colspan: 1 });
                    }
                }
                "tr" => {
                    if let Some(t) = table.as_mut() {
                        t.rows.push(std::mem::take(&mut row));
                    }
                }
                "tbl" => {
                    if let Some(t) = table.take()
                        && !t.rows.is_empty()
                    {
                        out.push(Block::Table(t));
                    }
                }
                "sp" => {
                    if let Some(s) = shape.take() {
                        if s.title && title.is_none() {
                            *title = s.paras.into_iter().next();
                        } else {
                            let many = s.paras.len() > 1;
                            for p in s.paras {
                                out.push(if many {
                                    Block::ListItem {
                                        depth: 0,
                                        marker: "•".into(),
                                        text: p,
                                    }
                                } else {
                                    Block::para(p)
                                });
                            }
                        }
                    }
                }
                _ => {}
            },
            Ev::Text(t) if in_t => cur.push(&t, &style),
            Ev::Text(_) => {}
        }
    }
}

fn number(name: &str) -> usize {
    name.chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

pub fn read<R: Read + Seek>(zip: &mut Archive<R>) -> Result<(Flow, Meta), String> {
    let mut slides: Vec<String> = zip
        .names()
        .into_iter()
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .collect();
    if slides.is_empty() {
        return Err("no slides found: not a PowerPoint file".into());
    }
    slides.sort_by_key(|n| number(n));
    let mut flow = Flow::default();
    for (i, slide) in slides.iter().enumerate() {
        if i > 0 {
            flow.blocks.push(Block::PageBreak);
        }
        let mut body = Vec::new();
        let mut title = None;
        parse_part(zip, slide, &mut flow, &mut body, &mut title);
        let heading = title
            .filter(|t| !t.is_blank())
            .unwrap_or_else(|| Rich::plain(format!("Slide {}", i + 1)));
        let mut h = Rich::plain(format!("{}. ", i + 1));
        for s in heading.spans {
            h.push(&s.text, &s.style);
        }
        flow.blocks.push(Block::Heading { level: 2, text: h });
        flow.blocks.extend(body);
        // speaker notes
        let notes = rels_of(zip, slide)
            .values()
            .find_map(|v| v.strip_prefix("notesSlide|").map(str::to_owned));
        if let Some(notes) = notes {
            let (mut nb, mut nt) = (Vec::new(), None);
            parse_part(zip, &notes, &mut flow, &mut nb, &mut nt);
            let text: Vec<String> = nb
                .iter()
                .map(Block::plain_text)
                .filter(|t| !t.trim().is_empty() && t.trim().parse::<u32>().is_err())
                .collect();
            if !text.is_empty() {
                flow.blocks
                    .push(Block::Quote(Rich::plain(format!("Notes: {}", text.join(" ")))));
            }
        }
    }
    flow.build_toc();
    let meta = zip
        .read_str("docProps/core.xml")
        .map(|t| crate::docx::core_meta(&t))
        .unwrap_or_default();
    Ok((flow, meta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::zip_bytes;

    #[test]
    fn slides_in_order_with_titles_tables_and_notes() {
        let slide = |title: &str, body: &str| {
            format!(
                r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>{title}</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:txBody><a:p><a:r><a:rPr b="1"/><a:t>{body}</a:t></a:r></a:p><a:p><a:r><a:t>второй</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
            )
        };
        let (s1, s2, s10) = (slide("Первый", "а"), slide("Второй", "б"), slide("Десятый", "в"));
        let bytes = zip_bytes(&[
            ("ppt/slides/slide10.xml", s10.as_bytes()),
            ("ppt/slides/slide2.xml", s2.as_bytes()),
            ("ppt/slides/slide1.xml", s1.as_bytes()),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                br#"<Relationships><Relationship Id="r1" Type="http://x/notesSlide" Target="../notesSlides/notesSlide1.xml"/></Relationships>"#,
            ),
            (
                "ppt/notesSlides/notesSlide1.xml",
                "<p:notes xmlns:p='p' xmlns:a='a'><p:sp><p:txBody><a:p><a:r><a:t>Говорить медленно</a:t></a:r></a:p></p:txBody></p:sp></p:notes>".as_bytes(),
            ),
        ]);
        let mut zip = Archive::new(std::io::Cursor::new(bytes)).unwrap();
        let (flow, _) = read(&mut zip).unwrap();
        let titles: Vec<_> = flow.toc.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["1. Первый", "2. Второй", "3. Десятый"]);
        assert!(flow.plain_text().contains("Notes: Говорить медленно"));
        assert!(flow.plain_text().contains("а\n"));
    }
}
