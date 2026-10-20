//! Decoding of text files with unknown encoding (UTF-8/16 with or without BOM, Windows-1251, CP866, ...).

use encoding_rs::{Encoding, IBM866, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1251, WINDOWS_1252};

/// Share of the most common Russian letters among all Cyrillic letters: real text scores high, mojibake low.
fn russian_score(s: &str) -> f32 {
    let mut cyr = 0u32;
    let mut common = 0u32;
    for c in s.chars() {
        if ('\u{430}'..='\u{44f}').contains(&c) || ('\u{410}'..='\u{42f}').contains(&c) || c == 'ё' || c == 'Ё' {
            cyr += 1;
            if "оеаинтсрвлОЕАИНТСРВЛ".contains(c) {
                common += 1;
            }
        }
    }
    if cyr < 4 { 0.0 } else { common as f32 / cyr as f32 }
}

pub fn decode(bytes: &[u8]) -> (String, &'static Encoding) {
    if let Some((enc, bom)) = Encoding::for_bom(bytes) {
        let (s, _) = enc.decode_without_bom_handling(&bytes[bom..]);
        return (s.into_owned(), enc);
    }
    // UTF-16 without BOM: the high bytes of Latin and Cyrillic letters are 0x00 and 0x04, in every other position
    let head = &bytes[..bytes.len().min(512)];
    let high = |skip: usize| {
        let all = head.iter().skip(skip).step_by(2).count();
        let hits = head.iter().skip(skip).step_by(2).filter(|&&b| b == 0 || b == 4).count();
        all >= 4 && hits * 10 >= all * 9
    };
    let (odd_high, even_high) = (high(1), high(0));
    if odd_high && !even_high {
        return (UTF_16LE.decode_without_bom_handling(bytes).0.into_owned(), UTF_16LE);
    }
    if even_high && !odd_high {
        return (UTF_16BE.decode_without_bom_handling(bytes).0.into_owned(), UTF_16BE);
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_owned(), UTF_8);
    }
    let w1251 = WINDOWS_1251.decode_without_bom_handling(bytes).0.into_owned();
    let d866 = IBM866.decode_without_bom_handling(bytes).0.into_owned();
    let (s1251, s866) = (russian_score(&w1251), russian_score(&d866));
    if s1251 >= 0.2 || s866 >= 0.2 {
        return if s866 > s1251 {
            (d866, IBM866)
        } else {
            (w1251, WINDOWS_1251)
        };
    }
    (
        WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned(),
        WINDOWS_1252,
    )
}

/// Like `decode`, but trusts an `encoding="..."` (XML) or `charset=...` (HTML) declaration in the first bytes.
pub fn decode_declared(bytes: &[u8]) -> String {
    if Encoding::for_bom(bytes).is_some() {
        return decode(bytes).0;
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    for key in ["encoding=", "charset="] {
        if let Some(at) = head.find(key) {
            let rest = head[at + key.len()..].trim_start_matches(['"', '\'', ' ']);
            let label: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if let Some(enc) = Encoding::for_label(label.as_bytes())
                && enc != UTF_8
            {
                return enc.decode_without_bom_handling(bytes).0.into_owned();
            }
        }
    }
    decode(bytes).0
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "Привет, мир! Это обычный русский текст для проверки кодировки.";

    #[test]
    fn detects_common_russian_encodings() {
        assert_eq!(decode(TEXT.as_bytes()).0, TEXT);
        assert_eq!(decode(&WINDOWS_1251.encode(TEXT).0).0, TEXT);
        assert_eq!(decode(&IBM866.encode(TEXT).0).0, TEXT);
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(TEXT.as_bytes());
        assert_eq!(decode(&bom).0, TEXT);
        let utf16: Vec<u8> = TEXT.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode(&utf16).0, TEXT);
    }

    #[test]
    fn declared_encoding_wins() {
        let mut xml = b"<?xml version=\"1.0\" encoding=\"windows-1251\"?><a>".to_vec();
        xml.extend_from_slice(&WINDOWS_1251.encode("Да").0);
        xml.extend_from_slice(b"</a>");
        assert!(decode_declared(&xml).ends_with("Да</a>"));
    }
}
