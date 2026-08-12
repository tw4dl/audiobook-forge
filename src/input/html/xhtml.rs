use std::str;

use anyhow::{Context, Error, anyhow};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesEnd, BytesStart, Event};

use super::dom::{Element, MAX_HTML_ATTRIBUTES, MAX_HTML_DEPTH, MAX_HTML_NODES, Node};

pub(super) enum XhtmlParseError {
    Bounds(Error),
    Malformed(Error),
}

pub(super) fn parse(source: &str) -> Result<Vec<Node>, XhtmlParseError> {
    let mut reader = Reader::from_str(source);
    let mut roots = Vec::new();
    let mut stack = Vec::<Element>::new();
    let mut node_count = 0_usize;
    let mut attribute_count = 0_usize;
    let mut source_cursor = SourceCursor::default();

    loop {
        let byte_offset = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
        let source_offset = source_cursor.character_offset(source, byte_offset);
        let event = reader
            .read_event()
            .map_err(|error| malformed(anyhow!(error)))?;
        match event {
            Event::Start(start) => {
                count_depth(stack.len() + 1)?;
                count_node(&mut node_count)?;
                stack.push(element(
                    &reader,
                    &start,
                    source_offset,
                    &mut attribute_count,
                )?);
            }
            Event::Empty(start) => {
                count_depth(stack.len() + 1)?;
                count_node(&mut node_count)?;
                let node = Node::Element(element(
                    &reader,
                    &start,
                    source_offset,
                    &mut attribute_count,
                )?);
                append_node(&mut roots, &mut stack, node);
            }
            Event::End(end) => close_element(&end, &mut roots, &mut stack)?,
            Event::Text(text) => {
                let decoded = text
                    .xml10_content()
                    .context("XHTML text is not valid XML text")
                    .map_err(malformed)?;
                let unescaped = quick_xml::escape::unescape(&decoded)
                    .context("XHTML text contains an invalid entity")
                    .map_err(malformed)?;
                append_text(
                    &mut roots,
                    &mut stack,
                    &mut node_count,
                    unescaped.into_owned(),
                )?;
            }
            Event::CData(text) => {
                let decoded = text
                    .xml10_content()
                    .context("XHTML CDATA is not valid text")
                    .map_err(malformed)?;
                append_text(
                    &mut roots,
                    &mut stack,
                    &mut node_count,
                    decoded.into_owned(),
                )?;
            }
            Event::GeneralRef(reference) => {
                let reference = reference
                    .decode()
                    .context("XHTML reference is not UTF-8")
                    .map_err(malformed)?;
                let escaped = format!("&{reference};");
                let decoded = quick_xml::escape::unescape(&escaped)
                    .context("XHTML contains an unsupported entity reference")
                    .map_err(malformed)?;
                append_text(
                    &mut roots,
                    &mut stack,
                    &mut node_count,
                    decoded.into_owned(),
                )?;
            }
            Event::Eof => {
                if stack.is_empty() {
                    return Ok(roots);
                }
                return Err(malformed(anyhow!("XHTML contains an unclosed element")));
            }
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
        }
    }
}

fn close_element(
    end: &BytesEnd<'_>,
    roots: &mut Vec<Node>,
    stack: &mut Vec<Element>,
) -> Result<(), XhtmlParseError> {
    let expected = stack
        .pop()
        .ok_or_else(|| malformed(anyhow!("XHTML contains an unmatched closing element")))?;
    let local_name = end.local_name();
    let actual = str::from_utf8(local_name.as_ref())
        .context("XHTML closing element name is not UTF-8")
        .map_err(malformed)?;
    if expected.name != actual {
        return Err(malformed(anyhow!(
            "XHTML closing element {actual:?} does not match {:?}",
            expected.name
        )));
    }
    append_node(roots, stack, Node::Element(expected));
    Ok(())
}

#[derive(Default)]
struct SourceCursor {
    byte_offset: usize,
    character_offset: usize,
}

impl SourceCursor {
    fn character_offset(&mut self, source: &str, byte_offset: usize) -> usize {
        let byte_offset = byte_offset.min(source.len());
        if byte_offset >= self.byte_offset {
            self.character_offset += source[self.byte_offset..byte_offset].chars().count();
            self.byte_offset = byte_offset;
        }
        self.character_offset
    }
}

fn element(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    source_offset: usize,
    attribute_count: &mut usize,
) -> Result<Element, XhtmlParseError> {
    let local_name = start.local_name();
    let name = str::from_utf8(local_name.as_ref())
        .context("XHTML element name is not UTF-8")
        .map_err(malformed)?
        .to_owned();
    let mut attrs = Vec::new();
    for attribute in start.attributes() {
        *attribute_count = attribute_count.saturating_add(1);
        if *attribute_count > MAX_HTML_ATTRIBUTES {
            return Err(bounds(anyhow!(
                "HTML exceeds {MAX_HTML_ATTRIBUTES} total attribute budget"
            )));
        }
        let attribute = attribute
            .context("XHTML contains an invalid attribute")
            .map_err(malformed)?;
        let key = str::from_utf8(attribute.key.as_ref())
            .context("XHTML attribute name is not UTF-8")
            .map_err(malformed)?
            .to_owned();
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .context("XHTML attribute contains invalid text")
            .map_err(malformed)?
            .into_owned();
        attrs.push((key, value));
    }
    Ok(Element {
        name,
        attrs,
        children: Vec::new(),
        source_offset: Some(source_offset),
    })
}

fn append_text(
    roots: &mut Vec<Node>,
    stack: &mut [Element],
    node_count: &mut usize,
    text: String,
) -> Result<(), XhtmlParseError> {
    if text.is_empty() {
        return Ok(());
    }
    count_node(node_count)?;
    append_node(roots, stack, Node::Text(text));
    Ok(())
}

fn append_node(roots: &mut Vec<Node>, stack: &mut [Element], node: Node) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        roots.push(node);
    }
}

fn count_node(node_count: &mut usize) -> Result<(), XhtmlParseError> {
    *node_count = node_count.saturating_add(1);
    if *node_count > MAX_HTML_NODES {
        return Err(bounds(anyhow!(
            "HTML contains more than {MAX_HTML_NODES} nodes"
        )));
    }
    Ok(())
}

fn count_depth(depth: usize) -> Result<(), XhtmlParseError> {
    if depth > MAX_HTML_DEPTH {
        return Err(bounds(anyhow!(
            "HTML nesting exceeds {MAX_HTML_DEPTH} levels"
        )));
    }
    Ok(())
}

fn bounds(error: Error) -> XhtmlParseError {
    XhtmlParseError::Bounds(error)
}

fn malformed(error: Error) -> XhtmlParseError {
    XhtmlParseError::Malformed(error)
}
