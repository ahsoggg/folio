//! RTF: text, character formatting, alignment, simple tables and embedded PNG/JPEG pictures.

use crate::model::*;
use encoding_rs::{Encoding, IBM866, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252, WINDOWS_1253, WINDOWS_1254};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dest {
    Normal,
    Skip,
    FontTbl,
    ColorTbl,
    Pict,
    ListText,
}

#[derive(Clone)]
struct State {
    dest: Dest,
    style: Style,
    size: i32,
    uc: usize,
    font: usize,
}

struct Reader<'a> {
    flow: &'a mut Flow,
    cur: Rich,
    marker: Option<String>,
    marker_buf: String,
    align: Align,
    max_size: i32,
    fonts: Vec<(usize, String)>,
    font_name: String,
    colors: Vec<Option<[u8; 3]>>,
    color_acc: [Option<u8>; 3],
    color_started: bool,
    pict_kind: Option<bool>,
    pict_hex: Vec<u8>,
    cell_buf: Rich,
    cells: Vec<Cell>,
    rows: Vec<Vec<Cell>>,
    in_tbl: bool,
    encoding: &'static Encoding,
    skip: usize,
    mono_fonts: Vec<usize>,
    pending: Vec<char>,
}

fn encoding_for(cp: u32) -> &'static Encoding {
    match cp {
        1250 => WINDOWS_1250,
        1251 => WINDOWS_1251,
        1253 => WINDOWS_1253,
        1254 => WINDOWS_1254,
        866 => IBM866,
        _ => WINDOWS_1252,
    }
}

impl Reader<'_> {
    fn flush_table(&mut self) {
        if !self.rows.is_empty() {
            let rows = std::mem::take(&mut self.rows);
            self.flow.blocks.push(Block::Table(Table { rows, header_rows: 0 }));
        }
    }

    fn end_paragraph(&mut self) {
        let mut rich = std::mem::take(&mut self.cur);
        rich.trim_end();
        let (marker, size) = (self.marker.take(), std::mem::replace(&mut self.max_size, 0));
        if rich.is_blank() {
            return;
        }
        self.flush_table();
        if let Some(marker) = marker {
            self.flow.blocks.push(Block::ListItem {
                depth: 0,
                marker,
                text: rich,
            });
            return;
        }
        let level = match size {
            s if s >= 36 => Some(1),
            s if s >= 30 => Some(2),
            s if s >= 26 && rich.spans.iter().all(|sp| sp.style.bold || sp.text.trim().is_empty()) => Some(3),
            _ => None,
        };
        if let Some(level) = level
            && rich.text().chars().count() < 160
        {
            self.flow.blocks.push(Block::Heading { level, text: rich });
        } else {
            self.flow.blocks.push(Block::Paragraph {
                text: rich,
                align: self.align,
            });
        }
    }

    fn text(&mut self, st: &State, s: &str) {
        match st.dest {
            Dest::Skip => {}
            Dest::FontTbl => {
                for c in s.chars() {
                    if c == ';' {
                        let name = std::mem::take(&mut self.font_name);
                        if !name.trim().is_empty() {
                            let l = name.to_lowercase();
                            if ["courier", "consolas", "mono", "lucida console"]
                                .iter()
                                .any(|m| l.contains(m))
                            {
                                self.mono_fonts.push(st.font);
                            }
                            self.fonts.push((st.font, name));
                        }
                    } else {
                        self.font_name.push(c);
                    }
                }
            }
            Dest::ColorTbl => {
                for c in s.chars() {
                    if c == ';' {
                        let [r, g, b] = self.color_acc;
                        self.colors.push(if self.color_started {
                            Some([r.unwrap_or(0), g.unwrap_or(0), b.unwrap_or(0)])
                        } else {
                            None
                        });
                        self.color_acc = [None; 3];
                        self.color_started = false;
                    }
                }
            }
            Dest::ListText => self.marker_buf.push_str(s),
            Dest::Pict => {}
            Dest::Normal => {
                let mut style = st.style.clone();
                if self.mono_fonts.contains(&st.font) {
                    style.mono = true;
                }
                self.max_size = self.max_size.max(st.size);
                if self.in_tbl {
                    self.cell_buf.push(s, &style);
                } else {
                    self.cur.push(s, &style);
                }
            }
        }
    }

    fn finish_pict(&mut self) {
        if self.pict_kind.is_some() && !self.pict_hex.is_empty() {
            let mut bytes = Vec::with_capacity(self.pict_hex.len() / 2);
            for pair in self.pict_hex.chunks(2) {
                if pair.len() == 2
                    && let Ok(s) = std::str::from_utf8(pair)
                    && let Ok(b) = u8::from_str_radix(s, 16)
                {
                    bytes.push(b);
                }
            }
            if image::guess_format(&bytes).is_ok() {
                self.end_paragraph();
                let id = self.flow.add_image(bytes);
                self.flow.blocks.push(Block::Image { id, alt: String::new() });
            }
        }
        self.pict_kind = None;
        self.pict_hex.clear();
    }
}

pub fn convert(data: &[u8], flow: &mut Flow) {
    let mut rd = Reader {
        flow,
        cur: Rich::default(),
        marker: None,
        marker_buf: String::new(),
        align: Align::Left,
        max_size: 0,
        fonts: Vec::new(),
        font_name: String::new(),
        colors: Vec::new(),
        color_acc: [None; 3],
        color_started: false,
        pict_kind: None,
        pict_hex: Vec::new(),
        cell_buf: Rich::default(),
        cells: Vec::new(),
        rows: Vec::new(),
        in_tbl: false,
        encoding: WINDOWS_1252,
        skip: 0,
        mono_fonts: Vec::new(),
        pending: Vec::new(),
    };
    let mut stack: Vec<State> = Vec::new();
    let mut st = State {
        dest: Dest::Normal,
        style: Style::default(),
        size: 24,
        uc: 1,
        font: 0,
    };
    let mut i = 0;
    let n = data.len();
    let mut text_buf: Vec<u8> = Vec::new();
    macro_rules! flush_text {
        () => {
            if !text_buf.is_empty() {
                let (s, _) = rd.encoding.decode_without_bom_handling(&text_buf);
                let s = s.into_owned();
                text_buf.clear();
                rd.text(&st, &s);
            }
        };
    }
    while i < n {
        let c = data[i];
        match c {
            b'{' => {
                flush_text!();
                stack.push(st.clone());
                i += 1;
            }
            b'}' => {
                flush_text!();
                if st.dest == Dest::Pict {
                    rd.finish_pict();
                }
                if st.dest == Dest::ListText {
                    let m = rd.marker_buf.trim().to_owned();
                    rd.marker_buf.clear();
                    if !m.is_empty() {
                        rd.marker = Some(m);
                    }
                }
                if let Some(prev) = stack.pop() {
                    st = prev;
                }
                i += 1;
            }
            b'\\' => {
                i += 1;
                if i >= n {
                    break;
                }
                let d = data[i];
                if d.is_ascii_alphabetic() {
                    flush_text!();
                    let start = i;
                    while i < n && data[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word = std::str::from_utf8(&data[start..i]).unwrap_or("");
                    let mut neg = false;
                    if i < n && data[i] == b'-' {
                        neg = true;
                        i += 1;
                    }
                    let pstart = i;
                    while i < n && data[i].is_ascii_digit() {
                        i += 1;
                    }
                    let param: Option<i32> = std::str::from_utf8(&data[pstart..i])
                        .ok()
                        .and_then(|s| s.parse::<i32>().ok())
                        .map(|v| if neg { -v } else { v });
                    if i < n && data[i] == b' ' {
                        i += 1;
                    }
                    control(&mut rd, &mut st, word, param);
                    if !rd.pending.is_empty() {
                        let s: String = std::mem::take(&mut rd.pending).into_iter().collect();
                        rd.text(&st, &s);
                    }
                } else {
                    i += 1;
                    match d {
                        b'\'' => {
                            if i + 1 < n {
                                let hex = std::str::from_utf8(&data[i..i + 2]).unwrap_or("");
                                if let Ok(b) = u8::from_str_radix(hex, 16) {
                                    if rd.skip > 0 {
                                        rd.skip -= 1;
                                    } else {
                                        text_buf.push(b);
                                    }
                                }
                                i += 2;
                            }
                        }
                        b'*' => {
                            flush_text!();
                            // ignorable destination, unless it is one of the few we understand
                            let mut j = i;
                            while j < n && (data[j] == b' ' || data[j] == b'\r' || data[j] == b'\n') {
                                j += 1;
                            }
                            let rest = &data[j..n.min(j + 12)];
                            if rest.starts_with(b"\\listtext") || rest.starts_with(b"\\pntext") {
                                st.dest = Dest::ListText;
                            } else if st.dest == Dest::Normal {
                                st.dest = Dest::Skip;
                            }
                        }
                        b'\\' | b'{' | b'}' => {
                            if rd.skip > 0 {
                                rd.skip -= 1;
                            } else {
                                text_buf.push(d);
                            }
                        }
                        b'~' => {
                            flush_text!();
                            rd.text(&st, "\u{a0}");
                        }
                        b'-' | b'_' => text_buf.push(b'-'),
                        b'\r' | b'\n' => {
                            flush_text!();
                            control(&mut rd, &mut st, "par", None);
                        }
                        _ => {}
                    }
                }
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                if st.dest == Dest::Pict {
                    if c.is_ascii_hexdigit() {
                        rd.pict_hex.push(c);
                    }
                } else if rd.skip > 0 {
                    rd.skip -= 1;
                } else if st.dest == Dest::ColorTbl && c != b';' {
                    // nothing: colour components arrive as control words
                } else {
                    text_buf.push(c);
                }
                i += 1;
            }
        }
    }
    flush_text!();
    rd.end_paragraph();
    rd.flush_table();
}

fn control(rd: &mut Reader, st: &mut State, word: &str, param: Option<i32>) {
    match word {
        "ansicpg" => rd.encoding = encoding_for(param.unwrap_or(1252) as u32),
        "fonttbl" => st.dest = Dest::FontTbl,
        "colortbl" => st.dest = Dest::ColorTbl,
        "stylesheet" | "info" | "header" | "footer" | "headerl" | "headerr" | "footerl" | "footerr" | "object"
        | "themedata" | "colorschememapping" | "latentstyles" | "datastore" | "xmlnstbl" | "listtable"
        | "listoverridetable" | "rsidtbl" | "generator" | "fldinst" | "bkmkstart" | "bkmkend" | "pgdsctbl"
        | "revtbl" | "private" | "mmathPr" | "filetbl" => {
            if st.dest == Dest::Normal {
                st.dest = Dest::Skip;
            }
        }
        "pict" => st.dest = Dest::Pict,
        "pngblip" | "jpegblip" => rd.pict_kind = Some(true),
        "f" if st.dest == Dest::FontTbl => st.font = param.unwrap_or(0) as usize,
        "f" => st.font = param.unwrap_or(0) as usize,
        "fcharset" if st.dest == Dest::FontTbl => {}
        "red" | "green" | "blue" if st.dest == Dest::ColorTbl => {
            rd.color_started = true;
            let v = param.unwrap_or(0).clamp(0, 255) as u8;
            match word {
                "red" => rd.color_acc[0] = Some(v),
                "green" => rd.color_acc[1] = Some(v),
                _ => rd.color_acc[2] = Some(v),
            }
        }
        "plain" => {
            let link = st.style.link.clone();
            st.style = Style {
                link,
                ..Style::default()
            };
            st.size = 24;
        }
        "b" => st.style.bold = param != Some(0),
        "i" => st.style.italic = param != Some(0),
        "ul" | "uld" | "uldb" | "ulw" => st.style.underline = param != Some(0),
        "ulnone" => st.style.underline = false,
        "strike" | "striked" => st.style.strike = param != Some(0),
        "super" => st.style.sup = true,
        "sub" => st.style.sub = true,
        "nosupersub" => {
            st.style.sup = false;
            st.style.sub = false;
        }
        "fs" => st.size = param.unwrap_or(24),
        "cf" => {
            st.style.color = param
                .and_then(|p| rd.colors.get(p as usize).copied().flatten())
                .filter(|c| *c != [0, 0, 0]);
        }
        "highlight" | "cb" => st.style.bg = param.and_then(|p| rd.colors.get(p as usize).copied().flatten()),
        "uc" => st.uc = param.unwrap_or(1).max(0) as usize,
        "u" => {
            if st.dest == Dest::Normal || st.dest == Dest::ListText {
                let v = param.unwrap_or(63) as i64;
                let cp = if v < 0 { (v + 65536) as u32 } else { v as u32 };
                if let Some(ch) = char::from_u32(cp) {
                    rd.pending.push(ch);
                }
                rd.skip = st.uc;
            }
        }
        "par" | "sect" => {
            if rd.in_tbl {
                rd.cell_buf.push("\n", &Style::default());
            } else {
                rd.end_paragraph();
            }
        }
        "line" => {
            let s = st.style.clone();
            if rd.in_tbl {
                rd.cell_buf.push("\n", &s);
            } else {
                rd.cur.push("\n", &s);
            }
        }
        "tab" => {
            let s = st.style.clone();
            rd.cur.push("    ", &s);
        }
        "pagebb" | "page" => {
            rd.end_paragraph();
            rd.flow.blocks.push(Block::PageBreak);
        }
        "pard" => {
            rd.align = Align::Left;
            rd.in_tbl = false;
            rd.marker = None;
        }
        "intbl" => rd.in_tbl = true,
        "ql" => rd.align = Align::Left,
        "qc" => rd.align = Align::Center,
        "qr" => rd.align = Align::Right,
        "qj" => rd.align = Align::Justify,
        "cell" | "nestcell" => {
            let mut t = std::mem::take(&mut rd.cell_buf);
            t.trim_end();
            rd.cells.push(Cell { text: t, colspan: 1 });
        }
        "row" | "nestrow" => {
            if !rd.cells.is_empty() {
                rd.rows.push(std::mem::take(&mut rd.cells));
            }
            rd.in_tbl = false;
        }
        "bullet" => text_symbol(rd, st, "•"),
        "endash" => text_symbol(rd, st, "–"),
        "emdash" => text_symbol(rd, st, "—"),
        "lquote" => text_symbol(rd, st, "‘"),
        "rquote" => text_symbol(rd, st, "’"),
        "ldblquote" => text_symbol(rd, st, "“"),
        "rdblquote" => text_symbol(rd, st, "”"),
        "emspace" | "enspace" | "qmspace" => text_symbol(rd, st, " "),
        _ => {}
    }
}

fn text_symbol(rd: &mut Reader, st: &State, s: &str) {
    rd.text(st, s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_formatting_and_unicode() {
        let mut flow = Flow::default();
        let rtf = format!(
            r"{{{b}rtf1{b}ansi{b}ansicpg1251{b}deff0{{{b}fonttbl{{{b}f0{b}fnil{b}fcharset204 Times New Roman;}}}}{{{b}colortbl;{b}red255{b}green0{b}blue0;}}{b}pard{b}qc Hello {b}b bold{b}b0  {b}cf1 red{b}cf0  {b}'cf{b}'f0{b}'e8{b}'e2{b}'e5{b}'f2 {b}u1052?{b}u1080?{b}u1088?{b}par Second{b}line line{b}par}}",
            b = '\\'
        );
        convert(rtf.as_bytes(), &mut flow);
        assert_eq!(flow.blocks.len(), 2);
        let Block::Paragraph { text, align } = &flow.blocks[0] else {
            panic!("{:?}", flow.blocks[0])
        };
        assert_eq!(*align, Align::Center);
        assert_eq!(text.text(), "Hello bold red Привет Мир");
        assert!(text.spans.iter().any(|s| s.style.bold && s.text == "bold"));
        assert!(
            text.spans
                .iter()
                .any(|s| s.style.color == Some([255, 0, 0]) && s.text == "red")
        );
        assert_eq!(flow.blocks[1].plain_text(), "Second\nline");
    }

    #[test]
    fn tables_and_skipped_groups() {
        let mut flow = Flow::default();
        convert(
            br"{\rtf1{\*\generator X;}{\info{\title T}}\trowd\intbl a\cell b\cell\row\pard after\par}",
            &mut flow,
        );
        let Block::Table(t) = &flow.blocks[0] else {
            panic!("{:?}", flow.blocks)
        };
        assert_eq!((t.rows.len(), t.rows[0][1].text.text()), (1, "b".to_owned()));
        assert_eq!(flow.blocks[1].plain_text(), "after");
        assert_eq!(flow.blocks.len(), 2);
    }
}
