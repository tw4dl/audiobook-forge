//! Semantic narration planning and speech normalization.

use crate::book::{Block, CanonicalBook, Section, SectionKind, SourcePosition, SourceRange};
use crate::pipeline::extract_sentences;
use crate::timeline::CueKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FootnoteMode {
    Inline,
    Skip,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NarrationPolicy {
    pub footnotes: FootnoteMode,
}

impl Default for NarrationPolicy {
    fn default() -> Self {
        Self {
            footnotes: FootnoteMode::Inline,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarrationUnit {
    pub id: String,
    pub text: String,
    pub section_id: Option<String>,
    pub source_range: Option<SourceRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarrationPlan {
    pub units: Vec<NarrationUnit>,
    pub warnings: Vec<String>,
    pub(crate) cues: Vec<PlannedCue>,
    pub(crate) pages: Vec<PlannedPage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedPage {
    pub(crate) label: String,
    pub(crate) position: SourcePosition,
    pub(crate) source_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedCue {
    pub(crate) id: String,
    pub(crate) kind: CueKind,
    pub(crate) unit_start: usize,
    pub(crate) unit_end: usize,
    pub(crate) source_range: Option<SourceRange>,
    pub(crate) section_id: Option<String>,
}

#[derive(Clone)]
struct DeferredNote {
    id: String,
    text: String,
    source_range: Option<SourceRange>,
    section_id: String,
}

/// Build provider-independent sentence units and semantic cue spans.
#[must_use]
pub fn plan_narration(book: &CanonicalBook, policy: NarrationPolicy) -> NarrationPlan {
    let mut builder = PlanBuilder {
        policy,
        units: Vec::new(),
        cues: Vec::new(),
        warnings: Vec::new(),
        deferred_notes: Vec::new(),
    };
    builder.visit_section(&book.root);
    for note in std::mem::take(&mut builder.deferred_notes) {
        builder.add_spoken_text(
            &note.id,
            &format!("Footnote. {}", note.text),
            Some(CueKind::Footnote),
            note.source_range,
            Some(note.section_id),
        );
    }
    NarrationPlan {
        units: builder.units,
        warnings: builder.warnings,
        cues: builder.cues,
        pages: book
            .pages
            .iter()
            .map(|page| PlannedPage {
                label: page.label.clone(),
                position: page.position.clone(),
                source_id: page_source_id(book, &page.position),
            })
            .collect(),
    }
}

fn page_source_id(book: &CanonicalBook, position: &SourcePosition) -> String {
    match position {
        SourcePosition::Epub { resource, .. } => resource.clone(),
        SourcePosition::Text { .. }
        | SourcePosition::Pdf { .. }
        | SourcePosition::Kindle { .. } => book.source.path.display().to_string(),
    }
}

struct PlanBuilder {
    policy: NarrationPolicy,
    units: Vec<NarrationUnit>,
    cues: Vec<PlannedCue>,
    warnings: Vec<String>,
    deferred_notes: Vec<DeferredNote>,
}

impl PlanBuilder {
    fn visit_section(&mut self, section: &Section) {
        if skip_section(section) {
            self.warnings.push(format!(
                "Skipped {} section {:?} by default narration policy",
                section_kind_label(section.kind),
                section.title.as_deref().unwrap_or("Untitled")
            ));
            return;
        }
        let unit_start = self.units.len();
        if !matches!(section.kind, SectionKind::Book | SectionKind::BodyMatter)
            && let Some(title) = section.title.as_deref()
        {
            self.add_spoken_text(
                &format!("{}:heading", section.id),
                title,
                None,
                section.source_range.clone(),
                Some(section.id.clone()),
            );
        }
        for (index, block) in section.blocks.iter().enumerate() {
            self.visit_block(section, block, index);
        }
        for child in &section.children {
            self.visit_section(child);
        }
        let unit_end = self.units.len();
        if unit_end > unit_start {
            self.cues.push(PlannedCue {
                id: format!("section:{}", section.id),
                kind: section_cue_kind(section.kind),
                unit_start,
                unit_end,
                source_range: section.source_range.clone(),
                section_id: Some(section.id.clone()),
            });
        }
    }

    fn visit_block(&mut self, section: &Section, block: &Block, index: usize) {
        let id = format!("{}:block:{}", section.id, index + 1);
        match block {
            Block::Paragraph(block) | Block::Quote(block) | Block::Aside(block) => {
                if !is_standalone_page_number(&block.text) {
                    self.add_spoken_text(
                        &id,
                        &block.text,
                        Some(CueKind::Paragraph),
                        block.source_range.clone(),
                        Some(section.id.clone()),
                    );
                }
            }
            Block::List(block) => {
                let text = block
                    .items
                    .iter()
                    .map(|item| sentence_terminated(item))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.add_spoken_text(
                    &id,
                    &text,
                    Some(CueKind::Paragraph),
                    block.source_range.clone(),
                    Some(section.id.clone()),
                );
            }
            Block::Figure(block) => {
                let text = block
                    .caption
                    .as_deref()
                    .or(block.alt_text.as_deref())
                    .unwrap_or_default();
                if !text.trim().is_empty() {
                    let text = if text.trim_start().to_ascii_lowercase().starts_with("figure") {
                        text.to_owned()
                    } else {
                        format!("Figure. {text}")
                    };
                    self.add_spoken_text(
                        &id,
                        &text,
                        Some(CueKind::Figure),
                        block.source_range.clone(),
                        Some(section.id.clone()),
                    );
                }
            }
            Block::Footnote(block) => match self.policy.footnotes {
                FootnoteMode::Inline => self.add_spoken_text(
                    &id,
                    &format!("Footnote. {}", block.text),
                    Some(CueKind::Footnote),
                    block.source_range.clone(),
                    Some(section.id.clone()),
                ),
                FootnoteMode::Skip => self
                    .warnings
                    .push(format!("Skipped footnote {id} by narration policy")),
                FootnoteMode::End => self.deferred_notes.push(DeferredNote {
                    id,
                    text: block.text.clone(),
                    source_range: block.source_range.clone(),
                    section_id: section.id.clone(),
                }),
            },
            Block::Navigation(_) => self
                .warnings
                .push(format!("Skipped source navigation block {id}")),
            Block::Code(_) => self
                .warnings
                .push(format!("Skipped code block {id}; source remains preserved")),
        }
    }

    fn add_spoken_text(
        &mut self,
        id: &str,
        raw: &str,
        parent_kind: Option<CueKind>,
        source_range: Option<SourceRange>,
        section_id: Option<String>,
    ) {
        let normalized = normalize_for_speech(raw);
        if normalized.is_empty() {
            return;
        }
        let include_sentence_cues = parent_kind.is_some();
        let unit_start = self.units.len();
        for (index, sentence) in extract_sentences(&normalized).into_iter().enumerate() {
            let sentence_id = format!("{id}:sentence:{}", index + 1);
            let sentence_start = self.units.len();
            self.units.push(NarrationUnit {
                id: sentence_id.clone(),
                text: sentence,
                section_id: section_id.clone(),
                source_range: source_range.clone(),
            });
            if include_sentence_cues {
                self.cues.push(PlannedCue {
                    id: sentence_id,
                    kind: CueKind::Sentence,
                    unit_start: sentence_start,
                    unit_end: sentence_start + 1,
                    source_range: source_range.clone(),
                    section_id: section_id.clone(),
                });
            }
        }
        let unit_end = self.units.len();
        if unit_end > unit_start
            && let Some(kind) = parent_kind
        {
            self.cues.push(PlannedCue {
                id: id.to_owned(),
                kind,
                unit_start,
                unit_end,
                source_range,
                section_id,
            });
        }
    }
}

/// Normalize extracted text into deterministic narration text.
#[must_use]
pub fn normalize_for_speech(raw: &str) -> String {
    let joined = raw
        .replace("-\r\n", "")
        .replace("-\n", "")
        .replace("-\r", "")
        .replace('\u{00ad}', "");
    let mut punctuation = String::with_capacity(joined.len());
    for character in joined.chars() {
        match character {
            '\u{00a0}' | '\u{2007}' | '\u{202f}' => punctuation.push(' '),
            '—' | '–' => punctuation.push_str(", "),
            '“' | '”' => punctuation.push('"'),
            '‘' | '’' => punctuation.push('\''),
            '…' => punctuation.push_str("..."),
            _ => punctuation.push(character),
        }
    }
    let citations_removed = remove_numeric_citations(&punctuation);
    citations_removed
        .split_whitespace()
        .map(normalize_token)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_token(token: &str) -> String {
    let leading_len = token
        .char_indices()
        .take_while(|(_, character)| matches!(character, '(' | '[' | '{' | '"' | '\''))
        .last()
        .map_or(0, |(index, character)| index + character.len_utf8());
    let (leading, rest) = token.split_at(leading_len);
    if !rest.starts_with("http://") && !rest.starts_with("https://") {
        return token.to_owned();
    }
    let core_end = rest
        .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\''])
        .len();
    let (url, trailing) = rest.split_at(core_end);
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .strip_prefix("www.")
        .unwrap_or(url);
    let mut spoken = String::new();
    for character in url.chars() {
        match character {
            '.' => spoken.push_str(" dot "),
            '/' => spoken.push_str(" slash "),
            '-' => spoken.push_str(" dash "),
            '_' => spoken.push_str(" underscore "),
            '?' => spoken.push_str(" question mark "),
            '&' => spoken.push_str(" and "),
            '=' => spoken.push_str(" equals "),
            '#' => spoken.push_str(" hash "),
            _ => spoken.push(character),
        }
    }
    format!(
        "{leading}{}{trailing}",
        spoken.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

fn remove_numeric_citations(text: &str) -> String {
    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0_usize;
    while index < characters.len() {
        if characters[index] == '['
            && let Some(end) = characters[index + 1..]
                .iter()
                .position(|character| *character == ']')
        {
            let end = index + 1 + end;
            let inside = &characters[index + 1..end];
            if !inside.is_empty()
                && inside.iter().all(|character| {
                    character.is_ascii_digit() || matches!(character, ',' | '-' | ' ')
                })
            {
                index = end + 1;
                continue;
            }
        }
        output.push(characters[index]);
        index += 1;
    }
    output
}

fn sentence_terminated(text: &str) -> String {
    let text = text.trim();
    if text.ends_with(['.', '!', '?', '…']) {
        text.to_owned()
    } else {
        format!("{text}.")
    }
}

fn is_standalone_page_number(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && text.chars().all(|character| character.is_ascii_digit())
}

fn skip_section(section: &Section) -> bool {
    if section.kind == SectionKind::Index {
        return true;
    }
    let title = section
        .title
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        title.trim(),
        "copyright" | "copyright page" | "table of contents" | "contents"
    )
}

fn section_cue_kind(kind: SectionKind) -> CueKind {
    match kind {
        SectionKind::Book => CueKind::Book,
        SectionKind::Part => CueKind::Part,
        SectionKind::Chapter => CueKind::Chapter,
        SectionKind::Section | SectionKind::Appendix | SectionKind::Notes => CueKind::Section,
        SectionKind::FrontMatter
        | SectionKind::BodyMatter
        | SectionKind::Bibliography
        | SectionKind::Index
        | SectionKind::BackMatter
        | SectionKind::Other => CueKind::Other,
    }
}

fn section_kind_label(kind: SectionKind) -> &'static str {
    match kind {
        SectionKind::Book => "book",
        SectionKind::FrontMatter => "front matter",
        SectionKind::BodyMatter => "body matter",
        SectionKind::Part => "part",
        SectionKind::Chapter => "chapter",
        SectionKind::Section => "section",
        SectionKind::Appendix => "appendix",
        SectionKind::Notes => "notes",
        SectionKind::Bibliography => "bibliography",
        SectionKind::Index => "index",
        SectionKind::BackMatter => "back matter",
        SectionKind::Other => "other",
    }
}
