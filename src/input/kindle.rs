mod container;
mod navigation;
mod palmdoc;

use anyhow::Result;

use crate::book::{Block, CanonicalBook, Section, SourceDocument, SourcePosition, SourceRange};

use self::container::KindleContainer;
use super::html::import_html_source;
use super::{BookImporter, ImportSource, normalize_text, section_text, source_id};

pub(super) struct KindleImporter;

impl BookImporter for KindleImporter {
    fn import(&self, input: ImportSource) -> Result<CanonicalBook> {
        let (path, bytes) = input.into_parts();
        let container = KindleContainer::parse(&bytes)?;
        let metadata = container.metadata(&path);
        let format = container.source_format();
        let format_version = container.format_version();
        let file_version = container.file_version();
        let markup = container.decode_markup()?;
        let mut warnings = container.warnings();
        let prepared = navigation::prepare(&markup, file_version)?;
        warnings.extend(prepared.warnings);

        let mut book = import_html_source(path.clone(), &prepared.markup)?;
        warnings.append(&mut book.warnings);
        book.metadata = metadata;
        book.root.title.clone_from(&book.metadata.title);
        remove_redundant_visible_headings(&mut book.root);
        if book.root.children.is_empty() {
            warnings.push(
                "Kindle content has no detected semantic sections; treating it as one body"
                    .to_owned(),
            );
        }
        let source_id = source_id(&path);
        assign_source_ranges(&mut book.root, &prepared.markup, &source_id, &mut 0);
        book.text = normalize_text(&section_text(&book.root));
        book.source = SourceDocument {
            path,
            format,
            format_version: Some(format_version),
        };
        book.warnings = warnings;
        Ok(book)
    }
}

fn remove_redundant_visible_headings(section: &mut Section) {
    if let Some(title) = section.title.as_deref()
        && section
            .blocks
            .first()
            .is_some_and(|block| block.text().trim() == title.trim())
    {
        section.blocks.remove(0);
    }
    for child in &mut section.children {
        remove_redundant_visible_headings(child);
    }
}

fn assign_source_ranges(section: &mut Section, markup: &str, source_id: &str, cursor: &mut usize) {
    if section.level > 0
        && let Some(title) = section.title.as_deref()
        && let Some(offset) = find_text(markup, title, *cursor)
    {
        section.source_range = Some(kindle_range(source_id, offset, offset + title.len()));
        *cursor = offset.saturating_add(title.len());
    }
    for block in &mut section.blocks {
        let text = block.text().to_owned();
        if text.is_empty() {
            continue;
        }
        if let Some(offset) = find_text(markup, &text, *cursor) {
            set_block_range(
                block,
                Some(kindle_range(source_id, offset, offset + text.len())),
            );
            *cursor = offset.saturating_add(text.len());
        }
    }
    for child in &mut section.children {
        assign_source_ranges(child, markup, source_id, cursor);
    }
}

fn find_text(markup: &str, text: &str, cursor: usize) -> Option<usize> {
    markup
        .get(cursor..)
        .and_then(|tail| tail.find(text))
        .map(|offset| cursor + offset)
        .or_else(|| markup.find(text))
}

fn kindle_range(source_id: &str, start: usize, end: usize) -> SourceRange {
    SourceRange {
        source_id: source_id.to_owned(),
        start: SourcePosition::Kindle { byte_offset: start },
        end: SourcePosition::Kindle { byte_offset: end },
    }
}

fn set_block_range(block: &mut Block, range: Option<SourceRange>) {
    match block {
        Block::Paragraph(block)
        | Block::Quote(block)
        | Block::Footnote(block)
        | Block::Aside(block)
        | Block::Navigation(block)
        | Block::Code(block) => block.source_range = range,
        Block::List(block) => block.source_range = range,
        Block::Figure(block) => block.source_range = range,
    }
}
