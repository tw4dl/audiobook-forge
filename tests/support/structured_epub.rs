use std::fs::File;
use std::io::Write;
use std::path::Path;

use zip::write::FileOptions;

pub(super) const COVER_BYTES: &[u8] = b"\xff\xd8\xff\xe0fixture-cover\xff\xd9";

pub(super) fn write_structured_epub3(path: &Path) {
    write_structured_epub3_with_navigation(path, navigation());
}

pub(super) fn write_structured_epub3_without_page_list(path: &Path) {
    write_structured_epub3_with_navigation(path, navigation_without_page_list());
}

pub(super) fn write_epub3_with_invalid_toc_target(path: &Path) {
    write_structured_epub3_with_navigation(path, invalid_navigation());
}

pub(super) fn write_epub3_with_reversed_toc(path: &Path) {
    write_structured_epub3_with_navigation(path, reversed_navigation());
}

pub(super) fn write_epub3_with_file_only_toc_target(path: &Path) {
    write_structured_epub3_with_navigation(path, file_only_navigation());
}

pub(super) fn write_epub3_with_malformed_navigation(path: &Path) {
    write_structured_epub3_with_navigation(path, malformed_navigation());
}

pub(super) fn write_epub3_with_malformed_navigation_and_tail(path: &Path) {
    write_minimal_epub3(
        path,
        malformed_navigation(),
        &[
            (
                "chapter.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="chapter">Chapter</h1><p>Chapter body.</p></body></html>"#,
            ),
            (
                "tail.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Tail body.</p></body></html>"#,
            ),
        ],
    );
}

pub(super) fn write_epub3_with_deep_navigation(path: &Path) {
    let navigation = deep_navigation(12_000);
    write_structured_epub3_with_navigation(path, navigation.as_bytes());
}

pub(super) fn write_epub3_with_disguised_deep_navigation(path: &Path) {
    let navigation = deep_navigation(12_000);
    write_structured_epub3_with_navigation_path(path, navigation.as_bytes(), "nav");
}

pub(super) fn write_epub3_with_reversed_same_document_toc(path: &Path) {
    write_minimal_epub3(
        path,
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol>
<li><a href="only.xhtml#beta">Beta</a></li>
<li><a href="only.xhtml#alpha">Alpha</a></li>
</ol></nav></body></html>"#,
        &[(
            "only.xhtml",
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<h1 id="alpha">Alpha</h1><p>Alpha body.</p>
<h1 id="beta">Beta</h1><p>Beta body.</p>
</body></html>"#,
        )],
    );
}

pub(super) fn write_epub3_with_inverted_parent_child_toc(path: &Path) {
    write_minimal_epub3(
        path,
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol><li><a href="part.xhtml#part">Part</a><ol>
<li><a href="chapter.xhtml#chapter">Chapter</a></li>
</ol></li></ol></nav></body></html>"#,
        &[
            (
                "chapter.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="chapter">Chapter</h1><p>Chapter body.</p></body></html>"#,
            ),
            (
                "part.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="part">Part</h1><p>Part body.</p></body></html>"#,
            ),
        ],
    );
}

pub(super) fn write_epub3_with_interleaved_toc_groups(path: &Path) {
    write_minimal_epub3(
        path,
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol>
<li><a href="one.xhtml#one">Group A</a><ol>
<li><a href="one.xhtml#one">First</a></li>
<li><a href="three.xhtml#three">Third</a></li>
</ol></li>
<li><a href="two.xhtml#two">Group B</a><ol>
<li><a href="two.xhtml#two">Second</a></li>
</ol></li>
</ol></nav></body></html>"#,
        &[
            (
                "one.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="one">First</h1><p>First body.</p></body></html>"#,
            ),
            (
                "two.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="two">Second</h1><p>Second body.</p></body></html>"#,
            ),
            (
                "three.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="three">Third</h1><p>Third body.</p></body></html>"#,
            ),
        ],
    );
}

pub(super) fn write_epub3_with_unlisted_headingless_tail(path: &Path) {
    write_minimal_epub3(
        path,
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol><li><a href="chapter.xhtml#chapter">Chapter</a></li></ol></nav>
</body></html>"#,
        &[
            (
                "chapter.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="chapter">Chapter</h1><p>Chapter body.</p></body></html>"#,
            ),
            (
                "tail.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Tail body.</p></body></html>"#,
            ),
        ],
    );
}

pub(super) fn write_epub3_with_prose_before_targeted_heading(path: &Path) {
    write_minimal_epub3(
        path,
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol><li><a href="chapter.xhtml#chapter">Chapter</a></li></ol></nav>
</body></html>"#,
        &[(
            "chapter.xhtml",
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Lead prose.</p><h1 id="chapter">Chapter</h1><p>Chapter body.</p></body></html>"#,
        )],
    );
}

pub(super) fn write_structured_epub3_with_container_target(path: &Path) {
    write_minimal_epub3(
        path,
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol><li><a href="chapter.xhtml#chapter-container">The Beginning</a></li></ol></nav>
</body></html>"#,
        &[(
            "chapter.xhtml",
            br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><section epub:type="chapter" id="chapter-container"><h1>The Beginning</h1><p>Chapter body.</p></section></body></html>"#,
        )],
    );
}

pub(super) fn write_epub3_with_headingless_semantic_container(path: &Path) {
    write_minimal_epub3(
        path,
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol/></nav></body></html>"#,
        &[(
            "chapter.xhtml",
            br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><section epub:type="chapter" id="chapter"><p>Headingless chapter body.</p></section></body></html>"#,
        )],
    );
}

pub(super) fn write_epub3_with_tokenized_navigation(path: &Path) {
    write_minimal_epub3(
        path,
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="custom:foo toc"><ol><li><a id="image-chapter" epub:type="chapter custom:foo" href="chapter.xhtml#chapter"><img alt="1" src="label.png"/></a></li></ol></nav><nav epub:type="custom:foo page-list"><ol><li><a href="chapter.xhtml#page"><img alt="42" src="page.png"/></a></li></ol></nav></body></html>"#,
        &[(
            "chapter.xhtml",
            br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><h1 id="chapter">Derived Name</h1><span id="page" epub:type="pagebreak"/><p>Tokenized navigation body.</p></body></html>"#,
        )],
    );
}

pub(super) fn write_public_domain_epub3(path: &Path) {
    write_minimal_epub3_named(
        path,
        "On the Eve excerpt",
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol>
<li><a href="excerpt-two.xhtml#two">Excerpt Two</a></li>
<li><a href="excerpt-one.xhtml#one">Excerpt One</a></li>
</ol></nav></body></html>"#,
        &[
            (
                "excerpt-one.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="one">Excerpt One</h1><p>Why so? inquired Elena.</p></body></html>"#,
            ),
            (
                "excerpt-two.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="two">Excerpt Two</h1><p>One would think you were speaking of some spiteful, disagreeable old woman. She is a pretty young girl.</p></body></html>"#,
            ),
        ],
    );
}

fn write_structured_epub3_with_navigation(path: &Path, navigation: &[u8]) {
    write_structured_epub3_with_navigation_path(path, navigation, "nav.xhtml");
}

fn write_structured_epub3_with_navigation_path(path: &Path, navigation: &[u8], nav_path: &str) {
    let file = File::create(path).expect("EPUB fixture");
    let mut zip = zip::ZipWriter::new(file);
    let stored = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    write_entry(&mut zip, "mimetype", b"application/epub+zip", stored);
    write_entry(
        &mut zip,
        "META-INF/container.xml",
        br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        deflated,
    );
    let package = package(nav_path);
    write_entry(&mut zip, "EPUB/package.opf", package.as_bytes(), deflated);
    write_entry(&mut zip, &format!("EPUB/{nav_path}"), navigation, deflated);
    write_entry(&mut zip, "EPUB/part.xhtml", part(), deflated);
    write_entry(&mut zip, "EPUB/chapter-1.xhtml", chapter_one(), deflated);
    write_entry(&mut zip, "EPUB/chapter-2.xhtml", chapter_two(), deflated);
    write_entry(&mut zip, "EPUB/cover.jpg", COVER_BYTES, deflated);
    zip.finish().expect("finish EPUB fixture");
}

pub(super) fn write_structured_epub2(path: &Path) {
    let file = File::create(path).expect("EPUB fixture");
    let mut zip = zip::ZipWriter::new(file);
    let stored = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    write_entry(&mut zip, "mimetype", b"application/epub+zip", stored);
    write_entry(
        &mut zip,
        "META-INF/container.xml",
        br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        deflated,
    );
    write_entry(&mut zip, "OPS/package.opf", package_epub2(), deflated);
    write_entry(&mut zip, "OPS/toc.ncx", ncx(), deflated);
    write_entry(&mut zip, "OPS/part.xhtml", legacy_part(), deflated);
    write_entry(&mut zip, "OPS/chapter.xhtml", legacy_chapter(), deflated);
    write_entry(&mut zip, "OPS/cover.jpg", COVER_BYTES, deflated);
    zip.finish().expect("finish EPUB fixture");
}

fn write_entry(zip: &mut zip::ZipWriter<File>, path: &str, bytes: &[u8], options: FileOptions) {
    zip.start_file(path, options).expect("EPUB entry");
    zip.write_all(bytes).expect("EPUB entry bytes");
}

fn package(nav_path: &str) -> String {
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="book-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="book-id">urn:uuid:structured-fixture</dc:identifier>
    <dc:title>Navigation by Design</dc:title>
    <dc:creator>Ada Reader</dc:creator>
    <dc:creator>Grace Listener</dc:creator>
    <dc:creator id="illustrator">Ivy Artist</dc:creator>
    <meta refines="#illustrator" property="role" scheme="marc:relators">ill</meta>
    <dc:language>en-US</dc:language>
    <meta property="dcterms:modified">2026-08-10T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="{nav_path}" media-type="application/xhtml+xml" properties="nav"/>
    <item id="cover" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>
    <item id="part" href="part.xhtml" media-type="application/xhtml+xml"/>
    <item id="chapter-1" href="chapter-1.xhtml" media-type="application/xhtml+xml"/>
    <item id="chapter-2" href="chapter-2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="part"/>
    <itemref idref="chapter-1"/>
    <itemref idref="chapter-2"/>
  </spine>
</package>"##
    )
}

fn navigation() -> &'static [u8] {
    br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head><body>
  <nav epub:type="toc" id="toc"><h1>Contents</h1><ol>
    <li><a id="toc-part" epub:type="part" href="part.xhtml#part-one">Part One</a><ol>
      <li><a id="toc-chapter-one" epub:type="chapter" href="chapter-1.xhtml#chapter-one">Chapter One</a><ol>
        <li><a id="toc-why" epub:type="section" href="chapter-1.xhtml#caf%C3%A9">Why It Matters</a></li>
      </ol></li>
      <li><a id="toc-chapter-two" epub:type="chapter" href="chapter-2.xhtml#chapter-two">Chapter Two</a></li>
    </ol></li>
  </ol></nav>
  <nav epub:type="page-list" id="pages"><h2>Pages</h2><ol>
    <li><a href="chapter-1.xhtml#page-7">7</a></li>
    <li><a href="chapter-2.xhtml#page-8">8</a></li>
  </ol></nav>
</body></html>"#
}

fn navigation_without_page_list() -> &'static [u8] {
    br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head><body>
  <nav epub:type="toc" id="toc"><h1>Contents</h1><ol>
    <li><a id="toc-part" epub:type="part" href="part.xhtml#part-one">Part One</a><ol>
      <li><a id="toc-chapter-one" epub:type="chapter" href="chapter-1.xhtml#chapter-one">Chapter One</a><ol>
        <li><a id="toc-why" epub:type="section" href="chapter-1.xhtml#caf%C3%A9">Why It Matters</a></li>
      </ol></li>
      <li><a id="toc-chapter-two" epub:type="chapter" href="chapter-2.xhtml#chapter-two">Chapter Two</a></li>
    </ol></li>
  </ol></nav>
</body></html>"#
}

fn invalid_navigation() -> &'static [u8] {
    br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head><body>
  <nav epub:type="toc" id="toc"><h1>Contents</h1><ol>
    <li><a id="ghost" epub:type="chapter" href="missing.xhtml#ghost">Ghost Chapter</a></li>
    <li><a id="missing-fragment" epub:type="chapter" href="chapter-1.xhtml#absent">Missing Fragment</a></li>
  </ol></nav>
  <nav epub:type="page-list" id="pages"><h2>Pages</h2><ol>
    <li><a href="missing.xhtml#page-99">99</a></li>
  </ol></nav>
</body></html>"#
}

fn reversed_navigation() -> &'static [u8] {
    br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head><body>
  <nav epub:type="toc" id="toc"><h1>Contents</h1><ol>
    <li><a id="toc-part" epub:type="part" href="part.xhtml#part-one">Part One</a><ol>
      <li><a id="toc-chapter-two" epub:type="chapter" href="chapter-2.xhtml#chapter-two">Chapter Two</a></li>
      <li><a id="toc-chapter-one" epub:type="chapter" href="chapter-1.xhtml#chapter-one">Chapter One</a></li>
    </ol></li>
  </ol></nav>
</body></html>"#
}

fn file_only_navigation() -> &'static [u8] {
    br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head><body>
  <nav epub:type="toc" id="toc"><h1>Contents</h1><ol>
    <li><a id="toc-part" epub:type="part" href="part.xhtml">Part One</a><ol>
      <li><a id="toc-chapter-one" epub:type="chapter" href="chapter-1.xhtml#">Chapter One</a><ol>
        <li><a id="toc-why" epub:type="section" href="chapter-1.xhtml#caf%C3%A9">Why It Matters</a></li>
      </ol></li>
      <li><a id="toc-chapter-two" epub:type="chapter" href="chapter-2.xhtml">Chapter Two</a></li>
    </ol></li>
  </ol></nav>
</body></html>"#
}

fn malformed_navigation() -> &'static [u8] {
    br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head><body>
  <nav epub:type="toc"><ol><li><a href="chapter-1.xhtml">Chapter One</a></ol></nav>
</body></html>"#
}

fn deep_navigation(depth: usize) -> String {
    use std::fmt::Write as _;

    let mut navigation = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head><body><nav epub:type="toc"><ol>"#,
    );
    for level in 0..depth {
        write!(
            &mut navigation,
            r#"<li><a id="deep-{level}" href="chapter-1.xhtml#chapter-one">Level {level}</a><ol>"#
        )
        .expect("deep navigation");
    }
    for _ in 0..depth {
        navigation.push_str("</ol></li>");
    }
    navigation.push_str("</ol></nav></body></html>");
    navigation
}

fn part() -> &'static [u8] {
    br#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<h1 id="part-one">Part One</h1><p>The opening of part one.</p>
</body></html>"#
}

fn chapter_one() -> &'static [u8] {
    br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<h1 id="chapter-one">Chapter One</h1><p>First chapter paragraph.</p>
<p id="cdata"><![CDATA[Visible CDATA.]]></p>
<span epub:type="pagebreak" id="page-7" title="7"/>
<h2 id="caf&#xE9;">Why It Matters</h2><p>Navigation preserves meaning.</p>
<aside epub:type="footnote" id="note-1"><p>A concise note.</p></aside>
<aside role="doc-endnote" id="note-2"><p>A closing endnote.</p></aside>
<ol><li epub:type="endnote" id="note-3"><p>A grouped endnote.</p></li></ol>
</body></html>"#
}

fn chapter_two() -> &'static [u8] {
    r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<h1 id="chapter-two">Chapter Two</h1><p>Second chapter paragraph.</p>
<span role="doc-pagebreak" id="page-8" aria-label="8"/>
<!--ééé-->
<span epub:type="pagebreak" aria-label="9"/>
</body></html>"#
        .as_bytes()
}

pub(super) fn chapter_two_page_9_character_offset() -> usize {
    let source = std::str::from_utf8(chapter_two()).expect("XHTML fixture is UTF-8");
    let byte_offset = source
        .find(r#"<span epub:type="pagebreak" aria-label="9"/>"#)
        .expect("page 9 fixture");
    source[..byte_offset].chars().count()
}

pub(super) fn chapter_one_page_7_character_offset() -> usize {
    character_offset(chapter_one(), r#"id="page-7""#)
}

pub(super) fn chapter_two_page_8_character_offset() -> usize {
    character_offset(chapter_two(), r#"id="page-8""#)
}

pub(super) fn legacy_page_11_character_offset() -> usize {
    character_offset(legacy_chapter(), r#"id="page-11""#)
}

fn character_offset(source: &[u8], marker: &str) -> usize {
    let source = std::str::from_utf8(source).expect("XHTML fixture is UTF-8");
    let marker_offset = source.find(marker).expect("page marker fixture");
    let byte_offset = source[..marker_offset]
        .rfind('<')
        .expect("page marker element");
    source[..byte_offset].chars().count()
}

fn write_minimal_epub3(path: &Path, navigation: &[u8], documents: &[(&str, &[u8])]) {
    write_minimal_epub3_named(path, "Ordering Fixture", navigation, documents);
}

fn write_minimal_epub3_named(
    path: &Path,
    title: &str,
    navigation: &[u8],
    documents: &[(&str, &[u8])],
) {
    use std::fmt::Write as _;

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

    let mut manifest = String::from(
        r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
    );
    let mut spine = String::new();
    for (index, (href, _)) in documents.iter().enumerate() {
        write!(
            &mut manifest,
            r#"<item id="doc-{index}" href="{href}" media-type="application/xhtml+xml"/>"#
        )
        .expect("manifest");
        write!(&mut spine, r#"<itemref idref="doc-{index}"/>"#).expect("spine");
    }
    let package = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="book-id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="book-id">urn:uuid:ordering</dc:identifier><dc:title>{title}</dc:title><dc:language>en</dc:language><meta property="dcterms:modified">2026-08-10T00:00:00Z</meta></metadata><manifest>{manifest}</manifest><spine>{spine}</spine></package>"#
    );
    write_entry(&mut zip, "EPUB/package.opf", package.as_bytes(), deflated);
    write_entry(&mut zip, "EPUB/nav.xhtml", navigation, deflated);
    for (href, content) in documents {
        write_entry(&mut zip, &format!("EPUB/{href}"), content, deflated);
    }
    zip.finish().expect("finish EPUB fixture");
}

fn package_epub2() -> &'static [u8] {
    br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="2.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="book-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:identifier id="book-id">urn:uuid:legacy-fixture</dc:identifier>
    <dc:title>Legacy Navigation</dc:title>
    <dc:creator opf:role="aut">Nora Narrator</dc:creator>
    <dc:language>en</dc:language>
    <meta name="cover" content="cover"/>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="cover" href="cover.jpg" media-type="image/jpeg"/>
    <item id="part" href="part.xhtml" media-type="application/xhtml+xml"/>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx"><itemref idref="part"/><itemref idref="chapter"/></spine>
</package>"#
}

fn ncx() -> &'static [u8] {
    br#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head><meta name="dtb:uid" content="urn:uuid:legacy-fixture"/></head>
  <docTitle><text>Legacy Navigation</text></docTitle>
  <navMap id="contents">
    <navPoint id="ncx-part" playOrder="1">
      <navLabel><text>Part Two</text></navLabel><content src="part.xhtml#legacy-part"/>
      <navPoint id="ncx-chapter" playOrder="2">
        <navLabel><text>Chapter Three</text></navLabel><content src="chapter.xhtml#legacy-chapter"/>
      </navPoint>
    </navPoint>
  </navMap>
  <pageList id="pages">
    <navLabel><text>Pages</text></navLabel>
    <pageTarget id="page-11-target" value="11" type="normal" playOrder="3">
      <navLabel><text>11</text></navLabel><content src="chapter.xhtml#page-11"/>
    </pageTarget>
  </pageList>
</ncx>"#
}

fn legacy_part() -> &'static [u8] {
    br#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<h1 id="legacy-part">Part Two</h1>
</body></html>"#
}

fn legacy_chapter() -> &'static [u8] {
    br#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<h1 id="legacy-chapter">Chapter Three</h1><p>An EPUB 2 paragraph.</p>
<span id="page-11"></span>
</body></html>"#
}
