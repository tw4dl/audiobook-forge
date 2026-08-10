//! EPUB and plain-text extraction.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rbook::Epub;

/// Text extracted from an input book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Book {
    pub text: String,
}

/// Read a UTF-8 TXT file or EPUB spine.
///
/// # Errors
///
/// Returns an error when the file is missing, unsupported, malformed, or empty.
pub fn read_book(path: &Path) -> Result<Book> {
    if !path.is_file() {
        bail!("input does not exist or is not a file: {}", path.display());
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut book = match extension.as_str() {
        "txt" => read_text(path)?,
        "epub" => read_epub(path)?,
        _ => bail!("supported input types: .epub and .txt"),
    };

    book.text = normalize_text(&book.text);
    if book.text.is_empty() {
        bail!("{} contains no readable text", path.display());
    }
    Ok(book)
}

fn read_text(path: &Path) -> Result<Book> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read UTF-8 text from {}", path.display()))?;
    Ok(Book { text })
}

fn read_epub(path: &Path) -> Result<Book> {
    let document = Epub::options()
        .skip_metadata(true)
        .skip_toc(true)
        .open(path)
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

    Ok(Book {
        text: chapters.join("\n\n"),
    })
}

fn normalize_text(text: &str) -> String {
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
