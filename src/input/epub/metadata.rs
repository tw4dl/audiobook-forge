use rbook::Epub;

use crate::book::{BookAsset, BookMetadata};

pub(super) fn read_metadata(epub: &Epub, fallback_title: &str) -> (BookMetadata, Vec<String>) {
    let source = epub.metadata();
    let title = source
        .title()
        .and_then(|title| nonempty(title.value()))
        .unwrap_or_else(|| fallback_title.to_owned());
    let authors = source
        .creators()
        .filter(is_author)
        .filter_map(|creator| nonempty(creator.value()))
        .collect();
    let language = source
        .language()
        .and_then(|language| nonempty(language.value()));
    let (cover, warnings) = read_cover(epub);
    (
        BookMetadata {
            title: Some(title),
            authors,
            language,
            cover,
        },
        warnings,
    )
}

fn is_author(creator: &rbook::epub::metadata::EpubContributor<'_>) -> bool {
    let mut roles = creator.roles().peekable();
    roles.peek().is_none() || roles.any(|role| role.code().eq_ignore_ascii_case("aut"))
}

fn read_cover(epub: &Epub) -> (Option<BookAsset>, Vec<String>) {
    let Some(entry) = epub.manifest().cover_image() else {
        return (None, Vec::new());
    };
    let source_id = entry.href().path().decode().into_owned();
    match entry.read_bytes() {
        Ok(bytes) => (
            Some(BookAsset {
                source_id,
                media_type: entry.media_type().to_owned(),
                bytes,
            }),
            Vec::new(),
        ),
        Err(error) => (
            None,
            vec![format!(
                "EPUB cover {source_id} could not be read and was skipped: {error}"
            )],
        ),
    }
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
