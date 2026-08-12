use std::collections::HashMap;

use crate::book::{Provenance, Section, SectionKind, SourcePosition};

use super::super::super::epub_source_range;
use super::super::content::SpineDocument;

pub(super) fn append_spine_documents(root: &mut Section, documents: Vec<SpineDocument>) {
    for mut document in documents {
        append_document_blocks(root, &mut document);
        root.children.append(&mut document.root.children);
    }
}

pub(super) fn merge_document(root: &mut Section, mut document: SpineDocument) {
    let fallback = first_section_id_for_resource(root, &document.resource);
    let merge_first_by_resource = fallback
        .as_deref()
        .is_some_and(|id| section_has_file_only_target(root, id, &document.resource));
    if merge_first_by_resource {
        let target = fallback
            .as_deref()
            .and_then(|id| find_section_mut_by_id(root, id))
            .expect("file-only target was found before mutation");
        target.blocks.append(&mut document.root.blocks);
    } else {
        append_document_blocks(root, &mut document);
    }
    let heading_aliases = std::mem::take(&mut document.heading_aliases);
    for (index, section) in document.root.children.into_iter().enumerate() {
        merge_content_section(
            root,
            section,
            fallback.as_deref(),
            merge_first_by_resource && index == 0,
            &heading_aliases,
        );
    }
}

fn append_document_blocks(root: &mut Section, document: &mut SpineDocument) {
    if document.root.blocks.is_empty() {
        return;
    }
    let mut body = Section::new(
        format!("epub:{}:body", document.resource),
        SectionKind::BodyMatter,
        None,
        1,
        Provenance::Derived,
    );
    body.source_range = Some(epub_source_range(&document.resource, None));
    body.blocks.append(&mut document.root.blocks);
    root.children.push(body);
}

fn merge_content_section(
    root: &mut Section,
    mut content: Section,
    fallback_id: Option<&str>,
    allow_resource_fallback: bool,
    heading_aliases: &HashMap<String, usize>,
) {
    let exact_id = content.source_range.as_ref().and_then(|range| {
        section_id_for_position(root, &range.start)
            .or_else(|| section_id_for_heading_alias(root, &range.start, heading_aliases))
    });
    let matched_id = exact_id.or_else(|| {
        allow_resource_fallback
            .then(|| fallback_id.map(str::to_owned))
            .flatten()
    });
    let current_id = if let Some(id) = matched_id {
        if let Some(target) = find_section_mut_by_id(root, &id) {
            if matches!(target.kind, SectionKind::Section | SectionKind::Other)
                && !matches!(
                    content.kind,
                    SectionKind::Book
                        | SectionKind::BodyMatter
                        | SectionKind::Section
                        | SectionKind::Other
                )
            {
                target.kind = content.kind;
            }
            target.blocks.append(&mut content.blocks);
        }
        id
    } else {
        let children = std::mem::take(&mut content.children);
        let id = content.id.clone();
        if let Some(parent) = fallback_id.and_then(|id| find_section_mut_by_id(root, id)) {
            content.level = parent.level.saturating_add(1);
            parent.children.push(content);
        } else {
            root.children.push(content);
        }
        for child in children {
            merge_content_section(root, child, Some(&id), false, heading_aliases);
        }
        return;
    };
    for child in content.children {
        merge_content_section(root, child, Some(&current_id), false, heading_aliases);
    }
}

fn section_id_for_position(root: &Section, position: &SourcePosition) -> Option<String> {
    for section in &root.children {
        if section
            .source_range
            .as_ref()
            .is_some_and(|range| source_positions_match(&range.start, position))
        {
            return Some(section.id.clone());
        }
        if let Some(id) = section_id_for_position(section, position) {
            return Some(id);
        }
    }
    None
}

fn section_id_for_heading_alias(
    section: &Section,
    content_position: &SourcePosition,
    heading_aliases: &HashMap<String, usize>,
) -> Option<String> {
    let SourcePosition::Epub {
        resource,
        character_offset: Some(character_offset),
        ..
    } = content_position
    else {
        return None;
    };
    if matches!(
        section.source_range.as_ref().map(|range| &range.start),
        Some(SourcePosition::Epub {
            resource: section_resource,
            fragment: Some(fragment),
            ..
        }) if section_resource == resource
            && heading_aliases.get(fragment) == Some(character_offset)
    ) {
        return Some(section.id.clone());
    }
    section
        .children
        .iter()
        .find_map(|child| section_id_for_heading_alias(child, content_position, heading_aliases))
}

fn source_positions_match(left: &SourcePosition, right: &SourcePosition) -> bool {
    match (left, right) {
        (
            SourcePosition::Epub {
                resource: left_resource,
                fragment: Some(left_fragment),
                ..
            },
            SourcePosition::Epub {
                resource: right_resource,
                fragment: Some(right_fragment),
                ..
            },
        ) => left_resource == right_resource && left_fragment == right_fragment,
        _ => left == right,
    }
}

fn section_has_file_only_target(root: &Section, id: &str, resource: &str) -> bool {
    find_section_by_id(root, id).is_some_and(|section| {
        matches!(
            section.source_range.as_ref().map(|range| &range.start),
            Some(SourcePosition::Epub {
                resource: section_resource,
                fragment: None,
                ..
            }) if section_resource == resource
        )
    })
}

fn first_section_id_for_resource(root: &Section, resource: &str) -> Option<String> {
    for section in &root.children {
        if matches!(
            section.source_range.as_ref().map(|range| &range.start),
            Some(SourcePosition::Epub { resource: section_resource, .. }) if section_resource == resource
        ) {
            return Some(section.id.clone());
        }
        if let Some(id) = first_section_id_for_resource(section, resource) {
            return Some(id);
        }
    }
    None
}

fn find_section_by_id<'a>(section: &'a Section, id: &str) -> Option<&'a Section> {
    if section.id == id {
        return Some(section);
    }
    section
        .children
        .iter()
        .find_map(|child| find_section_by_id(child, id))
}

fn find_section_mut_by_id<'a>(section: &'a mut Section, id: &str) -> Option<&'a mut Section> {
    if section.id == id {
        return Some(section);
    }
    section
        .children
        .iter_mut()
        .find_map(|child| find_section_mut_by_id(child, id))
}
