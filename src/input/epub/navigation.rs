use std::collections::{HashMap, HashSet};

use rbook::Epub;
use rbook::epub::toc::EpubTocEntry;

use crate::book::{PageMarker, Provenance, Section, SectionKind, SourcePosition, SourceRange};

use super::super::{epub_source_position, epub_source_range, heading_kind, semantic_section_kind};
use super::content::SpineDocument;

mod merge;
mod ordering;

const MAX_NAVIGATION_DEPTH: usize = 128;
const MAX_NAVIGATION_ENTRIES: usize = 10_000;

struct SpineIndex {
    order: HashMap<String, usize>,
    anchors: HashMap<String, HashMap<String, Option<usize>>>,
}

impl SpineIndex {
    fn new(documents: &[SpineDocument]) -> Self {
        let order = documents
            .iter()
            .enumerate()
            .map(|(index, document)| (document.resource.clone(), index))
            .collect();
        let anchors = documents
            .iter()
            .map(|document| (document.resource.clone(), document.anchors.clone()))
            .collect();
        Self { order, anchors }
    }
}

pub(super) fn build_navigation(
    epub: &Epub,
    title: &str,
    documents: Vec<SpineDocument>,
) -> (Section, Vec<PageMarker>, Vec<String>) {
    let mut root = Section::new(
        "book",
        SectionKind::Book,
        Some(title.to_owned()),
        0,
        Provenance::Derived,
    );
    let mut warnings = Vec::new();
    let spine_index = SpineIndex::new(&documents);
    let spine_pages = documents
        .iter()
        .flat_map(|document| document.pages.iter().cloned())
        .collect::<Vec<_>>();
    let contents = epub
        .toc()
        .contents()
        .filter(|contents| !contents.is_empty());

    if let Some(contents) =
        contents.filter(|contents| navigation_within_bounds(*contents, &mut warnings))
    {
        let mut next_id = 1_usize;
        let mut used_ids = HashSet::new();
        for entry in contents {
            root.children.extend(toc_sections(
                entry,
                1,
                &mut next_id,
                &mut used_ids,
                &spine_index,
                &mut warnings,
            ));
        }
        for document in documents {
            merge::merge_document(&mut root, document);
        }
        ordering::order_sections(&mut root, &spine_index.order, &spine_index.anchors);
    } else {
        if epub
            .toc()
            .contents()
            .is_none_or(|contents| contents.is_empty())
        {
            warnings.push(
                "EPUB has no authored table of contents; derived navigation from spine headings"
                    .to_owned(),
            );
        }
        merge::append_spine_documents(&mut root, documents);
    }

    let mut pages = page_markers(epub, &spine_index, &mut warnings);
    if pages.is_empty() {
        pages = spine_pages;
    }
    (root, pages, warnings)
}

fn navigation_within_bounds(contents: EpubTocEntry<'_>, warnings: &mut Vec<String>) -> bool {
    let mut count = 0_usize;
    for entry in contents.flatten() {
        count += 1;
        if entry.depth() > MAX_NAVIGATION_DEPTH {
            warnings.push(format!(
                "EPUB table of contents has more than {MAX_NAVIGATION_DEPTH} levels; derived navigation from spine headings"
            ));
            return false;
        }
        if count > MAX_NAVIGATION_ENTRIES {
            warnings.push(format!(
                "EPUB table of contents has more than {MAX_NAVIGATION_ENTRIES} entries; derived navigation from spine headings"
            ));
            return false;
        }
    }
    true
}

fn toc_sections(
    entry: EpubTocEntry<'_>,
    level: u8,
    next_id: &mut usize,
    used_ids: &mut HashSet<String>,
    spine_index: &SpineIndex,
    warnings: &mut Vec<String>,
) -> Vec<Section> {
    let target = match resolved_target(entry, spine_index) {
        Ok(target) => target,
        Err(issue) => {
            warnings.push(format!(
                "EPUB navigation entry {:?} {}; skipped",
                entry.label().trim(),
                issue.description()
            ));
            let mut promoted = Vec::new();
            for child in entry {
                promoted.extend(toc_sections(
                    child,
                    level,
                    next_id,
                    used_ids,
                    spine_index,
                    warnings,
                ));
            }
            return promoted;
        }
    };
    let id = unique_id(entry.id(), next_id, used_ids);
    let mut section = Section::new(
        id,
        section_kind(entry.kind().as_str(), entry.label()),
        Some(entry.label().trim().to_owned()),
        level,
        Provenance::Authored,
    );
    section.source_range = target.map(|target| target.source_range());
    for child in entry {
        section.children.extend(toc_sections(
            child,
            level.saturating_add(1),
            next_id,
            used_ids,
            spine_index,
            warnings,
        ));
    }
    vec![section]
}

fn unique_id(
    authored: Option<&str>,
    next_id: &mut usize,
    used_ids: &mut HashSet<String>,
) -> String {
    if let Some(authored) = authored.filter(|id| !id.trim().is_empty()) {
        let authored = authored.to_owned();
        if used_ids.insert(authored.clone()) {
            return authored;
        }
    }
    loop {
        let id = format!("epub-toc-{}", *next_id);
        *next_id += 1;
        if used_ids.insert(id.clone()) {
            return id;
        }
    }
}

fn section_kind(kind: &str, label: &str) -> SectionKind {
    semantic_section_kind(kind).unwrap_or_else(|| heading_kind(label))
}

#[derive(Clone)]
struct ResolvedTarget {
    resource: String,
    fragment: Option<String>,
}

impl ResolvedTarget {
    fn source_range(&self) -> SourceRange {
        epub_source_range(&self.resource, self.fragment.as_deref())
    }

    fn source_position(&self) -> SourcePosition {
        epub_source_position(&self.resource, self.fragment.as_deref())
    }
}

enum TargetIssue {
    MissingResource,
    OutsideSpine,
    MissingFragment(String),
}

impl TargetIssue {
    fn description(&self) -> String {
        match self {
            Self::MissingResource => "references a missing resource".to_owned(),
            Self::OutsideSpine => "references a resource outside the readable spine".to_owned(),
            Self::MissingFragment(fragment) => {
                format!("references missing fragment {fragment:?}")
            }
        }
    }
}

fn resolved_target(
    entry: EpubTocEntry<'_>,
    spine_index: &SpineIndex,
) -> Result<Option<ResolvedTarget>, TargetIssue> {
    let Some(href) = entry.href() else {
        return Ok(None);
    };
    if entry.manifest_entry().is_none() {
        return Err(TargetIssue::MissingResource);
    }
    let resource = href.path().decode().into_owned();
    if !spine_index.order.contains_key(&resource) {
        return Err(TargetIssue::OutsideSpine);
    }
    let fragment = href.fragment().map(percent_decode);
    if let Some(fragment) = fragment.as_deref()
        && !spine_index
            .anchors
            .get(&resource)
            .is_some_and(|anchors| anchors.contains_key(fragment))
    {
        return Err(TargetIssue::MissingFragment(fragment.to_owned()));
    }
    Ok(Some(ResolvedTarget { resource, fragment }))
}

fn page_markers(
    epub: &Epub,
    spine_index: &SpineIndex,
    warnings: &mut Vec<String>,
) -> Vec<PageMarker> {
    let Some(page_list) = epub.toc().page_list() else {
        return Vec::new();
    };
    let mut pages = Vec::new();
    for (index, entry) in page_list.flatten().enumerate() {
        if index >= MAX_NAVIGATION_ENTRIES {
            warnings.push(format!(
                "EPUB page list has more than {MAX_NAVIGATION_ENTRIES} entries; ignored"
            ));
            return Vec::new();
        }
        let label = entry.label().trim();
        if label.is_empty() {
            warnings.push("EPUB page list contains an empty label; skipped".to_owned());
            continue;
        }
        match resolved_target(entry, spine_index) {
            Ok(Some(target)) => pages.push(PageMarker {
                label: label.to_owned(),
                position: target.source_position(),
            }),
            Ok(None) => warnings.push(format!(
                "EPUB page {label:?} has no source location; skipped"
            )),
            Err(issue) => warnings.push(format!(
                "EPUB page {label:?} {}; skipped",
                issue.description()
            )),
        }
    }
    pages
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0_usize;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (bytes.get(index + 1), bytes.get(index + 2))
            && let (Some(high), Some(low)) = (hex(*high), hex(*low))
        {
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|_| raw.to_owned())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
