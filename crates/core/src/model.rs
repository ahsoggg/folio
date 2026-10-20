//! The one document model every non-PDF format is converted into.

use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
    pub sup: bool,
    pub sub: bool,
    pub color: Option<[u8; 3]>,
    pub bg: Option<[u8; 3]>,
    pub link: Option<Arc<str>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

/// A paragraph's worth of styled text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rich {
    pub spans: Vec<Span>,
}

impl Rich {
    pub fn plain(text: impl Into<String>) -> Rich {
        let text = text.into();
        if text.is_empty() {
            return Rich::default();
        }
        Rich {
            spans: vec![Span {
                text,
                style: Style::default(),
            }],
        }
    }

    /// Appends text, merging with the previous span when the style is the same.
    pub fn push(&mut self, text: &str, style: &Style) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut()
            && last.style == *style
        {
            last.text.push_str(text);
            return;
        }
        self.spans.push(Span {
            text: text.to_owned(),
            style: style.clone(),
        });
    }

    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn is_blank(&self) -> bool {
        self.spans.iter().all(|s| s.text.trim().is_empty())
    }

    pub fn trim_end(&mut self) {
        while let Some(last) = self.spans.last_mut() {
            let t = last.text.trim_end().len();
            last.text.truncate(t);
            if last.text.is_empty() {
                self.spans.pop();
            } else {
                break;
            }
        }
    }

    pub fn trim_start(&mut self) {
        while let Some(first) = self.spans.first_mut() {
            let t = first.text.trim_start().to_owned();
            if t.is_empty() {
                self.spans.remove(0);
            } else {
                first.text = t;
                break;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub text: Rich,
    pub colspan: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub rows: Vec<Vec<Cell>>,
    pub header_rows: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        text: Rich,
    },
    Paragraph {
        text: Rich,
        align: Align,
    },
    ListItem {
        depth: u8,
        marker: String,
        text: Rich,
    },
    Quote(Rich),
    /// `line` is the number of the first line, or 0 when lines are not numbered.
    Code {
        lang: Option<String>,
        text: String,
        line: usize,
    },
    Table(Table),
    Image {
        id: usize,
        alt: String,
    },
    Rule,
    PageBreak,
}

impl Block {
    pub fn para(text: Rich) -> Block {
        Block::Paragraph {
            text,
            align: Align::Left,
        }
    }

    /// The text of this block as the search and "copy all" see it.
    pub fn plain_text(&self) -> String {
        match self {
            Block::Heading { text, .. } | Block::Paragraph { text, .. } | Block::ListItem { text, .. } => text.text(),
            Block::Quote(t) => t.text(),
            Block::Code { text, .. } => text.clone(),
            Block::Table(t) => t
                .rows
                .iter()
                .map(|r| r.iter().map(|c| c.text.text()).collect::<Vec<_>>().join("\t"))
                .collect::<Vec<_>>()
                .join("\n"),
            Block::Image { alt, .. } => alt.clone(),
            Block::Rule | Block::PageBreak => String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TocItem {
    pub level: u8,
    pub title: String,
    pub block: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Flow {
    pub blocks: Vec<Block>,
    /// Encoded image files (PNG/JPEG/...), referenced by `Block::Image::id`.
    pub images: Vec<Arc<Vec<u8>>>,
    pub toc: Vec<TocItem>,
}

impl Flow {
    pub fn add_image(&mut self, bytes: Vec<u8>) -> usize {
        self.images.push(Arc::new(bytes));
        self.images.len() - 1
    }

    /// Builds the outline from the headings.
    pub fn build_toc(&mut self) {
        self.toc = self
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| match b {
                Block::Heading { level, text } if !text.is_blank() => Some(TocItem {
                    level: *level,
                    title: text.text().split_whitespace().collect::<Vec<_>>().join(" "),
                    block: i,
                }),
                _ => None,
            })
            .collect();
    }

    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for b in &self.blocks {
            let t = b.plain_text();
            if matches!(b, Block::PageBreak | Block::Rule) {
                out.push('\n');
                continue;
            }
            if let Block::ListItem { marker, depth, .. } = b {
                out.push_str(&"  ".repeat(*depth as usize));
                out.push_str(marker);
                out.push(' ');
            }
            out.push_str(&t);
            out.push('\n');
        }
        out
    }
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    pub cols: usize,
    /// True when the sheet was cut at `MAX_ROWS`.
    pub truncated: bool,
}

pub const MAX_ROWS: usize = 200_000;

#[derive(Clone, Debug)]
pub struct ImagePage {
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub enum Body {
    Flow(Flow),
    Sheets(Vec<Sheet>),
    /// A picture, or the pages of a comic book archive.
    Images(Vec<ImagePage>),
    /// Handled by the PDF engine, not converted.
    Pdf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Pdf,
    Docx,
    Odt,
    Rtf,
    Epub,
    Fb2,
    Markdown,
    Html,
    Text,
    Code,
    Sheet,
    Csv,
    Pptx,
    Odp,
    Json,
    Xml,
    Image,
    Comic,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Pdf => "PDF",
            Kind::Docx => "Word (DOCX)",
            Kind::Odt => "OpenDocument Text",
            Kind::Rtf => "RTF",
            Kind::Epub => "EPUB",
            Kind::Fb2 => "FictionBook (FB2)",
            Kind::Markdown => "Markdown",
            Kind::Html => "HTML",
            Kind::Text => "Text",
            Kind::Code => "Source code",
            Kind::Sheet => "Spreadsheet",
            Kind::Csv => "CSV",
            Kind::Pptx => "PowerPoint (PPTX)",
            Kind::Odp => "OpenDocument Presentation",
            Kind::Json => "JSON",
            Kind::Xml => "XML",
            Kind::Image => "Image",
            Kind::Comic => "Comic book",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub title: Option<String>,
    pub author: Option<String>,
    pub pages: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub kind: Kind,
    pub title: String,
    pub meta: Meta,
    pub body: Body,
    pub size_bytes: u64,
}

impl Document {
    pub fn plain_text(&self) -> String {
        match &self.body {
            Body::Flow(f) => f.plain_text(),
            Body::Sheets(sheets) => sheets
                .iter()
                .map(|s| {
                    let rows = s.rows.iter().map(|r| r.join("\t")).collect::<Vec<_>>().join("\n");
                    format!("# {}\n{rows}\n", s.name)
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Body::Images(_) | Body::Pdf => String::new(),
        }
    }

    pub fn word_count(&self) -> usize {
        self.plain_text().split_whitespace().count()
    }
}
