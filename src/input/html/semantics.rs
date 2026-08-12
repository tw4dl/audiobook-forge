use std::collections::HashMap;

use crate::book::{PageMarker, SectionKind};
use crate::input::semantic_section_kind;

use super::dom::{Element, Node};
use super::{attribute, element_text};

pub(super) fn epub_page_markers(
    nodes: &[Node],
    resource: &str,
    warnings: &mut Vec<String>,
) -> Vec<PageMarker> {
    let mut pages = Vec::new();
    collect_page_markers(nodes, resource, warnings, &mut pages);
    pages
}

fn collect_page_markers(
    nodes: &[Node],
    resource: &str,
    warnings: &mut Vec<String>,
    pages: &mut Vec<PageMarker>,
) {
    for node in nodes {
        let Node::Element(element) = node else {
            continue;
        };
        if has_semantic_token(element, "pagebreak") {
            let fragment = attribute(element, "id").filter(|id| !id.trim().is_empty());
            let label = attribute(element, "title")
                .or_else(|| attribute(element, "aria-label"))
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    let text = element_text(element);
                    (!text.is_empty()).then_some(text)
                });
            match (fragment, element.source_offset, label) {
                (fragment, character_offset, Some(label))
                    if fragment.is_some() || character_offset.is_some() =>
                {
                    pages.push(PageMarker {
                        label,
                        position: super::super::epub_source_position_at(
                            resource,
                            fragment,
                            character_offset,
                        ),
                    });
                }
                _ => warnings.push(format!(
                    "EPUB pagebreak in {resource} has no usable source position or label; skipped"
                )),
            }
        }
        collect_page_markers(&element.children, resource, warnings, pages);
    }
}

pub(super) fn epub_anchors(nodes: &[Node]) -> HashMap<String, Option<usize>> {
    let mut anchors = HashMap::new();
    collect_anchors(nodes, &mut anchors);
    anchors
}

pub(super) fn epub_heading_aliases(nodes: &[Node]) -> HashMap<String, usize> {
    let mut aliases = HashMap::new();
    collect_heading_aliases(nodes, &mut aliases);
    aliases
}

fn collect_heading_aliases(nodes: &[Node], aliases: &mut HashMap<String, usize>) -> Option<usize> {
    let mut first_heading = None;
    for node in nodes {
        let Node::Element(element) = node else {
            continue;
        };
        let heading = if matches!(
            element.name.as_str(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        ) {
            element.source_offset
        } else {
            collect_heading_aliases(&element.children, aliases)
        };
        if let (Some(id), Some(heading)) = (attribute(element, "id"), heading) {
            aliases.insert(id.to_owned(), heading);
        }
        first_heading = first_heading.or(heading);
    }
    first_heading
}

fn collect_anchors(nodes: &[Node], anchors: &mut HashMap<String, Option<usize>>) {
    for node in nodes {
        if let Node::Element(element) = node {
            if let Some(id) = attribute(element, "id").filter(|id| !id.is_empty()) {
                anchors.insert(id.to_owned(), element.source_offset);
            }
            collect_anchors(&element.children, anchors);
        }
    }
}

fn has_semantic_token(element: &Element, expected: &str) -> bool {
    ["epub:type", "type", "role"]
        .into_iter()
        .filter_map(|name| attribute(element, name))
        .flat_map(str::split_ascii_whitespace)
        .any(|token| token == expected || token.strip_prefix("doc-") == Some(expected))
}

pub(super) fn is_note_element(element: &Element) -> bool {
    has_semantic_token(element, "footnote") || has_semantic_token(element, "endnote")
}

pub(super) fn element_section_kind(element: &Element) -> Option<SectionKind> {
    ["epub:type", "type", "role"]
        .into_iter()
        .filter_map(|name| attribute(element, name))
        .find_map(semantic_section_kind)
}
