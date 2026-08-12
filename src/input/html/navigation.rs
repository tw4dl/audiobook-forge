use anyhow::Result;

use super::dom::{Element, Node};
use super::{attribute, normalize_inline, parse_xhtml_nodes};

#[derive(Clone, Debug)]
pub(in crate::input) struct ParsedNavigationEntry {
    pub(in crate::input) id: Option<String>,
    pub(in crate::input) kind: Option<String>,
    pub(in crate::input) label: String,
    pub(in crate::input) href: Option<String>,
    pub(in crate::input) children: Vec<Self>,
}

#[derive(Debug)]
pub(in crate::input) struct ParsedEpubNavigation {
    pub(in crate::input) resource: String,
    pub(in crate::input) contents: Vec<ParsedNavigationEntry>,
    pub(in crate::input) pages: Vec<ParsedNavigationEntry>,
    pub(in crate::input) warnings: Vec<String>,
}

pub(in crate::input) fn parse_epub_navigation(
    source: &str,
    resource: &str,
) -> Result<ParsedEpubNavigation> {
    let (nodes, warnings) = parse_xhtml_nodes(source, &format!("EPUB navigation {resource}"))?;
    let mut contents = Vec::new();
    let mut pages = Vec::new();
    collect_navigation(&nodes, &mut contents, &mut pages);
    Ok(ParsedEpubNavigation {
        resource: resource.to_owned(),
        contents,
        pages,
        warnings,
    })
}

fn collect_navigation(
    nodes: &[Node],
    contents: &mut Vec<ParsedNavigationEntry>,
    pages: &mut Vec<ParsedNavigationEntry>,
) {
    for node in nodes {
        let Node::Element(element) = node else {
            continue;
        };
        if element.name == "nav"
            && let Some(kind) =
                attribute(element, "epub:type").or_else(|| attribute(element, "type"))
            && let Some(list) = direct_child(element, "ol")
        {
            let entries = parse_list(list);
            if has_xml_token(kind, "toc") {
                contents.extend(entries);
            } else if has_xml_token(kind, "page-list") {
                pages.extend(entries);
            }
        }
        collect_navigation(&element.children, contents, pages);
    }
}

fn parse_list(list: &Element) -> Vec<ParsedNavigationEntry> {
    list.children
        .iter()
        .filter_map(|node| match node {
            Node::Element(element) if element.name == "li" => Some(parse_item(element)),
            _ => None,
        })
        .collect()
}

fn parse_item(item: &Element) -> ParsedNavigationEntry {
    let label_element = direct_child(item, "a").or_else(|| direct_child(item, "span"));
    let label = label_element.map(accessible_label).unwrap_or_default();
    let href = label_element
        .and_then(|element| attribute(element, "href"))
        .map(str::to_owned);
    let id = label_element
        .and_then(|element| attribute(element, "id"))
        .or_else(|| attribute(item, "id"))
        .map(str::to_owned);
    let kind = label_element
        .and_then(|element| attribute(element, "epub:type").or_else(|| attribute(element, "type")))
        .map(str::to_owned);
    let children = item
        .children
        .iter()
        .filter_map(|node| match node {
            Node::Element(element) if element.name == "ol" => Some(parse_list(element)),
            _ => None,
        })
        .flatten()
        .collect();
    ParsedNavigationEntry {
        id,
        kind,
        label,
        href,
        children,
    }
}

fn accessible_label(element: &Element) -> String {
    if let Some(label) = attribute(element, "aria-label")
        .or_else(|| attribute(element, "title"))
        .map(normalize_inline)
        .filter(|label| !label.is_empty())
    {
        return label;
    }
    let mut parts = Vec::new();
    collect_accessible_text(&element.children, &mut parts);
    normalize_inline(&parts.join(" "))
}

fn collect_accessible_text(nodes: &[Node], parts: &mut Vec<String>) {
    for node in nodes {
        match node {
            Node::Text(text) => parts.push(text.clone()),
            Node::Element(element) if element.name == "img" => {
                if let Some(text) =
                    attribute(element, "alt").or_else(|| attribute(element, "title"))
                {
                    parts.push(text.to_owned());
                }
            }
            Node::Element(element) => collect_accessible_text(&element.children, parts),
        }
    }
}

fn direct_child<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
    element.children.iter().find_map(|node| match node {
        Node::Element(child) if child.name == name => Some(child),
        _ => None,
    })
}

fn has_xml_token(value: &str, expected: &str) -> bool {
    value
        .split([' ', '\t', '\n', '\r'])
        .any(|token| token == expected)
}
