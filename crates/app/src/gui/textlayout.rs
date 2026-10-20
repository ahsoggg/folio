//! Rich text → egui layout jobs: reading fonts, colours that stay readable on every theme, search highlights,
//! clickable links and syntax-highlighted code.

use super::theme::{self, Palette};
use eframe::egui::{
    Align, Color32, FontId, Stroke,
    text::{LayoutJob, TextFormat, TextWrapping},
};
use folio_core::{Rich, Style};
use std::ops::Range;
use std::sync::{Arc, OnceLock};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

#[derive(Clone, Copy)]
pub struct Look {
    pub pal: Palette,
    pub base: &'static str,
    pub size: f32,
    pub spacing: f32,
    pub justify: bool,
}

/// What to mark: the lower-cased query and which occurrence inside this block is the current one.
pub struct Marks<'a> {
    pub query: &'a [char],
    pub current: Option<usize>,
}

/// A link target and the byte range of its text inside the job.
pub type LinkSpan = (Range<usize>, Arc<str>);

pub struct Laid {
    pub job: LayoutJob,
    pub links: Vec<LinkSpan>,
}

fn luminance(c: Color32) -> f32 {
    let f = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The author's colour when it can be read on this paper, the theme's ink when it cannot.
pub fn readable(color: [u8; 3], pal: &Palette) -> Color32 {
    let c = Color32::from_rgb(color[0], color[1], color[2]);
    if contrast(c, pal.paper) >= 3.0 { c } else { pal.ink }
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Char positions where `query` starts in `text` (case-insensitive, one char to one char).
pub fn find_all(text: &[char], query: &[char]) -> Vec<usize> {
    if query.is_empty() || text.len() < query.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + query.len() <= text.len() {
        if text[i..i + query.len()].iter().zip(query).all(|(a, b)| lower(*a) == *b) {
            out.push(i);
            i += query.len();
        } else {
            i += 1;
        }
    }
    out
}

pub fn query_chars(q: &str) -> Vec<char> {
    q.chars().map(lower).collect()
}

fn format_for(style: &Style, look: &Look, scale: f32, force_bold: bool, color: Option<Color32>) -> TextFormat {
    let mut size = look.size * scale;
    if style.sup || style.sub {
        size *= 0.72;
    }
    if style.mono {
        size *= 0.92;
    }
    let base = if style.mono { theme::MONO } else { look.base };
    let family = theme::family(base, style.bold || force_bold, style.italic);
    let link = style.link.is_some();
    let color = if link {
        look.pal.link
    } else if let Some(c) = style.color {
        readable(c, &look.pal)
    } else {
        color.unwrap_or(look.pal.ink)
    };
    let line = Stroke::new(1.0, color);
    TextFormat {
        font_id: FontId::new(size, family),
        color,
        background: style
            .bg
            .map(|b| {
                let c = Color32::from_rgb(b[0], b[1], b[2]);
                if look.pal.dark {
                    c.gamma_multiply(0.35)
                } else {
                    c.gamma_multiply(0.8)
                }
            })
            .unwrap_or(Color32::TRANSPARENT),
        underline: if style.underline || link { line } else { Stroke::NONE },
        strikethrough: if style.strike { line } else { Stroke::NONE },
        valign: if style.sup {
            Align::Min
        } else if style.sub {
            Align::Max
        } else {
            Align::Center
        },
        line_height: Some(look.size * scale * look.spacing),
        ..Default::default()
    }
}

/// One paragraph of rich text. `scale` multiplies the body size (headings), `bold` forces the bold face.
pub fn rich_job(
    rich: &Rich,
    look: &Look,
    scale: f32,
    bold: bool,
    color: Option<Color32>,
    marks: Option<&Marks>,
    width: f32,
    align: folio_core::Align,
) -> Laid {
    let mut job = LayoutJob::default();
    let mut links = Vec::new();
    let mut occurrence = 0usize;
    for span in &rich.spans {
        let fmt = format_for(&span.style, look, scale, bold, color);
        let start = job.text.len();
        match marks {
            Some(m) if !m.query.is_empty() => {
                let chars: Vec<char> = span.text.chars().collect();
                let hits = find_all(&chars, m.query);
                let mut at = 0;
                for h in hits {
                    if h > at {
                        job.append(&chars[at..h].iter().collect::<String>(), 0.0, fmt.clone());
                    }
                    let mut hf = fmt.clone();
                    hf.background = if m.current == Some(occurrence) {
                        look.pal.hit_current
                    } else {
                        look.pal.hit
                    };
                    job.append(&chars[h..h + m.query.len()].iter().collect::<String>(), 0.0, hf);
                    occurrence += 1;
                    at = h + m.query.len();
                }
                if at < chars.len() {
                    job.append(&chars[at..].iter().collect::<String>(), 0.0, fmt);
                }
            }
            _ => job.append(&span.text, 0.0, fmt),
        }
        if let Some(l) = &span.style.link {
            links.push((start..job.text.len(), l.clone()));
        }
    }
    job.wrap = TextWrapping {
        max_width: width,
        ..Default::default()
    };
    job.halign = match align {
        folio_core::Align::Center => Align::Center,
        folio_core::Align::Right => Align::Max,
        _ => Align::Min,
    };
    job.justify = look.justify && scale == 1.0 && matches!(align, folio_core::Align::Left | folio_core::Align::Justify);
    Laid { job, links }
}

/// The link under a char index of the laid-out text, if any.
pub fn link_at(laid: &Laid, char_index: usize) -> Option<Arc<str>> {
    let byte = laid.job.text.char_indices().nth(char_index).map(|(b, _)| b)?;
    laid.links
        .iter()
        .find(|(r, _)| r.contains(&byte))
        .map(|(_, l)| l.clone())
}

struct Syntaxes {
    set: SyntaxSet,
    themes: ThemeSet,
}

fn syntaxes() -> &'static Syntaxes {
    static S: OnceLock<Syntaxes> = OnceLock::new();
    S.get_or_init(|| Syntaxes {
        set: SyntaxSet::load_defaults_newlines(),
        themes: ThemeSet::load_defaults(),
    })
}

const MAX_HIGHLIGHT_LINE: usize = 1500;

fn syntax_theme(pal: &Palette) -> &'static str {
    if pal.dark {
        "base16-ocean.dark"
    } else {
        "InspiredGitHub"
    }
}

/// Monospaced code with line numbers; syntax colours when the language is known.
pub fn code_job(text: &str, lang: Option<&str>, first_line: usize, look: &Look, width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::new(look.size * 0.88, theme::family(theme::MONO, false, false));
    let line_h = look.size * 0.88 * (look.spacing.min(1.35));
    let plain = |color: Color32| TextFormat {
        font_id: font.clone(),
        color,
        line_height: Some(line_h),
        ..Default::default()
    };
    let sx = syntaxes();
    let syntax = lang.and_then(|l| {
        sx.set
            .find_syntax_by_extension(l)
            .or_else(|| sx.set.find_syntax_by_token(l))
    });
    let number_width = if first_line > 0 {
        (first_line + text.lines().count()).to_string().len().max(3)
    } else {
        0
    };
    let mut hl = syntax.and_then(|s| {
        let theme = sx.themes.themes.get(syntax_theme(&look.pal))?;
        Some(HighlightLines::new(s, theme))
    });
    for (i, line) in LinesWithEndings::from(text).enumerate() {
        if number_width > 0 {
            job.append(
                &format!("{:>number_width$}  ", first_line + i),
                0.0,
                plain(look.pal.ink_dim.gamma_multiply(0.7)),
            );
        }
        let body = line.trim_end_matches(['\n', '\r']);
        let spans = match hl.as_mut() {
            Some(h) if body.chars().count() <= MAX_HIGHLIGHT_LINE => h.highlight_line(line, &sx.set).ok(),
            _ => None,
        };
        match spans {
            Some(spans) => {
                for (st, s) in spans {
                    let s = s.trim_end_matches(['\n', '\r']);
                    if !s.is_empty() {
                        let c = Color32::from_rgb(st.foreground.r, st.foreground.g, st.foreground.b);
                        job.append(
                            s,
                            0.0,
                            plain(if contrast(c, look.pal.code_bg) >= 3.0 {
                                c
                            } else {
                                look.pal.ink
                            }),
                        );
                    }
                }
            }
            None => job.append(body, 0.0, plain(look.pal.ink)),
        }
        if line.ends_with('\n') {
            job.append("\n", 0.0, plain(look.pal.ink));
        }
    }
    job.wrap = TextWrapping {
        max_width: width,
        ..Default::default()
    };
    job
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ThemeId;
    use folio_core::Span;

    fn look(id: ThemeId) -> Look {
        Look {
            pal: theme::palette(id),
            base: theme::SERIF,
            size: 18.0,
            spacing: 1.4,
            justify: false,
        }
    }

    #[test]
    fn search_is_case_insensitive_and_keeps_text() {
        let text: Vec<char> = "Привет привет ПРИВЕТ".chars().collect();
        assert_eq!(find_all(&text, &query_chars("привет")), vec![0, 7, 14]);
        assert!(find_all(&text, &query_chars("")).is_empty());
    }

    #[test]
    fn marked_job_has_every_hit_and_the_whole_text() {
        let rich = Rich::plain("а Б а б а");
        let q = query_chars("б");
        let laid = rich_job(
            &rich,
            &look(ThemeId::Light),
            1.0,
            false,
            None,
            Some(&Marks {
                query: &q,
                current: Some(1),
            }),
            400.0,
            folio_core::Align::Left,
        );
        assert_eq!(laid.job.text, "а Б а б а");
        let marked = laid
            .job
            .sections
            .iter()
            .filter(|s| s.format.background != Color32::TRANSPARENT)
            .count();
        assert_eq!(marked, 2);
        let current = laid
            .job
            .sections
            .iter()
            .filter(|s| s.format.background == look(ThemeId::Light).pal.hit_current)
            .count();
        assert_eq!(current, 1);
    }

    #[test]
    fn links_are_found_by_char_index() {
        let style = Style {
            link: Some(Arc::from("https://x.org")),
            ..Style::default()
        };
        let rich = Rich {
            spans: vec![
                Span {
                    text: "до ".into(),
                    style: Style::default(),
                },
                Span {
                    text: "ссылка".into(),
                    style,
                },
            ],
        };
        let laid = rich_job(
            &rich,
            &look(ThemeId::Mocha),
            1.0,
            false,
            None,
            None,
            400.0,
            folio_core::Align::Left,
        );
        assert!(link_at(&laid, 1).is_none());
        assert_eq!(link_at(&laid, 5).as_deref(), Some("https://x.org"));
    }

    #[test]
    fn unreadable_author_colours_fall_back_to_the_ink() {
        let dark = theme::palette(ThemeId::Mocha);
        assert_eq!(readable([0, 0, 0], &dark), dark.ink);
        assert_eq!(readable([255, 200, 100], &dark), Color32::from_rgb(255, 200, 100));
        let light = theme::palette(ThemeId::Light);
        assert_eq!(readable([250, 250, 250], &light), light.ink);
    }

    #[test]
    fn code_has_numbers_and_colours() {
        let job = code_job(
            "fn main() {\n    println!(\"x\");\n}\n",
            Some("rs"),
            10,
            &look(ThemeId::Mocha),
            600.0,
        );
        assert!(job.text.starts_with(" 10  fn main"));
        assert!(job.text.contains(" 12  }"));
        let colours: std::collections::HashSet<_> = job.sections.iter().map(|s| s.format.color).collect();
        assert!(colours.len() > 2, "syntax colours expected");
    }
}
