mod dom;
mod semantics;
mod xhtml;

use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::book::{
    Block, BookMetadata, CanonicalBook, FigureBlock, ListBlock, PageMarker, Provenance, Section,
    SectionKind, SourceDocument, SourceFormat, TextBlock,
};

use self::dom::{Element, Node, standardize_html, tokenize, tokenize_xhtml, validate_input_bounds};
use self::semantics::{
    element_section_kind, epub_anchors, epub_heading_aliases, epub_page_markers, is_note_element,
};
use self::xhtml::XhtmlParseError;
use super::{
    BookImporter, ImportSource, epub_source_range, epub_source_range_at, heading_kind,
    normalize_text, section_text, title_from_path,
};

pub(super) struct HtmlImporter;

impl BookImporter for HtmlImporter {
    fn import(&self, input: ImportSource) -> Result<CanonicalBook> {
        let (path, bytes) = input.into_parts();
        let source = String::from_utf8(bytes)
            .with_context(|| format!("failed to read UTF-8 HTML from {}", path.display()))?;
        let is_xhtml = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xhtml"));
        let (nodes, warnings) = if is_xhtml {
            parse_xhtml_nodes(&source, "XHTML")?
        } else {
            let (standard_html, parse_errors) = standardize_html(&source)?;
            let (nodes, tokenizer_errors) = tokenize(&standard_html);
            let parse_errors = parse_errors + tokenizer_errors;
            let warnings = if parse_errors == 0 {
                Vec::new()
            } else {
                vec![format!(
                    "HTML contained {parse_errors} parser warning(s); recovered deterministically"
                )]
            };
            (nodes, warnings)
        };
        let title = document_title(&nodes).unwrap_or_else(|| title_from_path(&path));
        let mut builder = StructureBuilder::new(&title, None);
        consume_nodes(&nodes, &mut builder);
        let root = builder.finish();
        let text = section_text(&root);

        Ok(CanonicalBook {
            metadata: BookMetadata {
                title: Some(title),
                ..BookMetadata::default()
            },
            root,
            source: SourceDocument {
                path,
                format: SourceFormat::Html,
                format_version: None,
            },
            text,
            pages: Vec::new(),
            warnings,
        })
    }
}

pub(super) struct ParsedEpubHtml {
    pub(super) root: Section,
    pub(super) pages: Vec<PageMarker>,
    pub(super) anchors: HashMap<String, Option<usize>>,
    pub(super) heading_aliases: HashMap<String, usize>,
    pub(super) warnings: Vec<String>,
}

pub(super) fn parse_epub_xhtml(
    source: &str,
    resource: &str,
    fallback_title: &str,
) -> Result<ParsedEpubHtml> {
    let context = format!("EPUB resource {resource}");
    let (nodes, mut warnings) = parse_xhtml_nodes(source, &context)?;
    let title = document_title(&nodes).unwrap_or_else(|| fallback_title.to_owned());
    let mut builder = StructureBuilder::new(&title, Some(resource));
    consume_nodes(&nodes, &mut builder);
    let pages = epub_page_markers(&nodes, resource, &mut warnings);
    let anchors = epub_anchors(&nodes);
    let heading_aliases = epub_heading_aliases(&nodes);
    Ok(ParsedEpubHtml {
        root: builder.finish(),
        pages,
        anchors,
        heading_aliases,
        warnings,
    })
}

fn parse_xhtml_nodes(source: &str, context: &str) -> Result<(Vec<Node>, Vec<String>)> {
    validate_input_bounds(source)?;
    match xhtml::parse(source) {
        Ok(nodes) => Ok((nodes, Vec::new())),
        Err(XhtmlParseError::Bounds(error)) => Err(error),
        Err(XhtmlParseError::Malformed(error)) => {
            let (nodes, parse_errors) = tokenize_xhtml(source)?;
            Ok((
                nodes,
                vec![format!(
                    "{context} is not well-formed XHTML ({error}); recovered with {parse_errors} HTML parser warning(s)"
                )],
            ))
        }
    }
}

pub(super) fn plain_text(source: &str) -> Result<String> {
    let (html, _) = standardize_html(source)?;
    let text = html2text::from_read(html.as_bytes(), 10_000)
        .context("failed to render embedded HTML text")?;
    Ok(normalize_text(&text))
}

struct StructureBuilder {
    root: Section,
    stack: Vec<Section>,
    next_section_id: usize,
    resource: Option<String>,
}

impl StructureBuilder {
    fn new(title: &str, resource: Option<&str>) -> Self {
        Self {
            root: Section::new(
                "book",
                SectionKind::Book,
                Some(title.to_owned()),
                0,
                Provenance::Derived,
            ),
            stack: Vec::new(),
            next_section_id: 1,
            resource: resource.map(str::to_owned),
        }
    }

    fn push_heading(
        &mut self,
        level: u8,
        title: String,
        semantic_kind: Option<SectionKind>,
        fragment: Option<&str>,
        character_offset: Option<usize>,
    ) {
        self.close_sections(level);
        let id = self.resource.as_ref().map_or_else(
            || format!("section-{}", self.next_section_id),
            |resource| {
                fragment.map_or_else(
                    || format!("epub:{resource}:section-{}", self.next_section_id),
                    |fragment| format!("epub:{resource}#{fragment}"),
                )
            },
        );
        let mut section = Section::new(
            id,
            semantic_kind.unwrap_or_else(|| heading_kind(&title)),
            Some(title),
            level,
            Provenance::Authored,
        );
        section.source_range = self
            .resource
            .as_deref()
            .map(|resource| epub_source_range_at(resource, fragment, character_offset));
        self.stack.push(section);
        self.next_section_id += 1;
    }

    fn push_block(&mut self, block: Block) {
        self.push_block_at_source(block, None, None);
    }

    fn push_block_at_source(
        &mut self,
        mut block: Block,
        fragment: Option<&str>,
        character_offset: Option<usize>,
    ) {
        let range = self
            .resource
            .as_deref()
            .filter(|_| fragment.is_some() || character_offset.is_some())
            .map(|resource| epub_source_range_at(resource, fragment, character_offset))
            .or_else(|| {
                self.stack
                    .last()
                    .and_then(|section| section.source_range.clone())
            })
            .or_else(|| {
                self.resource
                    .as_deref()
                    .map(|resource| epub_source_range(resource, None))
            });
        set_block_source_range(&mut block, range);
        if let Some(section) = self.stack.last_mut() {
            section.blocks.push(block);
        } else {
            self.root.blocks.push(block);
        }
    }

    fn close_sections(&mut self, next_level: u8) {
        while self
            .stack
            .last()
            .is_some_and(|section| section.level >= next_level)
        {
            let section = self.stack.pop().expect("section stack has a last item");
            if let Some(parent) = self.stack.last_mut() {
                parent.children.push(section);
            } else {
                self.root.children.push(section);
            }
        }
    }

    fn finish(mut self) -> Section {
        self.close_sections(0);
        self.root
    }
}

fn consume_nodes(nodes: &[Node], builder: &mut StructureBuilder) {
    consume_nodes_with_kind(nodes, builder, &mut None);
}

fn consume_nodes_with_kind(
    nodes: &[Node],
    builder: &mut StructureBuilder,
    inherited_kind: &mut Option<SectionKind>,
) {
    let mut inline_text = String::new();
    for node in nodes {
        if is_structural_node(node) {
            push_paragraph(&mut inline_text, builder);
            consume_structural_node(node, builder, inherited_kind);
        } else {
            collect_text(node, &mut inline_text);
        }
    }
    push_paragraph(&mut inline_text, builder);
}

fn is_structural_node(node: &Node) -> bool {
    let Node::Element(element) = node else {
        return false;
    };
    matches!(
        element.name.as_str(),
        "html"
            | "body"
            | "main"
            | "article"
            | "section"
            | "div"
            | "header"
            | "footer"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "p"
            | "blockquote"
            | "ol"
            | "ul"
            | "figure"
            | "aside"
            | "nav"
            | "pre"
            | "hr"
    )
}

fn consume_structural_node(
    node: &Node,
    builder: &mut StructureBuilder,
    inherited_kind: &mut Option<SectionKind>,
) {
    let Node::Element(element) = node else {
        return;
    };
    match element.name.as_str() {
        "html" | "body" | "main" | "article" | "section" | "div" | "header" | "footer" => {
            if let Some(kind) =
                element_section_kind(element).filter(|kind| *kind != SectionKind::BodyMatter)
            {
                consume_nodes_with_kind(&element.children, builder, &mut Some(kind));
            } else {
                consume_nodes_with_kind(&element.children, builder, inherited_kind);
            }
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let title = node_text(node);
            if !title.is_empty() {
                let level = element.name.as_bytes()[1] - b'0';
                let semantic_kind = element_section_kind(element).or_else(|| inherited_kind.take());
                builder.push_heading(
                    level,
                    title,
                    semantic_kind,
                    attribute(element, "id"),
                    element.source_offset,
                );
            }
        }
        _ if is_note_element(element) => push_text_block(node, builder, Block::Footnote),
        "p" | "pre" => push_text_block(node, builder, Block::Paragraph),
        "blockquote" => push_text_block(node, builder, Block::Quote),
        "aside" => push_text_block(node, builder, Block::Aside),
        "nav" => push_text_block(node, builder, Block::Navigation),
        "ol" | "ul" => push_list(element, builder),
        "figure" => push_figure(element, builder),
        "hr" => {}
        _ => consume_nodes_with_kind(&element.children, builder, inherited_kind),
    }
}

fn set_block_source_range(block: &mut Block, range: Option<crate::book::SourceRange>) {
    match block {
        Block::Paragraph(block)
        | Block::Quote(block)
        | Block::Footnote(block)
        | Block::Aside(block)
        | Block::Navigation(block)
        | Block::Code(block) => block.source_range = range,
        Block::List(block) => block.source_range = range,
        Block::Figure(block) => block.source_range = range,
    }
}

fn push_text_block(node: &Node, builder: &mut StructureBuilder, wrap: fn(TextBlock) -> Block) {
    let text = node_text(node);
    if !text.is_empty() {
        let fragment = match node {
            Node::Element(element) => attribute(element, "id"),
            Node::Text(_) => None,
        };
        let character_offset = match node {
            Node::Element(element) => element.source_offset,
            Node::Text(_) => None,
        };
        builder.push_block_at_source(
            wrap(TextBlock {
                text,
                source_range: None,
            }),
            fragment,
            character_offset,
        );
    }
}

fn push_list(element: &Element, builder: &mut StructureBuilder) {
    let mut items = Vec::new();
    for child in &element.children {
        let Node::Element(item) = child else {
            continue;
        };
        if item.name != "li" {
            continue;
        }
        if is_note_element(item) {
            push_list_items(element, builder, &mut items);
            push_text_block(child, builder, Block::Footnote);
        } else {
            let text = node_text(child);
            if !text.is_empty() {
                items.push(text);
            }
        }
    }
    push_list_items(element, builder, &mut items);
}

fn push_list_items(element: &Element, builder: &mut StructureBuilder, items: &mut Vec<String>) {
    if items.is_empty() {
        return;
    }
    let items = std::mem::take(items);
    builder.push_block_at_source(
        Block::List(ListBlock {
            ordered: element.name == "ol",
            text: items.join(". "),
            items,
            source_range: None,
        }),
        attribute(element, "id"),
        element.source_offset,
    );
}

fn push_figure(element: &Element, builder: &mut StructureBuilder) {
    let alt_text = find_element(element, "img")
        .and_then(|image| attribute(image, "alt"))
        .map(normalize_inline)
        .filter(|text| !text.is_empty());
    let caption = find_element(element, "figcaption")
        .map(element_text)
        .filter(|text| !text.is_empty());
    if alt_text.is_some() || caption.is_some() {
        builder.push_block(Block::Figure(FigureBlock {
            alt_text,
            caption,
            source_range: None,
        }));
    }
}

fn push_paragraph(text: &mut String, builder: &mut StructureBuilder) {
    let normalized = normalize_inline(text);
    if !normalized.is_empty() {
        builder.push_block(Block::Paragraph(TextBlock {
            text: normalized,
            source_range: None,
        }));
    }
    text.clear();
}

fn node_text(node: &Node) -> String {
    let mut text = String::new();
    collect_text(node, &mut text);
    normalize_inline(&text)
}

fn element_text(element: &Element) -> String {
    let mut text = String::new();
    for child in &element.children {
        collect_text(child, &mut text);
    }
    normalize_inline(&text)
}

fn collect_text(node: &Node, output: &mut String) {
    match node {
        Node::Text(text) => output.push_str(text),
        Node::Element(element)
            if matches!(
                element.name.as_str(),
                "script" | "style" | "head" | "template" | "noscript"
            ) => {}
        Node::Element(element) if element.name == "br" => output.push(' '),
        Node::Element(element) => {
            for child in &element.children {
                collect_text(child, output);
            }
        }
    }
}

fn normalize_inline(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn document_title(nodes: &[Node]) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::Element(element) if element.name == "title" => {
            let title = node_text(node);
            (!title.is_empty()).then_some(title)
        }
        Node::Element(element) => document_title(&element.children),
        Node::Text(_) => None,
    })
}

fn find_element<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
    for child in &element.children {
        if let Node::Element(child) = child {
            if child.name == name {
                return Some(child);
            }
            if let Some(found) = find_element(child, name) {
                return Some(found);
            }
        }
    }
    None
}

fn attribute<'a>(element: &'a Element, name: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value.as_str()))
}
