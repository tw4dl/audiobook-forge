use std::ops::Range;

use anyhow::{Context, Result};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::book::{
    Block, BookMetadata, CanonicalBook, ListBlock, Provenance, Section, SectionKind,
    SourceDocument, SourceFormat, SourceRange, TextBlock,
};

use super::{
    BookImporter, ImportSource, heading_kind, normalize_text, section_text, source_id,
    text_source_range, title_from_path,
};

pub(super) struct MarkdownImporter;

impl BookImporter for MarkdownImporter {
    fn import(&self, input: ImportSource) -> Result<CanonicalBook> {
        let (path, bytes) = input.into_parts();
        let source = String::from_utf8(bytes)
            .with_context(|| format!("failed to read UTF-8 Markdown from {}", path.display()))?;
        let title = title_from_path(&path);
        let mut builder = StructureBuilder::new(&title);
        let mut captures = Vec::<Capture>::new();
        let source_id = source_id(&path);

        for (event, range) in Parser::new_ext(&source, Options::empty()).into_offset_iter() {
            process_event(
                event,
                range,
                &source,
                &source_id,
                &mut captures,
                &mut builder,
            )?;
        }

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
                format: SourceFormat::Markdown,
                format_version: None,
            },
            text,
            pages: Vec::new(),
            warnings: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureKind {
    Heading(u8),
    Paragraph,
    Quote,
    Code,
    List(bool),
    Item,
    Html,
}

impl CaptureKind {
    fn from_tag(tag: &Tag<'_>) -> Option<Self> {
        match tag {
            Tag::Heading { level, .. } => Some(Self::Heading(heading_level(*level))),
            Tag::Paragraph => Some(Self::Paragraph),
            Tag::BlockQuote(_) => Some(Self::Quote),
            Tag::CodeBlock(_) => Some(Self::Code),
            Tag::List(start) => Some(Self::List(start.is_some())),
            Tag::Item => Some(Self::Item),
            Tag::HtmlBlock => Some(Self::Html),
            _ => None,
        }
    }

    fn ends_with(self, end: TagEnd) -> bool {
        match (self, end) {
            (Self::Heading(level), TagEnd::Heading(end_level)) => level == heading_level(end_level),
            (Self::Paragraph, TagEnd::Paragraph)
            | (Self::Quote, TagEnd::BlockQuote(_))
            | (Self::Code, TagEnd::CodeBlock)
            | (Self::Item, TagEnd::Item)
            | (Self::Html, TagEnd::HtmlBlock) => true,
            (Self::List(ordered), TagEnd::List(end_ordered)) => ordered == end_ordered,
            _ => false,
        }
    }
}

struct Capture {
    kind: CaptureKind,
    text: String,
    items: Vec<String>,
    start: usize,
    end: usize,
}

impl Capture {
    fn new(kind: CaptureKind, range: Range<usize>) -> Self {
        Self {
            kind,
            text: String::new(),
            items: Vec::new(),
            start: range.start,
            end: range.end,
        }
    }

    fn include(&mut self, range: &Range<usize>) {
        self.start = self.start.min(range.start);
        self.end = self.end.max(range.end);
    }

    fn push_text(&mut self, text: &str) {
        self.text.push_str(text);
    }

    fn push_separator(&mut self) {
        if self.kind == CaptureKind::Code {
            self.text.push('\n');
        } else if !self.text.ends_with(char::is_whitespace) {
            self.text.push(' ');
        }
    }

    fn source_range(&self, source: &str, source_id: &str) -> SourceRange {
        let mut end = self.end;
        while end > self.start && matches!(source.as_bytes()[end - 1], b'\r' | b'\n') {
            end -= 1;
        }
        text_source_range(source_id, self.start, end)
    }

    fn normalized_text(&self) -> String {
        if self.kind == CaptureKind::Code {
            self.text.trim().to_owned()
        } else {
            normalize_text(&self.text)
        }
    }
}

fn process_event(
    event: Event<'_>,
    range: Range<usize>,
    source: &str,
    source_id: &str,
    captures: &mut Vec<Capture>,
    builder: &mut StructureBuilder,
) -> Result<()> {
    match event {
        Event::Start(tag) => {
            if let Some(kind) = CaptureKind::from_tag(&tag) {
                captures.push(Capture::new(kind, range));
            } else if let Some(capture) = captures.last_mut() {
                capture.include(&range);
            }
        }
        Event::End(end) => {
            let Some(capture) = captures.last_mut() else {
                return Ok(());
            };
            capture.include(&range);
            if capture.kind.ends_with(end) {
                let capture = captures.pop().expect("capture stack has a last item");
                finish_capture(capture, source, source_id, captures, builder)?;
            }
        }
        Event::Text(text)
        | Event::Code(text)
        | Event::InlineMath(text)
        | Event::DisplayMath(text)
        | Event::FootnoteReference(text) => {
            if let Some(capture) = captures.last_mut() {
                capture.include(&range);
                capture.push_text(&text);
            }
        }
        Event::Html(html) => {
            if let Some(capture) = captures.last_mut() {
                capture.include(&range);
                capture.push_text(&html);
            }
        }
        Event::SoftBreak | Event::HardBreak => {
            if let Some(capture) = captures.last_mut() {
                capture.include(&range);
                capture.push_separator();
            }
        }
        Event::InlineHtml(_) | Event::Rule | Event::TaskListMarker(_) => {}
    }
    Ok(())
}

fn finish_capture(
    capture: Capture,
    source: &str,
    source_id: &str,
    parents: &mut [Capture],
    builder: &mut StructureBuilder,
) -> Result<()> {
    let source_range = capture.source_range(source, source_id);
    let mut text = capture.normalized_text();
    if capture.kind == CaptureKind::Html && !text.is_empty() {
        text = super::html::plain_text(&text).context("failed to parse embedded Markdown HTML")?;
    }

    if let Some(parent) = parents.last_mut() {
        match capture.kind {
            CaptureKind::Item
                if parent.kind == CaptureKind::List(false)
                    || parent.kind == CaptureKind::List(true) =>
            {
                if !text.is_empty() {
                    parent.items.push(text);
                }
            }
            CaptureKind::List(_) => append_segment(parent, &capture.items.join(". ")),
            _ => append_segment(parent, &text),
        }
        return Ok(());
    }

    match capture.kind {
        CaptureKind::Heading(level) if !text.is_empty() => {
            builder.push_heading(level, text, source_range);
        }
        CaptureKind::Paragraph | CaptureKind::Html | CaptureKind::Item if !text.is_empty() => {
            builder.push_block(Block::Paragraph(TextBlock {
                text,
                source_range: Some(source_range),
            }));
        }
        CaptureKind::Quote if !text.is_empty() => {
            builder.push_block(Block::Quote(TextBlock {
                text,
                source_range: Some(source_range),
            }));
        }
        CaptureKind::Code if !text.is_empty() => {
            builder.push_block(Block::Code(TextBlock {
                text,
                source_range: Some(source_range),
            }));
        }
        CaptureKind::List(ordered) if !capture.items.is_empty() => {
            builder.push_block(Block::List(ListBlock {
                ordered,
                text: capture.items.join(". "),
                items: capture.items,
                source_range: Some(source_range),
            }));
        }
        _ => {}
    }
    Ok(())
}

fn append_segment(capture: &mut Capture, text: &str) {
    if text.is_empty() {
        return;
    }
    if !capture.text.is_empty() && !capture.text.ends_with(char::is_whitespace) {
        capture.text.push(' ');
    }
    capture.text.push_str(text);
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
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

    fn push_heading(&mut self, level: u8, title: String, source_range: SourceRange) {
        self.close_sections(level);
        let mut section = Section::new(
            format!("section-{}", self.next_section_id),
            heading_kind(&title),
            Some(title),
            level,
            Provenance::Authored,
        );
        section.source_range = Some(source_range);
        self.stack.push(section);
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
