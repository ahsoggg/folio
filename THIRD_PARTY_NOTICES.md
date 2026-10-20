# Third-party notices

Folio is licensed under MIT OR Apache-2.0.

## pdfium

PDF pages are rendered by **pdfium**, embedded into `Folio.exe` as `pdfium.dll` (Windows x64 build from
[bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries), release `chromium/8111`; see
`assets/pdfium/VERSION.txt` for the exact source and SHA-256). pdfium is licensed under the Apache License 2.0 and BSD-style
licenses; the full text is in `assets/pdfium/LICENSE`, and the licenses of the libraries pdfium itself is built from
(abseil, FreeType, HarfBuzz, ICU, lcms, libjpeg-turbo, OpenJPEG, libpng, zlib and others) are in `assets/pdfium/licenses`.
The binary distribution is accompanied by those notices as the licenses require.

## Rust crates

The remaining dependencies are permissively licensed (MIT, Apache-2.0, BSD, Zlib and similar); `cargo tree` lists them.
Notably: `egui`/`eframe` (MIT/Apache-2.0), `pdfium-render` (MIT/Apache-2.0), `calamine` (MIT), `syntect` (MIT, with
the Sublime Text syntax definitions it bundles under their own permissive licenses), `scraper`/`html5ever` (MIT/Apache-2.0),
`pulldown-cmark` (MIT), `quick-xml` (MIT), `zip` (MIT), `image` (MIT/Apache-2.0), `windows` (MIT/Apache-2.0).

## Fonts

Folio does not ship fonts. It uses the ones installed with Windows (Segoe UI, Georgia, Consolas, Segoe MDL2 Assets and,
for East Asian text, Microsoft YaHei, Yu Gothic or Malgun Gothic) and falls back to the fonts built into egui.
