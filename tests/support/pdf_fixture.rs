use std::path::Path;

use lopdf::content::{Content, Operation};
use lopdf::{
    Bookmark, Document, EncryptionState, EncryptionVersion, Object, Permissions, Stream, dictionary,
};

pub(super) fn write_pdf_with_bookmarks(path: &Path) {
    let pages = [
        &["CHAPTER ONE", "Opening paragraph."][..],
        &["A Closer Look", "Nested section text."][..],
        &["CHAPTER TWO", "Closing paragraph."][..],
    ];
    let mut document = make_document(&pages, "Public Domain Sample", "Example Author");
    let page_ids = document.get_pages().into_values().collect::<Vec<_>>();
    let chapter = document.add_bookmark(
        Bookmark::new("Chapter One".to_owned(), [0.0; 3], 0, page_ids[0]),
        None,
    );
    document.add_bookmark(
        Bookmark::new("A Closer Look".to_owned(), [0.0; 3], 0, page_ids[1]),
        Some(chapter),
    );
    document.add_bookmark(
        Bookmark::new("Chapter Two".to_owned(), [0.0; 3], 0, page_ids[2]),
        None,
    );
    attach_page_labels(&mut document);
    attach_outline(&mut document);
    document.save(path).expect("save PDF fixture");
}

pub(super) fn write_pdf_without_bookmarks(path: &Path) {
    let pages = [
        &["CHAPTER ONE", "Opening paragraph."][..],
        &["CHAPTER TWO", "Closing paragraph."][..],
    ];
    let mut document = make_document(&pages, "Inferred Sample", "Example Author");
    document.save(path).expect("save PDF fixture");
}

pub(super) fn write_pdf_without_headings(path: &Path) {
    let pages = [
        &["This is ordinary prose on the first page."][..],
        &["This is ordinary prose on the second page."][..],
    ];
    let mut document = make_document(&pages, "Prose Sample", "Example Author");
    document.save(path).expect("save PDF fixture");
}

pub(super) fn write_blank_pdf(path: &Path) {
    let pages = [&[][..], &[][..]];
    let mut document = make_document(&pages, "Scanned Sample", "Example Author");
    document.save(path).expect("save blank PDF fixture");
}

pub(super) fn write_encrypted_pdf(path: &Path) {
    let pages = [&["Protected text."][..]];
    let mut document = make_document(&pages, "Protected Sample", "Example Author");
    let encryption = EncryptionVersion::V2 {
        document: &document,
        owner_password: "owner-password",
        user_password: "reader-password",
        key_length: 128,
        permissions: Permissions::PRINTABLE,
    };
    let state = EncryptionState::try_from(encryption).expect("encryption state");
    document.encrypt(&state).expect("encrypt PDF fixture");
    document.save(path).expect("save encrypted PDF fixture");
}

pub(super) fn write_pdf_with_outline_cycle(path: &Path) {
    let pages = [&["Readable text."][..]];
    let mut document = make_document(&pages, "Cycle Sample", "Example Author");
    let page_id = document.get_pages()[&1];
    let node_id = document.new_object_id();
    document.objects.insert(
        node_id,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("Cycle"),
            "Dest" => vec![page_id.into(), Object::Name(b"Fit".to_vec())],
            "Next" => node_id,
        }),
    );
    let outline_id = document.add_object(dictionary! {
        "First" => node_id,
        "Last" => node_id,
        "Count" => 1,
    });
    set_catalog_entry(&mut document, "Outlines", outline_id.into());
    document.save(path).expect("save cyclic PDF fixture");
}

pub(super) fn write_pdf_with_wrong_page_count(path: &Path) {
    let pages = [&["Readable text."][..]];
    let mut document = make_document(&pages, "Count Sample", "Example Author");
    let catalog = document.catalog().expect("catalog");
    let pages_id = catalog
        .get(b"Pages")
        .and_then(Object::as_reference)
        .expect("pages reference");
    document
        .get_dictionary_mut(pages_id)
        .expect("pages dictionary")
        .set("Count", 2);
    document.save(path).expect("save malformed PDF fixture");
}

pub(super) fn write_pdf_with_oversized_page_stream(path: &Path) {
    let text = "A".repeat(17 * 1_024 * 1_024);
    let pages = [&[text.as_str()][..]];
    let mut document = make_document(&pages, "Bomb Sample", "Example Author");
    document.compress();
    document.save(path).expect("save compressed PDF fixture");
}

fn make_document(pages: &[&[&str]], title: &str, author: &str) -> Document {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });

    let page_ids = pages
        .iter()
        .map(|lines| {
            let content = Content {
                operations: text_operations(lines),
            };
            let bytes = content.encode().expect("encode PDF page content");
            let content_id = document.add_object(Stream::new(dictionary! {}, bytes));
            document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
            })
        })
        .collect::<Vec<_>>();

    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => i64::try_from(page_ids.len()).expect("page count"),
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    let info_id = document.add_object(dictionary! {
        "Title" => Object::string_literal(title),
        "Author" => Object::string_literal(author),
    });
    document.trailer.set("Root", catalog_id);
    document.trailer.set("Info", info_id);
    document.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal("fixture-id-one"),
            Object::string_literal("fixture-id-two"),
        ]),
    );
    document
}

fn text_operations(lines: &[&str]) -> Vec<Operation> {
    let mut operations = vec![
        Operation::new("BT", Vec::new()),
        Operation::new("Tf", vec!["F1".into(), 12.into()]),
        Operation::new("TL", vec![18.into()]),
        Operation::new("Td", vec![72.into(), 720.into()]),
    ];
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            operations.push(Operation::new("T*", Vec::new()));
        }
        operations.push(Operation::new("Tj", vec![Object::string_literal(*line)]));
    }
    operations.push(Operation::new("ET", Vec::new()));
    operations
}

fn attach_outline(document: &mut Document) {
    let outline_id = document.build_outline().expect("outline root");
    let catalog_id = document
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("catalog reference");
    document
        .get_dictionary_mut(catalog_id)
        .expect("catalog dictionary")
        .set("Outlines", outline_id);
}

fn attach_page_labels(document: &mut Document) {
    let labels_id = document.add_object(dictionary! {
        "Nums" => vec![
            0.into(),
            Object::Dictionary(dictionary! { "S" => "r" }),
            1.into(),
            Object::Dictionary(dictionary! {
                "P" => Object::string_literal("A-"),
                "S" => "D",
                "St" => 3,
            }),
        ],
    });
    set_catalog_entry(document, "PageLabels", labels_id.into());
}

fn set_catalog_entry(document: &mut Document, key: &str, value: Object) {
    let catalog_id = document
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("catalog reference");
    document
        .get_dictionary_mut(catalog_id)
        .expect("catalog dictionary")
        .set(key, value);
}
