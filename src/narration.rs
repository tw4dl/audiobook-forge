//! Semantic narration planning and speech normalization.

use std::collections::BTreeMap;

use serde::Serialize;

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
    pub original_text: String,
    pub text: String,
    pub section_id: Option<String>,
    pub source_range: Option<SourceRange>,
    pub normalization: TextNormalizationReport,
}

/// Deterministic text changes applied only to narration text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct TextNormalizationReport {
    pub count: usize,
    pub by_rule: BTreeMap<String, usize>,
    pub repairs: Vec<TextRepair>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct TextRepair {
    pub rule: String,
    pub from: String,
    pub to: String,
}

impl TextNormalizationReport {
    fn add(&mut self, rule: &str, count: usize) {
        if count == 0 {
            return;
        }
        self.count += count;
        *self.by_rule.entry(rule.to_owned()).or_default() += count;
    }

    fn record(&mut self, rule: &str, from: impl Into<String>, to: impl Into<String>) {
        self.add(rule, 1);
        self.repairs.push(TextRepair {
            rule: rule.to_owned(),
            from: from.into(),
            to: to.into(),
        });
    }
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
        let (normalized, text_normalization) = normalize_for_speech_with_report(raw);
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
                original_text: raw.to_owned(),
                text: sentence,
                section_id: section_id.clone(),
                source_range: source_range.clone(),
                normalization: if index == 0 {
                    text_normalization.clone()
                } else {
                    TextNormalizationReport::default()
                },
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
    normalize_for_speech_with_report(raw).0
}

/// Normalize narration text and record every deterministic text repair.
#[must_use]
pub fn normalize_for_speech_with_report(raw: &str) -> (String, TextNormalizationReport) {
    let mut report = TextNormalizationReport::default();
    let mut joined = raw.to_owned();
    for (needle, rule) in [
        ("-\r\n", "soft_hyphen_line_join"),
        ("-\n", "soft_hyphen_line_join"),
        ("-\r", "soft_hyphen_line_join"),
    ] {
        let count = joined.matches(needle).count();
        for _ in 0..count {
            report.record(rule, needle, "");
        }
        joined = joined.replace(needle, "");
    }
    let soft_hyphens = joined.matches('\u{00ad}').count();
    for _ in 0..soft_hyphens {
        report.record("soft_hyphen", "\u{00ad}", "");
    }
    joined = joined.replace('\u{00ad}', "");
    let joined = expand_currency_symbols(&joined, &mut report);
    let mut punctuation = String::with_capacity(joined.len());
    for character in joined.chars() {
        match character {
            '\u{00a0}' | '\u{2007}' | '\u{202f}' => {
                report.record("non_breaking_space", character.to_string(), " ");
                punctuation.push(' ');
            }
            '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' => {
                report.record("zero_width", character.to_string(), "");
            }
            '—' | '–' => {
                report.record("dash", character.to_string(), ", ");
                punctuation.push_str(", ");
            }
            '“' | '”' => {
                report.record("curly_quote", character.to_string(), "\"");
                punctuation.push('"');
            }
            '‘' | '’' => {
                report.record("curly_apostrophe", character.to_string(), "'");
                punctuation.push('\'');
            }
            '…' => {
                report.record("ellipsis", "…", "...");
                punctuation.push_str("...");
            }
            '$' => {
                report.record("currency_expansion", "$", " dollars ");
                punctuation.push_str(" dollars ");
            }
            '×' => {
                report.record("multiply_expansion", "×", " times ");
                punctuation.push_str(" times ");
            }
            '=' => {
                report.record("equals_expansion", "=", " equals ");
                punctuation.push_str(" equals ");
            }
            '©' => {
                report.record("copyright_expansion", "©", " copyright ");
                punctuation.push_str(" copyright ");
            }
            _ => punctuation.push(character),
        }
    }
    let (punctuation, spaced_ellipses) = collapse_spaced_ellipses(&punctuation);
    for _ in 0..spaced_ellipses {
        report.record("spaced_ellipsis", ". . .", "...");
    }
    let (citations_removed, citations) = remove_numeric_citations(&punctuation);
    for _ in 0..citations {
        report.record("numeric_citation", "[number]", "");
    }
    let mut normalized_tokens = Vec::new();
    for token in citations_removed.split_whitespace() {
        let normalized = normalize_token(token);
        if normalized != token && is_url_token(token) {
            report.record("url", token, &normalized);
        }
        if !normalized.is_empty() {
            normalized_tokens.push(normalized);
        }
    }
    (normalized_tokens.join(" "), report)
}

fn expand_currency_symbols(text: &str, report: &mut TextNormalizationReport) -> String {
    let characters = text.char_indices().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len());
    let mut index = 0_usize;
    while index < characters.len() {
        let (byte_index, character) = characters[index];
        if character == '$' && index + 1 < characters.len() {
            let mut end = index + 1;
            while end < characters.len()
                && (characters[end].1.is_ascii_digit() || matches!(characters[end].1, ',' | '.'))
            {
                end += 1;
            }
            if end > index + 1 {
                let end_byte = characters.get(end).map_or(text.len(), |(byte, _)| *byte);
                let from = &text[byte_index..end_byte];
                let number = &text[byte_index + character.len_utf8()..end_byte];
                let to = format!("{number} dollars");
                report.record("currency_expansion", from, &to);
                output.push_str(&to);
                index = end;
                continue;
            }
        }
        output.push(character);
        index += 1;
    }
    output
}

fn collapse_spaced_ellipses(text: &str) -> (String, usize) {
    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len());
    let mut index = 0_usize;
    let mut count = 0_usize;
    while index < characters.len() {
        if characters[index] == '.' {
            let mut cursor = index + 1;
            while cursor < characters.len() && characters[cursor].is_whitespace() {
                cursor += 1;
            }
            if cursor < characters.len() && characters[cursor] == '.' {
                cursor += 1;
                while cursor < characters.len() && characters[cursor].is_whitespace() {
                    cursor += 1;
                }
                if cursor < characters.len() && characters[cursor] == '.' {
                    output.push_str("...");
                    count += 1;
                    index = cursor + 1;
                    continue;
                }
            }
        }
        output.push(characters[index]);
        index += 1;
    }
    (output, count)
}

fn is_url_token(token: &str) -> bool {
    let trimmed = token.trim_matches(['(', '[', '{', '"', '\'']);
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
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

fn remove_numeric_citations(text: &str) -> (String, usize) {
    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0_usize;
    let mut count = 0_usize;
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
                count += 1;
                continue;
            }
        }
        output.push(characters[index]);
        index += 1;
    }
    (output, count)
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
    if matches!(section.kind, SectionKind::Index | SectionKind::Notes) {
        return true;
    }
    let title = section
        .title
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let title = title.trim();
    title == "copyright"
        || title == "copyright page"
        || title == "table of contents"
        || title == "contents"
        || title == "notes"
        || title == "endnotes"
        || title == "index"
        || title == "credits"
        || title.ends_with(" credits")
        || title == "connect with hmh"
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
