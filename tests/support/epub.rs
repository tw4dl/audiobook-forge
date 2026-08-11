use std::fmt::Write as _;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use zip::write::FileOptions;

pub(super) fn write_epub_fixture(path: &Path) {
    write_epub_fixture_inner(path, EpubFixtureOptions::default());
}

pub(super) fn write_epub_fixture_with_large_entry(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            variant: EpubFixtureVariant::LargeEntry,
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_unsafe_path(path: &Path, unsafe_name: &str) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            unsafe_name: Some(unsafe_name),
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_encryption_manifest(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            encryption_algorithm: Some("http://www.w3.org/2001/04/xmlenc#aes256-cbc"),
            encryption_reference: Some("OEBPS/one.xhtml"),
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_font_obfuscation(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            encryption_algorithm: Some("http://www.idpf.org/2008/embedding"),
            encryption_reference: Some("OEBPS/font.otf"),
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_utf16_font_obfuscation(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            encryption_algorithm: Some("http://www.idpf.org/2008/embedding"),
            encryption_reference: Some("OEBPS/font.otf"),
            utf16_markup: true,
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_font_algorithm_for_non_font(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            encryption_algorithm: Some("http://www.idpf.org/2008/embedding"),
            encryption_reference: Some("OEBPS/one.xhtml"),
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_duplicate_rootfile(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            variant: EpubFixtureVariant::DuplicateRootfile,
            encryption_algorithm: Some("http://www.idpf.org/2008/embedding"),
            encryption_reference: Some("OEBPS/font.otf"),
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_foreign_font_item(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            encryption_algorithm: Some("http://www.idpf.org/2008/embedding"),
            encryption_reference: Some("OEBPS/one.xhtml"),
            variant: EpubFixtureVariant::ForeignFontItem,
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_duplicate_manifest_url(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            encryption_algorithm: Some("http://www.idpf.org/2008/embedding"),
            encryption_reference: Some("OEBPS/one.xhtml"),
            variant: EpubFixtureVariant::DuplicateManifestUrl,
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_cross_package_font_relabel(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            encryption_algorithm: Some("http://www.idpf.org/2008/embedding"),
            encryption_reference: Some("OEBPS/one.xhtml"),
            variant: EpubFixtureVariant::CrossPackageFontRelabel,
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_remote_manifest_item(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            encryption_algorithm: Some("http://www.idpf.org/2008/embedding"),
            encryption_reference: Some("OEBPS/font.otf"),
            variant: EpubFixtureVariant::RemoteManifestItem,
            ..EpubFixtureOptions::default()
        },
    );
}

pub(super) fn write_epub_fixture_with_many_remote_manifest_items(path: &Path) {
    write_epub_fixture_inner(
        path,
        EpubFixtureOptions {
            encryption_algorithm: Some("http://www.idpf.org/2008/embedding"),
            encryption_reference: Some("OEBPS/font.otf"),
            variant: EpubFixtureVariant::ManyRemoteManifestItems,
            ..EpubFixtureOptions::default()
        },
    );
}

#[derive(Clone, Copy, Default)]
struct EpubFixtureOptions<'a> {
    variant: EpubFixtureVariant,
    unsafe_name: Option<&'a str>,
    encryption_algorithm: Option<&'a str>,
    encryption_reference: Option<&'a str>,
    utf16_markup: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum EpubFixtureVariant {
    #[default]
    Standard,
    LargeEntry,
    DuplicateRootfile,
    ForeignFontItem,
    DuplicateManifestUrl,
    CrossPackageFontRelabel,
    RemoteManifestItem,
    ManyRemoteManifestItems,
}

fn write_epub_fixture_inner(path: &Path, options: EpubFixtureOptions<'_>) {
    let file = File::create(path).expect("epub file");
    let mut zip = zip::ZipWriter::new(file);
    let stored = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).expect("mimetype entry");
    zip.write_all(b"application/epub+zip").expect("mimetype");

    zip.start_file("META-INF/container.xml", deflated)
        .expect("container entry");
    let additional_rootfile = match options.variant {
        EpubFixtureVariant::DuplicateRootfile => {
            r#"<rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>"#
        }
        EpubFixtureVariant::CrossPackageFontRelabel => {
            r#"<rootfile full-path="ALT/content.opf" media-type="application/oebps-package+xml"/>"#
        }
        _ => "",
    };
    let container = format!(
        r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>{additional_rootfile}</rootfiles>
</container>"#
    );
    zip.write_all(&encoded_markup(&container, options.utf16_markup))
        .expect("container");

    write_package_documents(&mut zip, options);

    zip.start_file("OEBPS/one.xhtml", deflated)
        .expect("chapter one entry");
    zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Chapter One</h1><p>Hello &amp; goodbye.</p></body></html>"#)
        .expect("chapter one");
    zip.start_file("OEBPS/two.xhtml", deflated)
        .expect("chapter two entry");
    zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Chapter Two</h1><p>The end.</p></body></html>"#)
        .expect("chapter two");
    zip.start_file("OEBPS/font.otf", deflated)
        .expect("font entry");
    zip.write_all(b"OTTO-test-font-resource")
        .expect("font bytes");
    if options.variant == EpubFixtureVariant::LargeEntry {
        zip.start_file("OEBPS/unused-large.bin", deflated)
            .expect("large entry");
        let zeros = vec![0_u8; 1_024 * 1_024];
        for _ in 0..33 {
            zip.write_all(&zeros).expect("large entry bytes");
        }
    }
    if let Some(unsafe_name) = options.unsafe_name {
        zip.start_file(unsafe_name, deflated)
            .expect("unsafe path entry");
        zip.write_all(b"unsafe").expect("unsafe path entry bytes");
    }
    if let Some(algorithm) = options.encryption_algorithm {
        let reference = options.encryption_reference.expect("encryption reference");
        zip.start_file("META-INF/encryption.xml", deflated)
            .expect("encryption manifest entry");
        let manifest = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container"
            xmlns:enc="http://www.w3.org/2001/04/xmlenc#">
  <enc:EncryptedData>
    <enc:EncryptionMethod Algorithm="{algorithm}"/>
    <enc:CipherData><enc:CipherReference URI="{reference}"/></enc:CipherData>
  </enc:EncryptedData>
</encryption>"#
        );
        zip.write_all(&encoded_markup(&manifest, options.utf16_markup))
            .expect("encryption manifest bytes");
    }
    zip.finish().expect("finish epub");
}

fn write_package_documents(zip: &mut zip::ZipWriter<File>, options: EpubFixtureOptions<'_>) {
    let deflated = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("OEBPS/content.opf", deflated)
        .expect("package entry");
    let duplicate_manifest_url = if options.variant == EpubFixtureVariant::DuplicateManifestUrl {
        r#"<item id="duplicate" href="one.xhtml" media-type="font/otf"/>"#
    } else {
        ""
    };
    let foreign_font_item = if options.variant == EpubFixtureVariant::ForeignFontItem {
        r#"<foreign:item xmlns:foreign="urn:foreign" href="one.xhtml" media-type="font/otf"/>"#
    } else {
        ""
    };
    let remote_manifest_items = remote_manifest_items(options.variant);
    let package = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Tiny Book</dc:title><dc:identifier id="id">tiny</dc:identifier><dc:language>en</dc:language></metadata>
  <manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="font" href="font.otf" media-type="font/otf"/>{duplicate_manifest_url}{remote_manifest_items}</manifest>
  {foreign_font_item}
  <spine><itemref idref="one"/><itemref idref="two"/></spine>
</package>"#
    );
    zip.write_all(&encoded_markup(&package, options.utf16_markup))
        .expect("package");

    if options.variant == EpubFixtureVariant::CrossPackageFontRelabel {
        zip.start_file("ALT/content.opf", deflated)
            .expect("alternate package entry");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Alternate</dc:title><dc:identifier id="id">alternate</dc:identifier><dc:language>en</dc:language></metadata>
  <manifest><item id="relabel" href="../OEBPS/one.xhtml" media-type="font/otf"/></manifest>
  <spine/>
</package>"#,
        )
        .expect("alternate package");
    }
}

fn remote_manifest_items(variant: EpubFixtureVariant) -> String {
    if variant == EpubFixtureVariant::RemoteManifestItem {
        return r#"<item id="remote" href="https://example.invalid/style.css" media-type="text/css"/>"#
            .to_owned();
    }
    if variant != EpubFixtureVariant::ManyRemoteManifestItems {
        return String::new();
    }

    let mut items = String::new();
    for index in 0..10_001 {
        write!(
            &mut items,
            r#"<item id="remote-{index}" href="https://example.invalid/{index}.css" media-type="text/css"/>"#
        )
        .expect("remote manifest item");
    }
    items
}

fn encoded_markup(source: &str, utf16: bool) -> Vec<u8> {
    if !utf16 {
        return source.as_bytes().to_vec();
    }
    let source = source
        .replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\"")
        .replace(
            "<?xml version=\"1.0\"?>",
            "<?xml version=\"1.0\" encoding=\"UTF-16\"?>",
        );
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(source.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

pub(super) fn patch_eocd_entry_count(path: &Path, count: u16) {
    let mut bytes = std::fs::read(path).expect("read EPUB fixture");
    let eocd = bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("end of central directory");
    bytes[eocd + 8..eocd + 10].copy_from_slice(&count.to_le_bytes());
    bytes[eocd + 10..eocd + 12].copy_from_slice(&count.to_le_bytes());
    std::fs::write(path, bytes).expect("patch EPUB fixture");
}

pub(super) fn patch_central_uncompressed_size(path: &Path, name: &str, size: u32) {
    let mut bytes = std::fs::read(path).expect("read EPUB fixture");
    let name_offset = bytes
        .windows(name.len())
        .rposition(|window| window == name.as_bytes())
        .expect("central directory filename");
    let header = name_offset
        .checked_sub(46)
        .expect("central directory header");
    assert_eq!(&bytes[header..header + 4], b"PK\x01\x02");
    bytes[header + 24..header + 28].copy_from_slice(&size.to_le_bytes());
    std::fs::write(path, bytes).expect("patch EPUB fixture");
}
