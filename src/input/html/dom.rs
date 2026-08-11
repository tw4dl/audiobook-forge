use std::cell::{Cell, RefCell};

use anyhow::{Context, Result};
use html2text::{Handle, RcDom};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, EndTag, StartTag, TagToken, Token, TokenSink, TokenSinkResult, Tokenizer,
    TokenizerOpts,
};
use html5ever::{ParseOpts, parse_document};

const MAX_HTML_NODES: usize = 100_000;
const MAX_HTML_DEPTH: usize = 128;
const MAX_HTML_ATTRIBUTES: usize = 4_096;

pub(super) fn standardize_html(source: &str) -> Result<(String, usize)> {
    enforce_raw_attribute_limit(source)?;
    enforce_token_limit(source)?;
    let dom =
        parse_document(RcDom::default(), ParseOpts::default()).one(StrTendril::from_slice(source));
    validate_dom_bounds(&dom.document)?;
    let parse_errors = dom.errors.borrow().len();
    let mut bytes = Vec::new();
    dom.serialize(&mut bytes)
        .context("failed to serialize normalized HTML")?;
    let html = String::from_utf8(bytes).context("normalized HTML was not UTF-8")?;
    Ok((html, parse_errors))
}

fn validate_dom_bounds(document: &Handle) -> Result<()> {
    let mut stack = vec![(document.clone(), 0_usize)];
    let mut node_count = 0_usize;
    while let Some((node, depth)) = stack.pop() {
        node_count += 1;
        if node_count > MAX_HTML_NODES {
            anyhow::bail!("HTML contains more than {MAX_HTML_NODES} nodes");
        }
        if depth > MAX_HTML_DEPTH {
            anyhow::bail!("HTML nesting exceeds {MAX_HTML_DEPTH} levels");
        }
        let children = node.children.borrow();
        stack.extend(
            children
                .iter()
                .rev()
                .cloned()
                .map(|child| (child, depth + 1)),
        );
    }
    Ok(())
}

#[derive(Default)]
struct LimitSink {
    resource_units: Cell<usize>,
    attribute_limit_exceeded: Cell<bool>,
    attributes: Cell<usize>,
}

impl LimitSink {
    fn count_resources(&self, count: usize) {
        self.resource_units
            .set(self.resource_units.get().saturating_add(count));
    }

    fn count_attributes(&self, count: usize) {
        let attributes = self.attributes.get().saturating_add(count);
        self.attributes.set(attributes);
        if attributes > MAX_HTML_ATTRIBUTES {
            self.attribute_limit_exceeded.set(true);
        }
    }
}

impl TokenSink for LimitSink {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        match token {
            Token::TagToken(tag) if tag.kind == StartTag => {
                self.count_attributes(tag.attrs.len());
                self.count_resources(1_usize.saturating_add(tag.attrs.len()));
                match tag.name.as_ref() {
                    "script" => TokenSinkResult::RawData(RawKind::ScriptData),
                    "style" => TokenSinkResult::RawData(RawKind::Rawtext),
                    "title" | "textarea" => TokenSinkResult::RawData(RawKind::Rcdata),
                    _ => TokenSinkResult::Continue,
                }
            }
            Token::TagToken(tag) if tag.kind == EndTag => {
                self.count_attributes(tag.attrs.len());
                self.count_resources(1_usize.saturating_add(tag.attrs.len()));
                TokenSinkResult::Continue
            }
            Token::CharacterTokens(_)
            | Token::CommentToken(_)
            | Token::DoctypeToken(_)
            | Token::NullCharacterToken
            | Token::ParseError(_) => {
                self.count_resources(1);
                TokenSinkResult::Continue
            }
            _ => TokenSinkResult::Continue,
        }
    }
}

fn enforce_token_limit(source: &str) -> Result<()> {
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(source));
    let tokenizer = Tokenizer::new(LimitSink::default(), TokenizerOpts::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    if tokenizer.sink.attribute_limit_exceeded.get() {
        anyhow::bail!("HTML exceeds {MAX_HTML_ATTRIBUTES} total attribute budget");
    }
    if tokenizer.sink.resource_units.get() > MAX_HTML_NODES {
        anyhow::bail!("HTML exceeds {MAX_HTML_NODES} parser resource units");
    }
    Ok(())
}

fn enforce_raw_attribute_limit(source: &str) -> Result<()> {
    let bytes = source.as_bytes();
    let mut index = 0_usize;
    let mut attribute_count = 0_usize;
    while index + 1 < bytes.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        let Some(tag_name_start) = raw_tag_name_start(bytes, index) else {
            index += 1;
            continue;
        };

        index = tag_name_start;
        let mut state = AttributeScanState::TagName;
        while index < bytes.len() {
            let byte = bytes[index];
            match state {
                AttributeScanState::TagName => match byte {
                    b'>' => break,
                    b'/' => state = AttributeScanState::BeforeName,
                    byte if byte.is_ascii_whitespace() => {
                        state = AttributeScanState::BeforeName;
                    }
                    _ => {}
                },
                AttributeScanState::BeforeName => match byte {
                    b'>' => break,
                    b'/' => {}
                    byte if byte.is_ascii_whitespace() => {}
                    _ => {
                        count_raw_attribute(&mut attribute_count)?;
                        state = AttributeScanState::Name;
                    }
                },
                AttributeScanState::Name => match byte {
                    b'>' => break,
                    b'=' => state = AttributeScanState::BeforeValue,
                    b'/' => state = AttributeScanState::BeforeName,
                    byte if byte.is_ascii_whitespace() => {
                        state = AttributeScanState::AfterName;
                    }
                    _ => {}
                },
                AttributeScanState::AfterName => match byte {
                    b'>' => break,
                    b'=' => state = AttributeScanState::BeforeValue,
                    b'/' => state = AttributeScanState::BeforeName,
                    byte if byte.is_ascii_whitespace() => {}
                    _ => {
                        count_raw_attribute(&mut attribute_count)?;
                        state = AttributeScanState::Name;
                    }
                },
                AttributeScanState::BeforeValue => match byte {
                    b'>' => break,
                    b'\'' | b'"' => state = AttributeScanState::QuotedValue(byte),
                    byte if byte.is_ascii_whitespace() => {}
                    _ => state = AttributeScanState::UnquotedValue,
                },
                AttributeScanState::QuotedValue(delimiter) => {
                    if byte == delimiter {
                        state = AttributeScanState::AfterQuotedValue;
                    }
                }
                AttributeScanState::AfterQuotedValue => match byte {
                    b'>' => break,
                    b'/' => state = AttributeScanState::BeforeName,
                    byte if byte.is_ascii_whitespace() => {
                        state = AttributeScanState::BeforeName;
                    }
                    _ => {
                        count_raw_attribute(&mut attribute_count)?;
                        state = AttributeScanState::Name;
                    }
                },
                AttributeScanState::UnquotedValue => match byte {
                    b'>' => break,
                    byte if byte.is_ascii_whitespace() => {
                        state = AttributeScanState::BeforeName;
                    }
                    _ => {}
                },
            }
            index += 1;
        }
    }
    Ok(())
}

fn raw_tag_name_start(bytes: &[u8], less_than: usize) -> Option<usize> {
    let next = less_than.checked_add(1)?;
    if bytes.get(next)?.is_ascii_alphabetic() {
        return Some(next);
    }
    let name = next.checked_add(1)?;
    (bytes.get(next) == Some(&b'/') && bytes.get(name)?.is_ascii_alphabetic()).then_some(name)
}

#[derive(Clone, Copy)]
enum AttributeScanState {
    TagName,
    BeforeName,
    Name,
    AfterName,
    BeforeValue,
    QuotedValue(u8),
    AfterQuotedValue,
    UnquotedValue,
}

fn count_raw_attribute(attribute_count: &mut usize) -> Result<()> {
    *attribute_count = attribute_count.saturating_add(1);
    if *attribute_count > MAX_HTML_ATTRIBUTES {
        anyhow::bail!("HTML exceeds {MAX_HTML_ATTRIBUTES} total attribute budget");
    }
    Ok(())
}

#[derive(Debug)]
enum Event {
    Start {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    End(String),
    Text(String),
}

#[derive(Default)]
struct EventSink {
    events: RefCell<Vec<Event>>,
    parse_errors: Cell<usize>,
}

impl TokenSink for EventSink {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        match token {
            Token::TagToken(tag) if tag.kind == StartTag => {
                let name = tag.name.to_string();
                let attrs = tag
                    .attrs
                    .into_iter()
                    .map(|attribute| {
                        (
                            attribute.name.local.to_string(),
                            attribute.value.to_string(),
                        )
                    })
                    .collect();
                self.events.borrow_mut().push(Event::Start {
                    name: name.clone(),
                    attrs,
                    self_closing: tag.self_closing,
                });
                match name.as_str() {
                    "script" => TokenSinkResult::RawData(RawKind::ScriptData),
                    "style" => TokenSinkResult::RawData(RawKind::Rawtext),
                    "title" | "textarea" => TokenSinkResult::RawData(RawKind::Rcdata),
                    _ => TokenSinkResult::Continue,
                }
            }
            TagToken(tag) if tag.kind == EndTag => {
                self.events
                    .borrow_mut()
                    .push(Event::End(tag.name.to_string()));
                TokenSinkResult::Continue
            }
            Token::CharacterTokens(text) => {
                self.events.borrow_mut().push(Event::Text(text.to_string()));
                TokenSinkResult::Continue
            }
            Token::ParseError(_) => {
                self.parse_errors.set(self.parse_errors.get() + 1);
                TokenSinkResult::Continue
            }
            _ => TokenSinkResult::Continue,
        }
    }
}

pub(super) fn tokenize(source: &str) -> (Vec<Node>, usize) {
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(source));
    let tokenizer = Tokenizer::new(EventSink::default(), TokenizerOpts::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let parse_errors = tokenizer.sink.parse_errors.get();
    let events = tokenizer.sink.events.into_inner();
    (build_tree(events), parse_errors)
}

#[derive(Debug, Clone)]
pub(super) enum Node {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone)]
pub(super) struct Element {
    pub(super) name: String,
    pub(super) attrs: Vec<(String, String)>,
    pub(super) children: Vec<Node>,
}

fn build_tree(events: Vec<Event>) -> Vec<Node> {
    let mut roots = Vec::new();
    let mut stack = Vec::<Element>::new();

    for event in events {
        match event {
            Event::Start {
                name,
                attrs,
                self_closing,
            } => {
                let is_void = is_void_element(&name);
                let element = Element {
                    name,
                    attrs,
                    children: Vec::new(),
                };
                if self_closing || is_void {
                    append_node(&mut roots, &mut stack, Node::Element(element));
                } else {
                    stack.push(element);
                }
            }
            Event::End(name) => {
                if let Some(index) = stack.iter().rposition(|element| element.name == name) {
                    while stack.len() > index {
                        let element = stack.pop().expect("matched HTML element");
                        append_node(&mut roots, &mut stack, Node::Element(element));
                    }
                }
            }
            Event::Text(text) => append_node(&mut roots, &mut stack, Node::Text(text)),
        }
    }
    while let Some(element) = stack.pop() {
        append_node(&mut roots, &mut stack, Node::Element(element));
    }
    roots
}

fn append_node(roots: &mut Vec<Node>, stack: &mut [Element], node: Node) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        roots.push(node);
    }
}

fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{MAX_HTML_ATTRIBUTES, enforce_raw_attribute_limit};

    #[test]
    fn raw_scan_counts_end_tag_attributes() {
        let attributes = html_attributes(MAX_HTML_ATTRIBUTES + 1);
        let source = format!("<p>text</p {attributes}>");

        let error = enforce_raw_attribute_limit(&source).expect_err("attribute budget");

        assert!(error.to_string().contains("total attribute budget"));
    }

    #[test]
    fn raw_scan_does_not_stop_at_less_than_inside_a_tag_name() {
        let attributes = html_attributes(MAX_HTML_ATTRIBUTES + 1);
        let source = format!("<p<1 {attributes}>text</p>");

        let error = enforce_raw_attribute_limit(&source).expect_err("attribute budget");

        assert!(error.to_string().contains("total attribute budget"));
    }

    #[test]
    fn raw_scan_counts_slash_separated_boolean_attributes() {
        let mut attributes = String::new();
        for index in 0..=MAX_HTML_ATTRIBUTES {
            write!(&mut attributes, "/a{index}").expect("attribute fixture");
        }
        let source = format!("<p a{attributes}>text</p>");

        let error = enforce_raw_attribute_limit(&source).expect_err("attribute budget");

        assert!(error.to_string().contains("total attribute budget"));
    }

    #[test]
    fn raw_scan_counts_vertical_tab_attribute_names() {
        assert!(!b'\x0b'.is_ascii_whitespace());
        let mut attributes = String::new();
        for width in 1..=MAX_HTML_ATTRIBUTES + 1 {
            for _ in 0..width {
                attributes.push('\u{000b}');
            }
            attributes.push(' ');
        }
        let source = format!("<p {attributes}>text</p>");

        let error = enforce_raw_attribute_limit(&source).expect_err("attribute budget");

        assert!(error.to_string().contains("total attribute budget"));
    }

    fn html_attributes(count: usize) -> String {
        let mut attributes = String::new();
        for index in 0..count {
            write!(&mut attributes, "a{index}=\"\"").expect("attribute fixture");
        }
        attributes
    }
}
