//! A small XML event list on top of quick-xml: local names, resolved entities, empty elements expanded.

use quick_xml::XmlVersion;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

#[derive(Clone, Debug, PartialEq)]
pub enum Ev {
    Start { name: String, attrs: Vec<(String, String)> },
    End { name: String },
    Text(String),
}

pub fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == name || k.rsplit(':').next() == Some(name))
        .map(|(_, v)| v.as_str())
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn predefined(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{a0}",
        _ => return None,
    })
}

/// Parses the whole document. Broken XML yields the events read up to the error.
pub fn events(xml: &str) -> Vec<Ev> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut out: Vec<Ev> = Vec::new();
    let push_text = |out: &mut Vec<Ev>, s: &str| {
        if s.is_empty() {
            return;
        }
        if let Some(Ev::Text(prev)) = out.last_mut() {
            prev.push_str(s);
        } else {
            out.push(Ev::Text(s.to_owned()));
        }
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let attrs = e
                    .attributes()
                    .flatten()
                    .map(|a| {
                        let key = a.key.0.to_owned();
                        let val = a
                            .normalized_value(XmlVersion::Implicit1_0)
                            .map(|v| v.into_owned())
                            .unwrap_or_default();
                        (key, val)
                    })
                    .collect();
                out.push(Ev::Start {
                    name: local(e.name().0).to_owned(),
                    attrs,
                });
            }
            Ok(Event::End(e)) => out.push(Ev::End {
                name: local(e.name().0).to_owned(),
            }),
            Ok(Event::Text(t)) => push_text(&mut out, &t.xml10_content()),
            Ok(Event::CData(t)) => push_text(&mut out, &t.into_inner()),
            Ok(Event::GeneralRef(r)) => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    push_text(&mut out, c.encode_utf8(&mut [0; 4]));
                } else if let Some(s) = predefined(&r.xml10_content()) {
                    push_text(&mut out, s);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_and_empty_elements() {
        let ev = events(r#"<a:p x:id="1"><b/>1 &lt; 2 &amp; &#x41;<![CDATA[<raw>]]></a:p>"#);
        assert_eq!(
            ev[0],
            Ev::Start {
                name: "p".into(),
                attrs: vec![("x:id".into(), "1".into())]
            }
        );
        assert_eq!(
            ev[1],
            Ev::Start {
                name: "b".into(),
                attrs: vec![]
            }
        );
        assert_eq!(ev[2], Ev::End { name: "b".into() });
        assert_eq!(ev[3], Ev::Text("1 < 2 & A<raw>".into()));
        assert_eq!(attr(&[("x:id".to_owned(), "1".to_owned())], "id"), Some("1"));
    }
}
