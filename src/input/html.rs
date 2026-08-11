mod dom;

use anyhow::{Context, Result};

use crate::book::{
    Block, BookMetadata, CanonicalBook, FigureBlock, ListBlock, Provenance, Section, SectionKind,
    SourceDocument, SourceFormat, TextBlock,
};

use self::dom::{Element, Node, standardize_html, tokenize};
use super::{
    BookImporter, ImportSource, heading_kind, normalize_text, section_text, title_from_path,
};

pub(super) struct HtmlImporter;

impl BookImporter for HtmlImporter {
    fn import(&self, input: ImportSource) -> Result<CanonicalBook> {
        let (path, bytes) = input.into_parts();
        let source = String::from_utf8(bytes)
            .with_context(|| format!("failed to read UTF-8 HTML from {}", path.display()))?;
        let (standard_html, parse_errors) = standardize_html(&source)?;
        let (nodes, tokenizer_errors) = tokenize(&standard_html);
        let title = document_title(&nodes).unwrap_or_else(|| title_from_path(&path));
        let mut builder = StructureBuilder::new(&title);
        consume_nodes(&nodes, &mut builder);
        let root = builder.finish();
        let text = section_text(&root);
        let parse_errors = parse_errors + tokenizer_errors;
        let warnings = if parse_errors == 0 {
            Vec::new()
        } else {
            vec![format!(
                "HTML contained {parse_errors} parser warning(s); recovered deterministically"
            )]
        };

        Ok(CanonicalBook {
            metadata: BookMetadata {
                title: Some(title),
                ..BookMetadata::default()
            },
            root,
            source: SourceDocument {
                path,
                format: SourceFormat::Html,
            },
            text,
            warnings,
        })
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
}

impl StructureBuilder {
    fn new(title: &str) -> Self {
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
        }
    }

    fn push_heading(&mut self, level: u8, title: String) {
        self.close_sections(level);
        self.stack.push(Section::new(
            format!("section-{}", self.next_section_id),
            heading_kind(&title),
            Some(title),
            level,
            Provenance::Authored,
        ));
        self.next_section_id += 1;
    }

    fn push_block(&mut self, block: Block) {
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
    let mut inline_text = String::new();
    for node in nodes {
        if is_structural_node(node) {
            push_paragraph(&mut inline_text, builder);
            consume_structural_node(node, builder);
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

fn consume_structural_node(node: &Node, builder: &mut StructureBuilder) {
    let Node::Element(element) = node else {
        return;
    };
    match element.name.as_str() {
        "html" | "body" | "main" | "article" | "section" | "div" | "header" | "footer" => {
            consume_nodes(&element.children, builder);
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let title = node_text(node);
            if !title.is_empty() {
                let level = element.name.as_bytes()[1] - b'0';
                builder.push_heading(level, title);
            }
        }
        "p" | "pre" => push_text_block(node, builder, Block::Paragraph),
        "blockquote" => push_text_block(node, builder, Block::Quote),
        "aside" => push_text_block(node, builder, Block::Aside),
        "nav" => push_text_block(node, builder, Block::Navigation),
        "ol" | "ul" => push_list(element, builder),
        "figure" => push_figure(element, builder),
        "hr" => {}
        _ => consume_nodes(&element.children, builder),
    }
}

fn push_text_block(node: &Node, builder: &mut StructureBuilder, wrap: fn(TextBlock) -> Block) {
    let text = node_text(node);
    if !text.is_empty() {
        builder.push_block(wrap(TextBlock {
            text,
            source_range: None,
        }));
    }
}

fn push_list(element: &Element, builder: &mut StructureBuilder) {
    let items = element
        .children
        .iter()
        .filter_map(|child| match child {
            Node::Element(item) if item.name == "li" => {
                let text = node_text(child);
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        return;
    }
    builder.push_block(Block::List(ListBlock {
        ordered: element.name == "ol",
        text: items.join(". "),
        items,
        source_range: None,
    }));
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
