use std::fs::File;
use std::io::Write;
use std::path::Path;

use zip::write::FileOptions;

pub(super) fn write_utf16_epub3(path: &Path) {
    write_epub(
        path,
        &utf16le(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol><li><a href="chapter.xhtml#chapter">Chapter One</a></li></ol></nav>
</body></html>"#,
        ),
        &utf16le(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
<h1 id="chapter">Chapter One</h1><p>Unicode body.</p>
</body></html>"#,
        ),
    );
}

pub(super) fn write_utf16_deep_navigation_epub3(path: &Path, depth: usize) {
    use std::fmt::Write as _;

    let mut navigation = String::from(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol>"#,
    );
    for level in 0..depth {
        write!(
            &mut navigation,
            r#"<li><a href="chapter.xhtml#chapter">Level {level}</a><ol>"#
        )
        .expect("deep navigation");
    }
    for _ in 0..depth {
        navigation.push_str("</ol></li>");
    }
    navigation.push_str("</ol></nav></body></html>");
    write_epub(
        path,
        &utf16le(&navigation),
        br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="chapter">Chapter One</h1></body></html>"#,
    );
}

fn write_epub(path: &Path, navigation: &[u8], chapter: &[u8]) {
    let file = File::create(path).expect("EPUB fixture");
    let mut zip = zip::ZipWriter::new(file);
    let stored = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    write_entry(&mut zip, "mimetype", b"application/epub+zip", stored);
    write_entry(
        &mut zip,
        "META-INF/container.xml",
        &utf16le(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        ),
        deflated,
    );
    write_entry(
        &mut zip,
        "EPUB/package.opf",
        &utf16le(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="book-id">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:identifier id="book-id">urn:uuid:utf16</dc:identifier><dc:title>UTF-16 Book</dc:title>
<dc:language>en</dc:language><meta property="dcterms:modified">2026-08-10T00:00:00Z</meta>
</metadata><manifest>
<item id="nav" href="nav" media-type="application/xhtml+xml" properties="nav"/>
<item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
</manifest><spine><itemref idref="chapter"/></spine></package>"#,
        ),
        deflated,
    );
    write_entry(&mut zip, "EPUB/nav", navigation, deflated);
    write_entry(&mut zip, "EPUB/chapter.xhtml", chapter, deflated);
    zip.finish().expect("finish EPUB fixture");
}

fn utf16le(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

fn write_entry(zip: &mut zip::ZipWriter<File>, path: &str, bytes: &[u8], options: FileOptions) {
    zip.start_file(path, options).expect("EPUB entry");
    zip.write_all(bytes).expect("EPUB entry bytes");
}
