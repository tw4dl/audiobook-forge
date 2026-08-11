use std::collections::{HashMap, HashSet};

use rbook::Epub;
use rbook::epub::toc::EpubTocEntry;

use crate::book::{PageMarker, Provenance, Section, SectionKind, SourcePosition, SourceRange};

use super::super::html::{ParsedEpubNavigation, ParsedNavigationEntry, parse_epub_navigation};
use super::super::{
    epub_source_position_at, epub_source_range_at, heading_kind, semantic_section_kind,
};
use super::content::SpineDocument;
use super::protection::path::normalize_archive_path;

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
    let raw_navigation = read_raw_navigation(epub, &mut warnings);
    let contents = epub
        .toc()
        .contents()
        .filter(|contents| !contents.is_empty());
    let use_raw_contents = raw_navigation
        .as_ref()
        .is_some_and(|navigation| !navigation.contents.is_empty())
        && contents.is_none_or(|contents| !rbook_navigation_has_labels(contents));

    if use_raw_contents {
        let navigation = raw_navigation
            .as_ref()
            .expect("raw navigation was checked before use");
        let mut next_id = 1_usize;
        let mut used_ids = HashSet::new();
        for entry in &navigation.contents {
            root.children.extend(raw_toc_sections(
                entry,
                &navigation.resource,
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
    } else if let Some(contents) =
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
    if pages.is_empty()
        && let Some(navigation) = raw_navigation.as_ref()
    {
        pages = raw_page_markers(navigation, &spine_index, &mut warnings);
    }
    if pages.is_empty() {
        pages = spine_pages;
    }
    (root, pages, warnings)
}

fn read_raw_navigation(epub: &Epub, warnings: &mut Vec<String>) -> Option<ParsedEpubNavigation> {
    let entry = epub.manifest().by_property("nav").next()?;
    let resource = entry.href().path().decode().into_owned();
    let source = match entry.read_str() {
        Ok(source) => source,
        Err(error) => {
            warnings.push(format!(
                "EPUB navigation {resource} could not be read for standards fallback: {error}"
            ));
            return None;
        }
    };
    match parse_epub_navigation(&source, &resource) {
        Ok(mut navigation) => {
            warnings.append(&mut navigation.warnings);
            Some(navigation)
        }
        Err(error) => {
            warnings.push(format!(
                "EPUB navigation {resource} could not be parsed for standards fallback: {error}"
            ));
            None
        }
    }
}

fn rbook_navigation_has_labels(contents: EpubTocEntry<'_>) -> bool {
    let mut saw_entry = false;
    for entry in contents {
        saw_entry = true;
        if !entry_and_descendants_have_labels(entry) {
            return false;
        }
    }
    saw_entry
}

fn entry_and_descendants_have_labels(entry: EpubTocEntry<'_>) -> bool {
    !entry.label().trim().is_empty() && entry.into_iter().all(entry_and_descendants_have_labels)
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

#[allow(clippy::too_many_arguments)]
fn raw_toc_sections(
    entry: &ParsedNavigationEntry,
    navigation_resource: &str,
    level: u8,
    next_id: &mut usize,
    used_ids: &mut HashSet<String>,
    spine_index: &SpineIndex,
    warnings: &mut Vec<String>,
) -> Vec<Section> {
    let label = entry.label.trim();
    if label.is_empty() {
        warnings.push("EPUB navigation entry has an empty label; skipped".to_owned());
        return promote_raw_children(
            entry,
            navigation_resource,
            level,
            next_id,
            used_ids,
            spine_index,
            warnings,
        );
    }
    let target = match resolved_raw_target(entry, navigation_resource, spine_index) {
        Ok(target) => target,
        Err(issue) => {
            warnings.push(format!(
                "EPUB navigation entry {label:?} {}; skipped",
                issue.description()
            ));
            return promote_raw_children(
                entry,
                navigation_resource,
                level,
                next_id,
                used_ids,
                spine_index,
                warnings,
            );
        }
    };
    let id = unique_id(entry.id.as_deref(), next_id, used_ids);
    let mut section = Section::new(
        id,
        section_kind(entry.kind.as_deref().unwrap_or_default(), label),
        Some(label.to_owned()),
        level,
        Provenance::Authored,
    );
    section.source_range = target.map(|target| target.source_range());
    for child in &entry.children {
        section.children.extend(raw_toc_sections(
            child,
            navigation_resource,
            level.saturating_add(1),
            next_id,
            used_ids,
            spine_index,
            warnings,
        ));
    }
    vec![section]
}

#[allow(clippy::too_many_arguments)]
fn promote_raw_children(
    entry: &ParsedNavigationEntry,
    navigation_resource: &str,
    level: u8,
    next_id: &mut usize,
    used_ids: &mut HashSet<String>,
    spine_index: &SpineIndex,
    warnings: &mut Vec<String>,
) -> Vec<Section> {
    entry
        .children
        .iter()
        .flat_map(|child| {
            raw_toc_sections(
                child,
                navigation_resource,
                level,
                next_id,
                used_ids,
                spine_index,
                warnings,
            )
        })
        .collect()
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
    character_offset: Option<usize>,
}

impl ResolvedTarget {
    fn source_range(&self) -> SourceRange {
        epub_source_range_at(
            &self.resource,
            self.fragment.as_deref(),
            self.character_offset,
        )
    }

    fn source_position(&self) -> SourcePosition {
        epub_source_position_at(
            &self.resource,
            self.fragment.as_deref(),
            self.character_offset,
        )
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
    let fragment = href
        .fragment()
        .map(percent_decode)
        .filter(|fragment| !fragment.is_empty());
    let character_offset = if let Some(fragment) = fragment.as_deref() {
        let Some(offset) = spine_index
            .anchors
            .get(&resource)
            .and_then(|anchors| anchors.get(fragment))
        else {
            return Err(TargetIssue::MissingFragment(fragment.to_owned()));
        };
        *offset
    } else {
        None
    };
    Ok(Some(ResolvedTarget {
        resource,
        fragment,
        character_offset,
    }))
}

fn resolved_raw_target(
    entry: &ParsedNavigationEntry,
    navigation_resource: &str,
    spine_index: &SpineIndex,
) -> Result<Option<ResolvedTarget>, TargetIssue> {
    let Some(href) = entry.href.as_deref() else {
        return Ok(None);
    };
    let (path, fragment) = href
        .split_once('#')
        .map_or((href, None), |(path, fragment)| (path, Some(fragment)));
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let resource = if path.is_empty() {
        navigation_resource.to_owned()
    } else {
        let absolute = path.starts_with('/');
        let path = path.strip_prefix('/').unwrap_or(path);
        let base = (!absolute)
            .then(|| {
                navigation_resource
                    .trim_start_matches('/')
                    .rsplit_once('/')
                    .map(|(parent, _)| parent)
            })
            .flatten();
        let normalized = normalize_archive_path(base, path).ok_or(TargetIssue::MissingResource)?;
        format!("/{normalized}")
    };
    if !spine_index.order.contains_key(&resource) {
        return Err(TargetIssue::OutsideSpine);
    }
    let fragment = fragment
        .map(percent_decode)
        .filter(|fragment| !fragment.is_empty());
    let character_offset = if let Some(fragment) = fragment.as_deref() {
        let Some(offset) = spine_index
            .anchors
            .get(&resource)
            .and_then(|anchors| anchors.get(fragment))
        else {
            return Err(TargetIssue::MissingFragment(fragment.to_owned()));
        };
        *offset
    } else {
        None
    };
    Ok(Some(ResolvedTarget {
        resource,
        fragment,
        character_offset,
    }))
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

fn raw_page_markers(
    navigation: &ParsedEpubNavigation,
    spine_index: &SpineIndex,
    warnings: &mut Vec<String>,
) -> Vec<PageMarker> {
    let mut pages = Vec::new();
    let mut stack = navigation.pages.iter().rev().collect::<Vec<_>>();
    while let Some(entry) = stack.pop() {
        if pages.len() >= MAX_NAVIGATION_ENTRIES {
            warnings.push(format!(
                "EPUB page list has more than {MAX_NAVIGATION_ENTRIES} entries; ignored"
            ));
            return Vec::new();
        }
        stack.extend(entry.children.iter().rev());
        let label = entry.label.trim();
        if label.is_empty() {
            warnings.push("EPUB page list contains an empty label; skipped".to_owned());
            continue;
        }
        match resolved_raw_target(entry, &navigation.resource, spine_index) {
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
