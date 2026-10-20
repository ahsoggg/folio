//! Builders for small test documents (used by the unit tests of the readers and by the app's tests).

use std::io::Write;
use std::path::{Path, PathBuf};

/// A ZIP file with the given members, stored uncompressed or deflated.
pub fn zip_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(&mut out);
    let opts = zip::write::SimpleFileOptions::default();
    for (name, data) in members {
        zw.start_file(*name, opts).expect("start file");
        zw.write_all(data).expect("write member");
    }
    zw.finish().expect("finish zip");
    out.into_inner()
}

pub fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).expect("write fixture");
    p
}

/// A 2x2 PNG.
pub fn tiny_png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(2, 2, |x, y| image::Rgba([(x * 200) as u8, (y * 200) as u8, 90, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).expect("encode png");
    buf.into_inner()
}

pub const DOCX_DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Глава первая</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">Обычный </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>жирный</w:t></w:r><w:r><w:rPr><w:i/><w:color w:val="FF0000"/></w:rPr><w:t xml:space="preserve"> курсив</w:t></w:r>
<w:hyperlink r:id="rId5"><w:r><w:t>ссылка</w:t></w:r></w:hyperlink></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>первый пункт</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>маркер</w:t></w:r></w:p>
<w:tbl><w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>A2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B2</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
<w:p><w:r><w:drawing><a:graphic><a:graphicData><a:blip r:embed="rId9"/></a:graphicData></a:graphic></w:drawing></w:r></w:p>
<w:p><w:r><w:br w:type="page"/></w:r></w:p>
<w:p><w:r><w:t>Вторая страница</w:t></w:r></w:p>
</w:body></w:document>"#;

pub const DOCX_STYLES: &str = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#;

pub const DOCX_NUMBERING: &str = r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#;

pub const DOCX_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/" TargetMode="External"/>
<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#;

pub fn sample_docx() -> Vec<u8> {
    let png = tiny_png();
    zip_bytes(&[
        ("word/document.xml", DOCX_DOCUMENT.as_bytes()),
        ("word/styles.xml", DOCX_STYLES.as_bytes()),
        ("word/numbering.xml", DOCX_NUMBERING.as_bytes()),
        ("word/_rels/document.xml.rels", DOCX_RELS.as_bytes()),
        ("word/media/image1.png", &png),
        (
            "docProps/core.xml",
            r#"<cp:coreProperties xmlns:cp="x" xmlns:dc="y"><dc:title>Тестовый документ</dc:title><dc:creator>Автор</dc:creator></cp:coreProperties>"#
                .as_bytes(),
        ),
    ])
}
