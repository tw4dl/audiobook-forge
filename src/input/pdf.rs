//! Bounded import for text-based PDF documents.

mod labels;
mod outline;
mod structure;

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use lopdf::{Document, Error as PdfError, LoadOptions, Object, ObjectId};

use crate::book::{
    BookMetadata, CanonicalBook, PageMarker, Provenance, Section, SectionKind, SourceDocument,
    SourceFormat, SourcePosition,
};

use super::{BookImporter, ImportSource, section_text, source_id, title_from_path};

const MAX_PDF_OBJECTS: usize = 200_000;
const MAX_PDF_PAGES: usize = 10_000;
const MAX_PDF_STREAM_BYTES: usize = 16 * 1_024 * 1_024;
const MAX_PDF_PAGE_TEXT_BYTES: usize = 16 * 1_024 * 1_024;
const MAX_PDF_TOTAL_TEXT_BYTES: usize = 128 * 1_024 * 1_024;

pub(super) struct PdfImporter;

#[derive(Debug)]
pub(super) struct PdfPage {
    pub number: u32,
    pub text: String,
}

impl BookImporter for PdfImporter {
    fn import(&self, source: ImportSource) -> Result<CanonicalBook> {
        let (path, bytes) = source.into_parts();
        let document = load_document(&path, &bytes)?;
        validate_document_shape(&path, &document)?;
        let page_objects = document.get_pages();
        let pages = extract_pages(&path, &document, &page_objects)?;
        if pages.iter().all(|page| page.text.trim().is_empty()) {
            bail!(
                "PDF contains no extractable text; OCR is not supported: {}",
                path.display()
            );
        }

        let metadata = read_metadata(&path, &document);
        let title = metadata
            .title
            .clone()
            .unwrap_or_else(|| title_from_path(&path));
        let source_id = source_id(&path);
        let (page_labels, mut warnings) = match labels::read_page_labels(&document, pages.len()) {
            Ok(Some(labels)) => (labels, Vec::new()),
            Ok(None) => (
                physical_page_labels(&pages),
                vec!["Page labels unavailable; using physical PDF page numbers".to_owned()],
            ),
            Err(error) => (
                physical_page_labels(&pages),
                vec![format!(
                    "PDF page labels are malformed; using physical page numbers: {error}"
                )],
            ),
        };
        let root = match outline::read_outline(&document, pages.len())? {
            outline::OutlineRead::Found {
                items,
                warnings: outline_warnings,
            } => {
                warnings.extend(outline_warnings);
                structure::from_outline(&title, &source_id, &pages, &items)
            }
            outline::OutlineRead::Missing { warning } => {
                if let Some(warning) = warning {
                    warnings.push(warning);
                }
                let inferred = structure::infer(&title, &source_id, &pages);
                if inferred.heading_count == 0 {
                    warnings.push(
                        "PDF has no outline and no high-confidence chapter headings; treating the document as one body section"
                            .to_owned(),
                    );
                } else {
                    warnings.push(format!(
                        "PDF has no outline; inferred {} chapter headings",
                        inferred.heading_count
                    ));
                }
                warnings.push(
                    "Tagged PDF logical structure is not used in V1; applied deterministic text inference"
                        .to_owned(),
                );
                inferred.root
            }
        };
        let text = section_text(&root);
        let page_markers = pages
            .iter()
            .zip(page_labels)
            .map(|(page, label)| PageMarker {
                label,
                position: SourcePosition::Pdf {
                    page_number: page.number,
                    character_offset: Some(0),
                },
            })
            .collect();

        Ok(CanonicalBook {
            metadata,
            root,
            source: SourceDocument {
                path,
                format: SourceFormat::Pdf,
                format_version: Some(document.version.clone()),
            },
            text,
            pages: page_markers,
            warnings,
        })
    }
}

fn physical_page_labels(pages: &[PdfPage]) -> Vec<String> {
    pages.iter().map(|page| page.number.to_string()).collect()
}

fn load_document(path: &Path, bytes: &[u8]) -> Result<Document> {
    let options = LoadOptions::with_max_decompressed_size(MAX_PDF_STREAM_BYTES);
    let document = match Document::load_mem_with_options(bytes, options) {
        Ok(document) => document,
        Err(
            PdfError::InvalidPassword
            | PdfError::Decryption(_)
            | PdfError::UnsupportedSecurityHandler(_),
        ) => {
            bail!("encrypted PDF is not supported; provide a DRM-free PDF")
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to parse PDF from {}", path.display()));
        }
    };
    if document.is_encrypted() || document.was_encrypted() {
        bail!("encrypted PDF is not supported; provide a DRM-free PDF");
    }
    Ok(document)
}

fn validate_document_shape(path: &Path, document: &Document) -> Result<()> {
    if document.objects.len() > MAX_PDF_OBJECTS {
        bail!(
            "PDF contains more than {MAX_PDF_OBJECTS} objects: {}",
            path.display()
        );
    }
    let pages = document.get_pages();
    if pages.is_empty() {
        bail!("PDF contains no pages: {}", path.display());
    }
    if pages.len() > MAX_PDF_PAGES {
        bail!(
            "PDF contains more than {MAX_PDF_PAGES} pages: {}",
            path.display()
        );
    }
    if let Some(declared) = declared_page_count(document)
        && declared != pages.len()
    {
        bail!(
            "PDF page tree is malformed: declared {declared} pages but found {}",
            pages.len()
        );
    }
    Ok(())
}

fn declared_page_count(document: &Document) -> Option<usize> {
    let catalog = document.catalog().ok()?;
    let pages = catalog.get(b"Pages").ok()?;
    let pages = dereference(document, pages).ok()?.as_dict().ok()?;
    let count = pages.get(b"Count").ok()?.as_i64().ok()?;
    usize::try_from(count).ok()
}

fn extract_pages(
    path: &Path,
    document: &Document,
    page_objects: &BTreeMap<u32, ObjectId>,
) -> Result<Vec<PdfPage>> {
    let mut pages = Vec::with_capacity(page_objects.len());
    let mut total_bytes = 0_usize;
    for number in page_objects.keys().copied() {
        let extracted = document
            .extract_text_with_limit(&[number], MAX_PDF_PAGE_TEXT_BYTES)
            .with_context(|| {
                format!(
                    "failed to extract bounded text from PDF page {number} in {}",
                    path.display()
                )
            })?;
        let text = normalize_page_text(&extracted);
        total_bytes = total_bytes
            .checked_add(text.len())
            .context("PDF extracted text size overflow")?;
        if total_bytes > MAX_PDF_TOTAL_TEXT_BYTES {
            bail!(
                "PDF extracted text exceeds {} MiB limit: {}",
                MAX_PDF_TOTAL_TEXT_BYTES / 1_024 / 1_024,
                path.display()
            );
        }
        pages.push(PdfPage { number, text });
    }
    Ok(pages)
}

fn normalize_page_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .to_owned()
}

fn read_metadata(path: &Path, document: &Document) -> BookMetadata {
    let title = info_text(document, b"Title").filter(|value| !value.trim().is_empty());
    let author = info_text(document, b"Author").filter(|value| !value.trim().is_empty());
    BookMetadata {
        title: title.or_else(|| Some(title_from_path(path))),
        authors: author.into_iter().collect(),
        language: None,
        cover: None,
    }
}

fn info_text(document: &Document, key: &[u8]) -> Option<String> {
    let info = document.trailer.get(b"Info").ok()?;
    let dictionary = dereference(document, info).ok()?.as_dict().ok()?;
    let value = dictionary.get(key).ok()?;
    let value = dereference(document, value).ok()?;
    lopdf::decode_text_string(value).ok()
}

fn dereference<'a>(document: &'a Document, object: &'a Object) -> lopdf::Result<&'a Object> {
    match object {
        Object::Reference(id) => document.get_object(*id),
        _ => Ok(object),
    }
}

pub(super) fn source_position(page_number: u32, character_offset: usize) -> SourcePosition {
    SourcePosition::Pdf {
        page_number,
        character_offset: Some(character_offset),
    }
}

pub(super) fn empty_root(title: &str) -> Section {
    Section::new(
        "book",
        SectionKind::Book,
        Some(title.to_owned()),
        0,
        Provenance::Derived,
    )
}
