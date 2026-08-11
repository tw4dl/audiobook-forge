//! Bounded, format-neutral book import.

mod epub;
mod html;
mod markdown;
mod text;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::book::{CanonicalBook, Section, SectionKind, SourcePosition, SourceRange};

const MIB: u64 = 1_024 * 1_024;
const MAX_RAW_INPUT_BYTES: u64 = 32 * MIB;
const MAX_EPUB_INPUT_BYTES: u64 = 512 * MIB;

/// Common contract for format-specific importers.
pub(super) trait BookImporter {
    /// Import one validated source into the canonical model.
    ///
    /// # Errors
    ///
    /// Returns an error when the source cannot be read or parsed.
    fn import(&self, source: ImportSource) -> Result<CanonicalBook>;
}

pub(super) struct ImportSource {
    path: PathBuf,
    bytes: Vec<u8>,
}

impl ImportSource {
    pub(super) fn into_parts(self) -> (PathBuf, Vec<u8>) {
        (self.path, self.bytes)
    }
}

/// Read one supported book into the canonical model.
///
/// Raw text formats are limited to 32 MiB. EPUB archives are limited to 512 MiB.
///
/// # Errors
///
/// Returns an error when the file is missing, too large, unsupported, malformed, or empty.
pub fn read_book(path: &Path) -> Result<CanonicalBook> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (max_bytes, max_mib) = match extension.as_str() {
        "epub" => (MAX_EPUB_INPUT_BYTES, 512),
        "txt" | "md" | "markdown" | "htm" | "html" | "xhtml" => (MAX_RAW_INPUT_BYTES, 32),
        _ => bail!("supported input types: .epub, .html, .md, and .txt"),
    };
    let source = read_source(path, max_bytes, max_mib)?;

    let mut book = match extension.as_str() {
        "txt" => text::TextImporter.import(source)?,
        "md" | "markdown" => markdown::MarkdownImporter.import(source)?,
        "htm" | "html" | "xhtml" => html::HtmlImporter.import(source)?,
        "epub" => epub::EpubImporter.import(source)?,
        _ => unreachable!("extension was validated before reading"),
    };

    book.text = normalize_text(&book.text);
    if book.text.is_empty() {
        bail!("{} contains no readable text", path.display());
    }
    Ok(book)
}

fn read_source(path: &Path, max_bytes: u64, max_mib: u64) -> Result<ImportSource> {
    let file = File::open(path)
        .with_context(|| format!("input does not exist or is not a file: {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect input size for {}", path.display()))?;
    if !metadata.is_file() {
        bail!("input does not exist or is not a file: {}", path.display());
    }
    if metadata.len() > max_bytes {
        bail!("input exceeds {max_mib} MiB limit: {}", path.display());
    }
    let bytes = match read_at_most(file, max_bytes) {
        Ok(bytes) => bytes,
        Err(BoundedReadError::TooLarge { .. }) => {
            bail!("input exceeds {max_mib} MiB limit: {}", path.display());
        }
        Err(BoundedReadError::Io(error)) => {
            return Err(error)
                .with_context(|| format!("failed to read input from {}", path.display()));
        }
    };
    Ok(ImportSource {
        path: path.to_path_buf(),
        bytes,
    })
}

#[derive(Debug, thiserror::Error)]
enum BoundedReadError {
    #[error("failed to read input: {0}")]
    Io(#[from] std::io::Error),
    #[error("input grew beyond its {limit}-byte limit")]
    TooLarge { limit: u64 },
}

fn read_at_most(
    reader: impl Read,
    max_bytes: u64,
) -> std::result::Result<Vec<u8>, BoundedReadError> {
    let mut bytes = Vec::new();
    reader.take(max_bytes + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(BoundedReadError::TooLarge { limit: max_bytes });
    }
    Ok(bytes)
}

pub(super) fn title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Untitled")
        .to_owned()
}

pub(super) fn source_id(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("source")
        .to_owned()
}

pub(super) fn heading_kind(heading: &str) -> SectionKind {
    let first = heading.split_whitespace().next().unwrap_or_default();
    if first.eq_ignore_ascii_case("part") || first.eq_ignore_ascii_case("book") {
        SectionKind::Part
    } else if first.eq_ignore_ascii_case("chapter") {
        SectionKind::Chapter
    } else {
        SectionKind::Section
    }
}

pub(super) fn text_source_range(source_id: &str, start: usize, end: usize) -> SourceRange {
    SourceRange {
        source_id: source_id.to_owned(),
        start: SourcePosition::Text { byte_offset: start },
        end: SourcePosition::Text { byte_offset: end },
    }
}

pub(super) fn source_lines(text: &str) -> Vec<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut line_start = 0_usize;
    let mut index = 0_usize;
    while index < bytes.len() {
        if matches!(bytes[index], b'\r' | b'\n') {
            lines.push((line_start, index, &text[line_start..index]));
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            index += 1;
            line_start = index;
        } else {
            index += 1;
        }
    }
    if line_start < text.len() {
        lines.push((line_start, text.len(), &text[line_start..]));
    }
    lines
}

pub(super) fn section_text(section: &Section) -> String {
    let mut parts = Vec::new();
    if matches!(
        section.kind,
        SectionKind::Part | SectionKind::Chapter | SectionKind::Section
    ) && let Some(title) = &section.title
    {
        parts.push(title.clone());
    }
    parts.extend(section.blocks.iter().map(|block| block.text().to_owned()));
    parts.extend(section.children.iter().map(section_text));
    parts.join("\n\n")
}

pub(super) fn normalize_text(text: &str) -> String {
    let normalized_lines = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut paragraphs = Vec::new();
    let mut current = Vec::new();
    for line in normalized_lines.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }
    paragraphs.join("\n\n")
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::read_at_most;

    #[test]
    fn bounded_reader_rejects_bytes_beyond_the_declared_limit() {
        let error = read_at_most(Cursor::new(vec![0_u8; 6]), 5)
            .expect_err("reader must enforce the limit while reading");

        assert_eq!(error.to_string(), "input grew beyond its 5-byte limit");
    }
}
