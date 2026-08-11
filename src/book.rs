//! Format-neutral book structure shared by import, inspection, and synthesis.

use std::fmt;
use std::path::PathBuf;

/// A book after format-specific import and before synthesis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalBook {
    pub metadata: BookMetadata,
    pub root: Section,
    pub source: SourceDocument,
    /// Normalized text consumed by the current synthesis pipeline.
    pub text: String,
    /// Authored page markers. Reflowable sources leave this empty when no page list exists.
    pub pages: Vec<PageMarker>,
    pub warnings: Vec<String>,
}

impl CanonicalBook {
    #[must_use]
    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookMetadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub cover: Option<BookAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookAsset {
    pub source_id: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDocument {
    pub path: PathBuf,
    pub format: SourceFormat,
    pub format_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Azw3,
    Epub,
    Html,
    Markdown,
    Mobi,
    Pdf,
    Text,
}

impl fmt::Display for SourceFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Azw3 => formatter.write_str("AZW3/KF8"),
            Self::Epub => formatter.write_str("EPUB"),
            Self::Html => formatter.write_str("HTML"),
            Self::Markdown => formatter.write_str("Markdown"),
            Self::Mobi => formatter.write_str("MOBI"),
            Self::Pdf => formatter.write_str("PDF"),
            Self::Text => formatter.write_str("TXT"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: String,
    pub kind: SectionKind,
    pub title: Option<String>,
    pub level: u8,
    pub provenance: Provenance,
    pub source_range: Option<SourceRange>,
    pub blocks: Vec<Block>,
    pub children: Vec<Self>,
}

impl Section {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        kind: SectionKind,
        title: Option<String>,
        level: u8,
        provenance: Provenance,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            title,
            level,
            provenance,
            source_range: None,
            blocks: Vec::new(),
            children: Vec::new(),
        }
    }

    #[must_use]
    pub fn word_count(&self) -> usize {
        let own_words = self
            .blocks
            .iter()
            .map(|block| block.text().split_whitespace().count())
            .sum::<usize>();
        own_words + self.children.iter().map(Self::word_count).sum::<usize>()
    }
}

/// How a semantic section entered the canonical structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Explicitly present in source markup or navigation.
    Authored,
    /// Deterministically created from source structure.
    Derived,
    /// Detected by a deterministic heuristic.
    Inferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Book,
    FrontMatter,
    BodyMatter,
    Part,
    Chapter,
    Section,
    Appendix,
    Notes,
    Bibliography,
    Index,
    BackMatter,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(TextBlock),
    Quote(TextBlock),
    List(ListBlock),
    Figure(FigureBlock),
    Footnote(TextBlock),
    Aside(TextBlock),
    Navigation(TextBlock),
    Code(TextBlock),
}

impl Block {
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Paragraph(block)
            | Self::Quote(block)
            | Self::Footnote(block)
            | Self::Aside(block)
            | Self::Navigation(block)
            | Self::Code(block) => &block.text,
            Self::List(block) => &block.text,
            Self::Figure(block) => block
                .caption
                .as_deref()
                .or(block.alt_text.as_deref())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBlock {
    pub text: String,
    pub source_range: Option<SourceRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListBlock {
    pub ordered: bool,
    pub items: Vec<String>,
    /// Current narration text. Kept separate from item structure for normalization.
    pub text: String,
    pub source_range: Option<SourceRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FigureBlock {
    pub alt_text: Option<String>,
    pub caption: Option<String>,
    pub source_range: Option<SourceRange>,
}

/// A half-open range in one imported source (`start..end`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SourceRange {
    pub source_id: String,
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMarker {
    pub label: String,
    pub position: SourcePosition,
}

/// A source position that keeps format-specific coordinates behind one API.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourcePosition {
    /// A UTF-8 byte offset in TXT, Markdown, or standalone HTML.
    Text { byte_offset: usize },
    /// A location within one EPUB XHTML resource.
    Epub {
        resource: String,
        fragment: Option<String>,
        character_offset: Option<usize>,
    },
    /// A location within a text-based PDF page.
    Pdf {
        page_number: u32,
        character_offset: Option<usize>,
    },
    /// A UTF-8 byte offset in decoded MOBI or KF8 markup.
    Kindle { byte_offset: usize },
}
