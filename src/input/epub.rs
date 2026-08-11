use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use rbook::Epub;

use crate::book::{CanonicalBook, SourceDocument, SourceFormat};

use super::{BookImporter, ImportSource, section_text, title_from_path};

mod content;
mod metadata;
mod navigation;
mod protection;
mod security;

use content::read_spine;
use metadata::read_metadata;
use navigation::build_navigation;
use security::validate_archive;

pub(super) struct EpubImporter;

impl BookImporter for EpubImporter {
    fn import(&self, source: ImportSource) -> Result<CanonicalBook> {
        let (path, bytes) = source.into_parts();
        validate_archive(&bytes, &path)?;
        let (document, mut warnings) = open_document(bytes, &path)?;
        let fallback_title = title_from_path(&path);
        let (metadata, metadata_warnings) = read_metadata(&document, &fallback_title);
        warnings.extend(metadata_warnings);
        let (spine, spine_warnings) = read_spine(&document)?;
        warnings.extend(spine_warnings);
        let title = metadata.title.as_deref().unwrap_or(&fallback_title);
        let (root, pages, navigation_warnings) = build_navigation(&document, title, spine);
        warnings.extend(navigation_warnings);
        let text = section_text(&root);
        let format_version = nonempty(document.metadata().version_str());

        Ok(CanonicalBook {
            metadata,
            root,
            source: SourceDocument {
                path,
                format: SourceFormat::Epub,
                format_version,
            },
            text,
            pages,
            warnings,
        })
    }
}

fn open_document(bytes: Vec<u8>, path: &Path) -> Result<(Epub, Vec<String>)> {
    let bytes = Arc::<[u8]>::from(bytes);
    match Epub::options().read(Cursor::new(Arc::clone(&bytes))) {
        Ok(document) => Ok((document, Vec::new())),
        Err(navigation_error) => {
            let mut options = Epub::options();
            options.skip_toc(true);
            let document = options.read(Cursor::new(bytes)).with_context(|| {
                format!(
                    "failed to open EPUB {} after navigation parse failure: {navigation_error}",
                    path.display()
                )
            })?;
            Ok((
                document,
                vec![format!(
                    "EPUB navigation could not be parsed ({navigation_error}); derived navigation from spine headings"
                )],
            ))
        }
    }
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
