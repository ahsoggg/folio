//! Plain text and source code, JSON.

use crate::model::*;

const CHUNK_LINES: usize = 120;

/// Source or log text: monospaced chunks with line numbers.
pub fn code_flow(text: &str, lang: Option<&str>) -> Flow {
    let mut flow = Flow::default();
    let lines: Vec<&str> = text.lines().collect();
    for (i, chunk) in lines.chunks(CHUNK_LINES).enumerate() {
        flow.blocks.push(Block::Code {
            lang: lang.map(str::to_owned),
            text: chunk.join("\n"),
            line: i * CHUNK_LINES + 1,
        });
    }
    flow
}

/// Prose: one paragraph per line, blank lines separate. Lines that look like a heading (ALL CAPS, short) stay text.
pub fn prose_flow(text: &str) -> Flow {
    let mut flow = Flow::default();
    for line in text.lines() {
        let t = line.trim_end();
        if t.trim().is_empty() {
            continue;
        }
        let indent = t.len() - t.trim_start().len();
        let mut rich = Rich::plain(t.trim_start());
        if indent >= 4 {
            let s = Style {
                mono: false,
                ..Style::default()
            };
            rich = Rich::default();
            rich.push(&format!("{}{}", "  ".repeat(indent.min(16) / 4), t.trim_start()), &s);
        }
        flow.blocks.push(Block::para(rich));
    }
    flow
}

/// Pretty-printed JSON; invalid JSON is shown as it is.
pub fn json_flow(text: &str) -> Flow {
    let pretty = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok());
    code_flow(pretty.as_deref().unwrap_or(text), Some("json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_chunked_with_line_numbers() {
        let text: String = (1..=250).map(|i| format!("line {i}\n")).collect();
        let f = code_flow(&text, Some("rs"));
        assert_eq!(f.blocks.len(), 3);
        assert!(matches!(&f.blocks[1], Block::Code { line: 121, .. }));
        assert_eq!(f.plain_text().lines().count(), 250);
    }

    #[test]
    fn json_is_pretty_printed_in_original_key_order() {
        let f = json_flow(r#"{"b":1,"a":[1,2]}"#);
        let t = f.plain_text();
        assert!(t.find("\"b\"").unwrap() < t.find("\"a\"").unwrap());
        assert!(t.contains("\n  \"b\": 1"));
    }

    #[test]
    fn prose_skips_blank_lines() {
        assert_eq!(prose_flow("раз\n\n\nдва\n").blocks.len(), 2);
    }
}
