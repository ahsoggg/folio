//! Word (.docx): paragraphs, headings, lists, tables, images, links and character formatting.

use crate::model::*;
use crate::xml::{self, Ev, attr};
use crate::zipx::{self, Archive};
use std::collections::HashMap;
use std::io::{Read, Seek};

#[derive(Default)]
struct StyleInfo {
    name: String,
    based_on: Option<String>,
}

#[derive(Default)]
struct Numbering {
    /// numId -> abstractNumId
    nums: HashMap<String, String>,
    /// (abstractNumId, level) -> numFmt
    formats: HashMap<(String, usize), String>,
    counters: HashMap<String, Vec<u32>>,
}

impl Numbering {
    fn parse(xml_text: &str) -> Numbering {
        let mut n = Numbering::default();
        let (mut abs, mut lvl, mut num) = (String::new(), 0usize, String::new());
        let mut in_num = false;
        for ev in xml::events(xml_text) {
            match ev {
                Ev::Start { name, attrs } => match name.as_str() {
                    "abstractNum" => abs = attr(&attrs, "abstractNumId").unwrap_or("").to_owned(),
                    "lvl" => lvl = attr(&attrs, "ilvl").and_then(|v| v.parse().ok()).unwrap_or(0),
                    "numFmt" => {
                        n.formats
                            .insert((abs.clone(), lvl), attr(&attrs, "val").unwrap_or("decimal").to_owned());
                    }
                    "num" => {
                        in_num = true;
                        num = attr(&attrs, "numId").unwrap_or("").to_owned();
                    }
                    "abstractNumId" if in_num => {
                        n.nums.insert(num.clone(), attr(&attrs, "val").unwrap_or("").to_owned());
                    }
                    _ => {}
                },
                Ev::End { name } if name == "num" => in_num = false,
                _ => {}
            }
        }
        n
    }

    fn marker(&mut self, num_id: &str, level: usize) -> String {
        let abs = self.nums.get(num_id).cloned().unwrap_or_default();
        let fmt = self
            .formats
            .get(&(abs, level))
            .map(String::as_str)
            .unwrap_or("bullet")
            .to_owned();
        let counters = self.counters.entry(num_id.to_owned()).or_default();
        if counters.len() <= level {
            counters.resize(level + 1, 0);
        }
        counters[level] += 1;
        counters.truncate(level + 1);
        let n = counters[level];
        match fmt.as_str() {
            "bullet" | "none" => "•".to_owned(),
            "lowerLetter" | "russianLower" => format!("{}.", letter(n, false)),
            "upperLetter" | "russianUpper" => format!("{}.", letter(n, true)),
            "lowerRoman" => format!("{}.", roman(n).to_lowercase()),
            "upperRoman" => format!("{}.", roman(n)),
            _ => format!("{n}."),
        }
    }
}

fn letter(n: u32, upper: bool) -> String {
    let c = (b'a' + ((n.max(1) - 1) % 26) as u8) as char;
    if upper {
        c.to_ascii_uppercase().to_string()
    } else {
        c.to_string()
    }
}

pub(crate) fn roman(mut n: u32) -> String {
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, s) in table {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

fn on(attrs: &[(String, String)]) -> bool {
    !matches!(attr(attrs, "val"), Some("0" | "false" | "off" | "none"))
}

fn hex_color(v: &str) -> Option<[u8; 3]> {
    if v.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

fn highlight_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "yellow" => [255, 235, 59],
        "green" => [139, 220, 120],
        "cyan" => [128, 222, 234],
        "magenta" => [240, 150, 220],
        "blue" => [144, 164, 255],
        "red" => [255, 138, 128],
        "lightGray" => [210, 210, 210],
        "darkYellow" => [200, 180, 60],
        _ => return None,
    })
}

#[derive(Default)]
struct Para {
    style_id: Option<String>,
    num: Option<(String, usize)>,
    align: Align,
    outline: Option<u8>,
    break_before: bool,
    break_after: bool,
    rich: Rich,
    images: Vec<usize>,
}

struct Parser<'a> {
    rels: HashMap<String, String>,
    styles: HashMap<String, StyleInfo>,
    numbering: Numbering,
    flow: Flow,
    load_image: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>,
    base: String,
}

impl Parser<'_> {
    fn heading_level(&self, style_id: &str) -> Option<u8> {
        let mut id = style_id.to_owned();
        for _ in 0..6 {
            let info = self.styles.get(&id)?;
            let name = info.name.to_lowercase();
            if let Some(n) = name
                .strip_prefix("heading ")
                .or_else(|| name.strip_prefix("заголовок "))
                && let Ok(l) = n.trim().parse::<u8>()
            {
                return Some(l.clamp(1, 6));
            }
            match name.as_str() {
                "title" | "название" => return Some(1),
                "subtitle" | "подзаголовок" => return Some(2),
                _ => {}
            }
            id = info.based_on.clone()?;
        }
        None
    }

    fn add_image(&mut self, rel_id: &str) -> Option<usize> {
        let target = self.rels.get(rel_id)?.clone();
        let path = zipx::resolve(&self.base, &target);
        let lower = path.to_ascii_lowercase();
        if ![".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".tif", ".tiff"]
            .iter()
            .any(|e| lower.ends_with(e))
        {
            return None;
        }
        let bytes = (self.load_image)(&path)?;
        Some(self.flow.add_image(bytes))
    }

    /// `evs[start]` is the `p` start; returns the paragraph and the index after its end.
    fn paragraph(&mut self, evs: &[Ev], start: usize) -> (Para, usize) {
        let mut p = Para::default();
        let (mut depth, mut i) = (0usize, start);
        let (mut in_ppr, mut in_rpr, mut in_t) = (false, false, false);
        let mut link: Option<std::sync::Arc<str>> = None;
        let mut run = Style::default();
        while i < evs.len() {
            match &evs[i] {
                Ev::Start { name, attrs } => match name.as_str() {
                    "p" => {
                        depth += 1;
                        if depth > 1 {
                            p.rich.push(" ", &Style::default());
                        }
                    }
                    "pPr" => in_ppr = true,
                    "rPr" => in_rpr = true,
                    "pStyle" if in_ppr => p.style_id = attr(attrs, "val").map(str::to_owned),
                    "ilvl" if in_ppr => {
                        let l = attr(attrs, "val").and_then(|v| v.parse().ok()).unwrap_or(0);
                        p.num = Some((p.num.take().map(|n| n.0).unwrap_or_default(), l));
                    }
                    "numId" if in_ppr => {
                        let id = attr(attrs, "val").unwrap_or("0").to_owned();
                        let lvl = p.num.take().map(|n| n.1).unwrap_or(0);
                        p.num = if id == "0" { None } else { Some((id, lvl)) };
                    }
                    "jc" if in_ppr => {
                        p.align = match attr(attrs, "val") {
                            Some("center") => Align::Center,
                            Some("right" | "end") => Align::Right,
                            Some("both" | "distribute") => Align::Justify,
                            _ => Align::Left,
                        }
                    }
                    "outlineLvl" if in_ppr => {
                        if let Some(l) = attr(attrs, "val").and_then(|v| v.parse::<u8>().ok())
                            && l < 9
                        {
                            p.outline = Some(l + 1);
                        }
                    }
                    "pageBreakBefore" if in_ppr => p.break_before = on(attrs),
                    "r" => {
                        run = Style {
                            link: link.clone(),
                            underline: link.is_some(),
                            ..Style::default()
                        }
                    }
                    "b" | "bCs" if in_rpr && !in_ppr => run.bold = on(attrs),
                    "i" | "iCs" if in_rpr && !in_ppr => run.italic = on(attrs),
                    "u" if in_rpr && !in_ppr => run.underline = on(attrs),
                    "strike" | "dstrike" if in_rpr && !in_ppr => run.strike = on(attrs),
                    "color" if in_rpr && !in_ppr => run.color = attr(attrs, "val").and_then(hex_color),
                    "highlight" if in_rpr && !in_ppr => run.bg = attr(attrs, "val").and_then(highlight_color),
                    "shd" if in_rpr && !in_ppr => {
                        run.bg = run.bg.or(attr(attrs, "fill").and_then(hex_color));
                    }
                    "vertAlign" if in_rpr && !in_ppr => match attr(attrs, "val") {
                        Some("superscript") => run.sup = true,
                        Some("subscript") => run.sub = true,
                        _ => {}
                    },
                    "rFonts" if in_rpr && !in_ppr => {
                        let f = attr(attrs, "ascii")
                            .or(attr(attrs, "hAnsi"))
                            .unwrap_or("")
                            .to_lowercase();
                        run.mono = ["consolas", "courier", "mono", "lucida console"]
                            .iter()
                            .any(|m| f.contains(m));
                    }
                    "t" | "delText" => in_t = true,
                    "tab" if !in_ppr && !in_rpr => p.rich.push("    ", &run),
                    "br" | "cr" => {
                        if attr(attrs, "type") == Some("page") {
                            p.break_after = true;
                        } else {
                            p.rich.push("\n", &run);
                        }
                    }
                    "noBreakHyphen" => p.rich.push("-", &run),
                    "hyperlink" => {
                        link = attr(attrs, "id")
                            .and_then(|id| self.rels.get(id))
                            .map(|t| std::sync::Arc::from(t.as_str()));
                    }
                    "blip" => {
                        if let Some(id) = attr(attrs, "embed")
                            && let Some(img) = self.add_image(id)
                        {
                            p.images.push(img);
                        }
                    }
                    _ => {}
                },
                Ev::End { name } => match name.as_str() {
                    "p" => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return (p, i + 1);
                        }
                    }
                    "pPr" => in_ppr = false,
                    "rPr" => in_rpr = false,
                    "t" | "delText" => in_t = false,
                    "hyperlink" => link = None,
                    _ => {}
                },
                Ev::Text(s) if in_t => p.rich.push(s, &run),
                Ev::Text(_) => {}
            }
            i += 1;
        }
        (p, i)
    }

    fn emit(&mut self, p: Para, out: &mut Vec<Block>) {
        if p.break_before {
            out.push(Block::PageBreak);
        }
        let mut rich = p.rich;
        rich.trim_end();
        let level = p.style_id.as_deref().and_then(|s| self.heading_level(s)).or(p.outline);
        if !rich.is_blank() {
            if let Some(level) = level {
                out.push(Block::Heading { level, text: rich });
            } else if let Some((num, lvl)) = p.num {
                let marker = self.numbering.marker(&num, lvl);
                out.push(Block::ListItem {
                    depth: lvl.min(8) as u8,
                    marker,
                    text: rich,
                });
            } else {
                out.push(Block::Paragraph {
                    text: rich,
                    align: p.align,
                });
            }
        }
        for id in p.images {
            out.push(Block::Image { id, alt: String::new() });
        }
        if p.break_after {
            out.push(Block::PageBreak);
        }
    }

    fn table(&mut self, evs: &[Ev], start: usize) -> (Table, usize) {
        let mut t = Table::default();
        let (mut depth, mut i) = (0usize, start);
        let mut row: Vec<Cell> = Vec::new();
        let mut cell: Option<Cell> = None;
        let mut header_row = false;
        let mut header_open = true;
        while i < evs.len() {
            match &evs[i] {
                Ev::Start { name, attrs } => match name.as_str() {
                    "tbl" => depth += 1,
                    "tr" if depth == 1 => {
                        row = Vec::new();
                        header_row = false;
                    }
                    "tblHeader" if depth == 1 => header_row = on(attrs),
                    "tc" if depth == 1 => {
                        cell = Some(Cell {
                            text: Rich::default(),
                            colspan: 1,
                        })
                    }
                    "gridSpan" if depth == 1 => {
                        if let Some(c) = cell.as_mut() {
                            c.colspan = attr(attrs, "val").and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
                        }
                    }
                    "p" => {
                        let (para, next) = self.paragraph(evs, i);
                        if let Some(c) = cell.as_mut() {
                            if !c.text.spans.is_empty() && !para.rich.is_blank() {
                                c.text.push("\n", &Style::default());
                            }
                            for s in para.rich.spans {
                                c.text.push(&s.text, &s.style);
                            }
                        }
                        i = next;
                        continue;
                    }
                    _ => {}
                },
                Ev::End { name } => match name.as_str() {
                    "tbl" => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return (t, i + 1);
                        }
                    }
                    "tc" if depth == 1 => {
                        if let Some(mut c) = cell.take() {
                            c.text.trim_end();
                            row.push(c);
                        }
                    }
                    "tr" if depth == 1 => {
                        if header_row && header_open {
                            t.header_rows += 1;
                        } else {
                            header_open = false;
                        }
                        t.rows.push(std::mem::take(&mut row));
                    }
                    _ => {}
                },
                Ev::Text(_) => {}
            }
            i += 1;
        }
        (t, i)
    }

    fn body(&mut self, evs: &[Ev]) {
        let mut blocks = Vec::new();
        let mut i = 0;
        while i < evs.len() {
            match &evs[i] {
                Ev::Start { name, .. } if name == "p" => {
                    let (para, next) = self.paragraph(evs, i);
                    self.emit(para, &mut blocks);
                    i = next;
                }
                Ev::Start { name, .. } if name == "tbl" => {
                    let (table, next) = self.table(evs, i);
                    if !table.rows.is_empty() {
                        blocks.push(Block::Table(table));
                    }
                    i = next;
                }
                _ => i += 1,
            }
        }
        self.flow.blocks = blocks;
    }
}

fn parse_rels(text: &str) -> HashMap<String, String> {
    xml::events(text)
        .into_iter()
        .filter_map(|e| match e {
            Ev::Start { name, attrs } if name == "Relationship" => {
                Some((attr(&attrs, "Id")?.to_owned(), attr(&attrs, "Target")?.to_owned()))
            }
            _ => None,
        })
        .collect()
}

fn parse_styles(text: &str) -> HashMap<String, StyleInfo> {
    let mut map = HashMap::new();
    let mut cur: Option<String> = None;
    for e in xml::events(text) {
        match e {
            Ev::Start { name, attrs } => match name.as_str() {
                "style" => {
                    cur = attr(&attrs, "styleId").map(str::to_owned);
                    if let Some(id) = &cur {
                        map.insert(id.clone(), StyleInfo::default());
                    }
                }
                "name" => {
                    if let Some(info) = cur.as_ref().and_then(|c| map.get_mut(c)) {
                        info.name = attr(&attrs, "val").unwrap_or("").to_owned();
                    }
                }
                "basedOn" => {
                    if let Some(info) = cur.as_ref().and_then(|c| map.get_mut(c)) {
                        info.based_on = attr(&attrs, "val").map(str::to_owned);
                    }
                }
                _ => {}
            },
            Ev::End { name } if name == "style" => cur = None,
            _ => {}
        }
    }
    map
}

pub(crate) fn core_meta(text: &str) -> Meta {
    let mut meta = Meta::default();
    let mut tag = String::new();
    for e in xml::events(text) {
        match e {
            Ev::Start { name, .. } => tag = name,
            Ev::Text(t) if !t.trim().is_empty() => match tag.as_str() {
                "title" => meta.title = Some(t.trim().to_owned()),
                "creator" => meta.author = Some(t.trim().to_owned()),
                _ => {}
            },
            _ => {}
        }
    }
    meta
}

pub fn read<R: Read + Seek>(zip: &mut Archive<R>) -> Result<(Flow, Meta), String> {
    let main = zip
        .read_str("word/document.xml")
        .ok_or("word/document.xml is missing: not a Word document")?;
    let rels = zip
        .read_str("word/_rels/document.xml.rels")
        .map(|t| parse_rels(&t))
        .unwrap_or_default();
    let styles = zip
        .read_str("word/styles.xml")
        .map(|t| parse_styles(&t))
        .unwrap_or_default();
    let numbering = zip
        .read_str("word/numbering.xml")
        .map(|t| Numbering::parse(&t))
        .unwrap_or_default();
    let meta = zip
        .read_str("docProps/core.xml")
        .map(|t| core_meta(&t))
        .unwrap_or_default();
    let mut loader = |path: &str| zip.read(path);
    let mut parser = Parser {
        rels,
        styles,
        numbering,
        flow: Flow::default(),
        load_image: &mut loader,
        base: "word/document.xml".into(),
    };
    parser.body(&xml::events(&main));
    let mut flow = parser.flow;
    flow.build_toc();
    Ok((flow, meta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    #[test]
    fn reads_headings_runs_lists_tables_and_images() {
        let mut zip = Archive::new(std::io::Cursor::new(testkit::sample_docx())).unwrap();
        let (flow, meta) = read(&mut zip).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Тестовый документ"));
        assert_eq!(meta.author.as_deref(), Some("Автор"));
        assert!(matches!(&flow.blocks[0], Block::Heading { level: 1, text } if text.text() == "Глава первая"));
        let Block::Paragraph { text, .. } = &flow.blocks[1] else {
            panic!("paragraph expected, got {:?}", flow.blocks[1])
        };
        assert_eq!(text.text(), "Обычный жирный курсив".to_owned() + "ссылка");
        assert!(text.spans.iter().any(|s| s.style.bold && s.text == "жирный"));
        assert!(
            text.spans
                .iter()
                .any(|s| s.style.italic && s.style.color == Some([255, 0, 0]))
        );
        assert!(
            text.spans
                .iter()
                .any(|s| s.style.link.as_deref() == Some("https://example.com/") && s.text == "ссылка")
        );
        assert!(matches!(&flow.blocks[2], Block::ListItem { marker, .. } if marker == "1."));
        assert!(matches!(&flow.blocks[3], Block::ListItem { marker, .. } if marker == "•"));
        let Block::Table(t) = &flow.blocks[4] else {
            panic!("table expected")
        };
        assert_eq!((t.rows.len(), t.rows[0].len(), t.header_rows), (2, 2, 1));
        assert_eq!(t.rows[1][1].text.text(), "B2");
        assert!(matches!(flow.blocks[5], Block::Image { id: 0, .. }));
        assert_eq!(flow.images.len(), 1);
        assert!(matches!(flow.blocks[6], Block::PageBreak));
        assert_eq!(flow.toc.len(), 1);
        assert_eq!(flow.toc[0].title, "Глава первая");
    }
}
