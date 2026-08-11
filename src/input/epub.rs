use std::io::Cursor;

use anyhow::{Context, Result};
use rbook::Epub;

use crate::book::{
    Block, BookMetadata, CanonicalBook, Provenance, Section, SectionKind, SourceDocument,
    SourceFormat, TextBlock,
};

use super::{BookImporter, ImportSource, normalize_text, title_from_path};

mod protection;
mod security;

use security::validate_archive;

pub(super) struct EpubImporter;

impl BookImporter for EpubImporter {
    fn import(&self, source: ImportSource) -> Result<CanonicalBook> {
        let (path, bytes) = source.into_parts();
        validate_archive(&bytes, &path)?;
        let document = Epub::options()
            .skip_metadata(true)
            .skip_toc(true)
            .read(Cursor::new(bytes))
            .with_context(|| format!("failed to open EPUB {}", path.display()))?;
        let mut chapters = Vec::new();

        for item in document.reader() {
            let content = item.context("failed to read EPUB spine item")?;
            let text = html2text::from_read(content.content().as_bytes(), 10_000)
                .context("failed to render EPUB chapter text")?;
            let text = normalize_text(&text);
            if !text.is_empty() {
                chapters.push(text);
            }
        }

        let text = chapters.join("\n\n");
        let title = title_from_path(&path);
        let mut root = Section::new(
            "book",
            SectionKind::Book,
            Some(title.clone()),
            0,
            Provenance::Derived,
        );
        if !text.is_empty() {
            root.blocks.push(Block::Paragraph(TextBlock {
                text: text.clone(),
                source_range: None,
            }));
        }

        Ok(CanonicalBook {
            metadata: BookMetadata {
                title: Some(title),
                ..BookMetadata::default()
            },
            root,
            source: SourceDocument {
                path,
                format: SourceFormat::Epub,
            },
            text,
            warnings: Vec::new(),
        })
    }
}
