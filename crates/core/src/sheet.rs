//! Spreadsheets (xlsx, xlsm, xlsb, xls, ods) through calamine, and delimited text (csv, tsv).

use crate::model::*;
use calamine::{Data, Reader};
use std::io::Cursor;

fn cell_text(d: &Data) -> String {
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        Data::Float(f) => format!("{f}"),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        Data::DateTime(dt) => match dt.as_datetime() {
            Some(t) => {
                let full = t.format("%Y-%m-%d %H:%M:%S").to_string();
                full.strip_suffix(" 00:00:00").map(str::to_owned).unwrap_or(full)
            }
            None => dt.as_f64().to_string(),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#{e:?}"),
    }
}

pub fn read_workbook(bytes: Vec<u8>) -> Result<Vec<Sheet>, String> {
    let mut wb = calamine::open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|e| format!("cannot read the workbook: {e}"))?;
    let mut sheets = Vec::new();
    for name in wb.sheet_names() {
        let Ok(range) = wb.worksheet_range(&name) else { continue };
        let (r0, c0) = range.start().map(|(r, c)| (r as usize, c as usize)).unwrap_or((0, 0));
        let (r0, c0) = (r0.min(2_000), c0.min(60));
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut cols = 0;
        for _ in 0..r0 {
            rows.push(Vec::new());
        }
        let mut truncated = false;
        for r in range.rows() {
            if rows.len() >= MAX_ROWS {
                truncated = true;
                break;
            }
            let mut line: Vec<String> = vec![String::new(); c0];
            line.extend(r.iter().map(cell_text));
            while line.last().is_some_and(String::is_empty) {
                line.pop();
            }
            cols = cols.max(line.len());
            rows.push(line);
        }
        sheets.push(Sheet {
            name,
            rows,
            cols,
            truncated,
        });
    }
    if sheets.is_empty() {
        return Err("the workbook has no sheets".into());
    }
    Ok(sheets)
}

/// Splits delimited text; understands quotes and doubled quotes.
pub fn parse_delimited(text: &str, delim: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && field.is_empty() {
            quoted = true;
        } else if c == delim {
            row.push(std::mem::take(&mut field));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
            if rows.len() >= MAX_ROWS {
                return rows;
            }
        } else {
            field.push(c);
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

pub fn detect_delimiter(text: &str) -> char {
    let head: String = text.lines().take(20).collect::<Vec<_>>().join("\n");
    [',', ';', '\t', '|']
        .into_iter()
        .max_by_key(|d| head.matches(*d).count())
        .filter(|d| head.contains(*d))
        .unwrap_or(',')
}

pub fn read_csv(text: &str, delim: Option<char>) -> Sheet {
    let delim = delim.unwrap_or_else(|| detect_delimiter(text));
    let rows = parse_delimited(text.trim_start_matches('\u{feff}'), delim);
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let truncated = rows.len() >= MAX_ROWS;
    Sheet {
        name: "CSV".into(),
        rows,
        cols,
        truncated,
    }
}

/// Spreadsheet column name: 0 → A, 26 → AA.
pub fn column_name(mut i: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (i % 26) as u8) as char);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_delimiters_and_newlines() {
        let t = "имя;город\r\n\"Иван \"\"Грозный\"\"\";\"Москва;\nЦентр\"\nПётр;Казань";
        let s = read_csv(t, None);
        assert_eq!(s.rows.len(), 3);
        assert_eq!(s.rows[1][0], "Иван \"Грозный\"");
        assert_eq!(s.rows[1][1], "Москва;\nЦентр");
        assert_eq!(s.cols, 2);
        assert_eq!(detect_delimiter("a\tb\tc\n1\t2\t3"), '\t');
    }

    #[test]
    fn column_names() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
        assert_eq!(column_name(702), "AAA");
    }

    #[test]
    fn xlsx_workbook_is_read() {
        let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>Название</t></is></c><c r="B1"><v>42</v></c></row>
<row r="2"><c r="A2" t="inlineStr"><is><t>x</t></is></c><c r="B2"><v>1.5</v></c></row></sheetData></worksheet>"#;
        let bytes = crate::testkit::zip_bytes(&[
            (
                "[Content_Types].xml",
                br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Лист1" sheetId="1" r:id="rId1"/></sheets></workbook>"#.as_bytes(),
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ]);
        let sheets = read_workbook(bytes).unwrap();
        assert_eq!(sheets[0].name, "Лист1");
        assert_eq!(sheets[0].rows[0], vec!["Название".to_owned(), "42".to_owned()]);
        assert_eq!(sheets[0].rows[1][1], "1.5");
    }
}
