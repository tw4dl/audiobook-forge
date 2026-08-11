use std::collections::HashMap;

use anyhow::{Context, Result};
use rbook::Epub;

use crate::book::{PageMarker, Section};

use super::super::html::parse_epub_xhtml;

pub(super) struct SpineDocument {
    pub(super) resource: String,
    pub(super) root: Section,
    pub(super) pages: Vec<PageMarker>,
    pub(super) anchors: HashMap<String, Option<usize>>,
    pub(super) heading_aliases: HashMap<String, usize>,
}

pub(super) fn read_spine(epub: &Epub) -> Result<(Vec<SpineDocument>, Vec<String>)> {
    let mut documents = Vec::new();
    let mut warnings = Vec::new();
    for spine_entry in epub.spine() {
        let primary = spine_entry.manifest_entry().with_context(|| {
            format!(
                "EPUB spine references missing manifest item {:?}",
                spine_entry.idref()
            )
        })?;
        let manifest = if supported_content_type(primary.media_type()) {
            primary
        } else {
            let fallback = primary
                .fallbacks()
                .find(|entry| supported_content_type(entry.media_type()))
                .with_context(|| {
                    format!(
                        "EPUB spine resource {} ({}) has no supported XHTML, HTML, or SVG fallback",
                        primary.href().path().decode(),
                        primary.media_type()
                    )
                })?;
            warnings.push(format!(
                "EPUB spine resource {} ({}) used fallback {} ({})",
                primary.href().path().decode(),
                primary.media_type(),
                fallback.href().path().decode(),
                fallback.media_type()
            ));
            fallback
        };
        let resource = manifest.href().path().decode().into_owned();
        let content = manifest
            .read_str()
            .with_context(|| format!("failed to read EPUB spine resource {resource}"))?;
        let parsed = parse_epub_xhtml(&content, &resource, manifest.id())
            .with_context(|| format!("failed to parse EPUB spine resource {resource}"))?;
        warnings.extend(parsed.warnings);
        documents.push(SpineDocument {
            resource,
            root: parsed.root,
            pages: parsed.pages,
            anchors: parsed.anchors,
            heading_aliases: parsed.heading_aliases,
        });
    }
    Ok((documents, warnings))
}

fn supported_content_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "application/xhtml+xml" | "text/html" | "image/svg+xml"
    )
}
