use std::fs::File;
use std::io::Write;
use std::path::Path;

use kokoro_book::book::SourcePosition;
use kokoro_book::input::read_book;
use tempfile::tempdir;
use zip::write::FileOptions;

#[test]
fn follows_a_foreign_spine_items_xhtml_fallback() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("foreign-fallback.epub");
    write_fallback_epub(&path);

    let book = read_book(&path).expect("EPUB foreign-resource fallback");

    assert!(book.text.contains("Fallback chapter body."));
    assert!(book.warnings.iter().any(|warning| {
        warning.contains("diagram.png")
            && warning.contains("fallback.xhtml")
            && warning.contains("fallback")
    }));
    assert!(matches!(
        book.root.children[0]
            .source_range
            .as_ref()
            .map(|range| &range.start),
        Some(SourcePosition::Epub { resource, .. }) if resource == "/EPUB/fallback.xhtml"
    ));
}

fn write_fallback_epub(path: &Path) {
    let file = File::create(path).expect("EPUB fixture");
    let mut zip = zip::ZipWriter::new(file);
    let stored = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    write_entry(&mut zip, "mimetype", b"application/epub+zip", stored);
    write_entry(
        &mut zip,
        "META-INF/container.xml",
        br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        deflated,
    );
    write_entry(
        &mut zip,
        "EPUB/package.opf",
        br#"<?xml version="1.0"?><package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">urn:uuid:fallback</dc:identifier><dc:title>Fallback Book</dc:title><dc:language>en</dc:language><meta property="dcterms:modified">2026-08-10T00:00:00Z</meta></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="foreign" href="diagram.png" media-type="image/png" fallback="description"/><item id="description" href="fallback.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="foreign"/></spine></package>"#,
        deflated,
    );
    write_entry(
        &mut zip,
        "EPUB/nav.xhtml",
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="fallback.xhtml#chapter">Fallback Chapter</a></li></ol></nav></body></html>"#,
        deflated,
    );
    write_entry(
        &mut zip,
        "EPUB/diagram.png",
        b"\x89PNG\r\n\x1a\n\xff",
        deflated,
    );
    write_entry(
        &mut zip,
        "EPUB/fallback.xhtml",
        br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="chapter">Fallback Chapter</h1><p>Fallback chapter body.</p></body></html>"#,
        deflated,
    );
    zip.finish().expect("finish EPUB fixture");
}

fn write_entry(zip: &mut zip::ZipWriter<File>, path: &str, bytes: &[u8], options: FileOptions) {
    zip.start_file(path, options).expect("EPUB entry");
    zip.write_all(bytes).expect("EPUB entry bytes");
}
