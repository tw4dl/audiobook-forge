use std::fs::File;
use std::io::Write;
use std::path::Path;

use kokoro_book::input::read_book;
use tempfile::tempdir;
use zip::write::FileOptions;

#[test]
fn reads_and_normalizes_utf8_text() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("book.TXT");
    std::fs::write(&path, "  First line.\r\n\r\nSecond   line.  ").expect("fixture");

    let book = read_book(&path).expect("read text");

    assert_eq!(book.text, "First line.\n\nSecond line.");
}

#[test]
fn rejects_empty_text() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("empty.txt");
    std::fs::write(&path, " \n\t").expect("fixture");

    let error = read_book(&path).expect_err("empty input must fail");
    assert!(error.to_string().contains("contains no readable text"));
}

#[test]
fn rejects_unsupported_input_types() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("book.pdf");
    std::fs::write(&path, b"not a pdf").expect("fixture");

    let error = read_book(&path).expect_err("unsupported input must fail");
    assert!(
        error
            .to_string()
            .contains("supported input types: .epub and .txt")
    );
}

#[test]
fn rejects_invalid_utf8_text() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("book.txt");
    std::fs::write(&path, [0xff, 0xfe]).expect("fixture");

    let error = read_book(&path).expect_err("invalid UTF-8 must fail");
    assert!(error.to_string().contains("failed to read UTF-8 text"));
}

#[test]
fn rejects_a_malformed_epub() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("book.epub");
    std::fs::write(&path, b"not an epub").expect("fixture");

    let error = read_book(&path).expect_err("malformed EPUB must fail");
    assert!(error.to_string().contains("failed to open EPUB"));
}

#[test]
fn reads_epub_spine_in_order_and_ignores_markup() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("sample.epub");
    write_epub_fixture(&path);

    let book = read_book(&path).expect("read epub");

    let first = book.text.find("Chapter One").expect("first chapter");
    let second = book.text.find("Chapter Two").expect("second chapter");
    assert!(first < second);
    assert!(book.text.contains("Hello & goodbye."));
    assert!(!book.text.contains("<h1>"));
}

fn write_epub_fixture(path: &Path) {
    let file = File::create(path).expect("epub file");
    let mut zip = zip::ZipWriter::new(file);
    let stored = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).expect("mimetype entry");
    zip.write_all(b"application/epub+zip").expect("mimetype");

    zip.start_file("META-INF/container.xml", deflated)
        .expect("container entry");
    zip.write_all(
        br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
    )
    .expect("container");

    zip.start_file("OEBPS/content.opf", deflated)
        .expect("package entry");
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Tiny Book</dc:title><dc:identifier id="id">tiny</dc:identifier><dc:language>en</dc:language></metadata>
  <manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest>
  <spine><itemref idref="one"/><itemref idref="two"/></spine>
</package>"#,
    )
    .expect("package");

    zip.start_file("OEBPS/one.xhtml", deflated)
        .expect("chapter one entry");
    zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Chapter One</h1><p>Hello &amp; goodbye.</p></body></html>"#)
        .expect("chapter one");
    zip.start_file("OEBPS/two.xhtml", deflated)
        .expect("chapter two entry");
    zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Chapter Two</h1><p>The end.</p></body></html>"#)
        .expect("chapter two");
    zip.finish().expect("finish epub");
}
