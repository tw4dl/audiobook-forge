use std::path::Path;

use anyhow::{Context, Result};

use crate::book::{
    Block, BookMetadata, CanonicalBook, Provenance, Section, SectionKind, SourceDocument,
    SourceFormat, TextBlock,
};

use super::{
    BookImporter, ImportSource, section_text, source_id, source_lines, text_source_range,
    title_from_path,
};

pub(super) struct TextImporter;

impl BookImporter for TextImporter {
    fn import(&self, source: ImportSource) -> Result<CanonicalBook> {
        let (path, bytes) = source.into_parts();
        let text = String::from_utf8(bytes)
            .with_context(|| format!("failed to read UTF-8 text from {}", path.display()))?;
        let title = title_from_path(&path);
        let mut root = Section::new(
            "book",
            SectionKind::Book,
            Some(title.clone()),
            0,
            Provenance::Derived,
        );
        let mut part = None;
        let mut active = None;
        let mut paragraph = Vec::new();
        let mut paragraph_range = None;
        let mut next_section_id = 1_usize;
        let source_id = source_id(&path);

        for (line_start, line_end, line) in source_lines(&text) {
            let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
            if let Some((kind, heading)) = text_heading(&line) {
                flush_paragraph(
                    &mut root,
                    part.as_mut(),
                    active.as_mut(),
                    &mut paragraph,
                    &mut paragraph_range,
                    &source_id,
                );
                push_active_section(&mut root, part.as_mut(), &mut active);
                let level = if kind == SectionKind::Part {
                    if let Some(completed_part) = part.take() {
                        root.children.push(completed_part);
                    }
                    1
                } else {
                    u8::from(part.is_some()) + 1
                };
                let mut section = Section::new(
                    format!("section-{next_section_id}"),
                    kind,
                    Some(heading),
                    level,
                    Provenance::Inferred,
                );
                section.source_range = Some(text_source_range(&source_id, line_start, line_end));
                if kind == SectionKind::Part {
                    part = Some(section);
                } else {
                    active = Some(section);
                }
                next_section_id += 1;
            } else if line.is_empty() {
                flush_paragraph(
                    &mut root,
                    part.as_mut(),
                    active.as_mut(),
                    &mut paragraph,
                    &mut paragraph_range,
                    &source_id,
                );
            } else {
                if paragraph_range.is_none() {
                    paragraph_range = Some((line_start, line_end));
                } else if let Some((_, end)) = paragraph_range.as_mut() {
                    *end = line_end;
                }
                paragraph.push(line);
            }
        }
        flush_paragraph(
            &mut root,
            part.as_mut(),
            active.as_mut(),
            &mut paragraph,
            &mut paragraph_range,
            &source_id,
        );
        push_active_section(&mut root, part.as_mut(), &mut active);
        if let Some(completed_part) = part {
            root.children.push(completed_part);
        }

        Ok(finish_book(&path, title, root))
    }
}

fn finish_book(path: &Path, title: String, mut root: Section) -> CanonicalBook {
    let unstructured = root.children.is_empty();
    if unstructured {
        let mut body = Section::new(
            "section-1",
            SectionKind::BodyMatter,
            Some("Body".to_owned()),
            1,
            Provenance::Derived,
        );
        body.blocks = std::mem::take(&mut root.blocks);
        root.children.push(body);
    }
    let text = section_text(&root);
    let warnings = if unstructured {
        vec![
            "TXT has no detected semantic sections; treating the document as one section"
                .to_owned(),
        ]
    } else {
        Vec::new()
    };
    CanonicalBook {
        metadata: BookMetadata {
            title: Some(title),
            ..BookMetadata::default()
        },
        root,
        source: SourceDocument {
            path: path.to_path_buf(),
            format: SourceFormat::Text,
            format_version: None,
        },
        text,
        pages: Vec::new(),
        warnings,
    }
}

fn text_heading(line: &str) -> Option<(SectionKind, String)> {
    if line.chars().count() > 120 {
        return None;
    }
    let words = line.split_whitespace().collect::<Vec<_>>();
    let first = *words.first()?;
    let kind = if first.eq_ignore_ascii_case("chapter") {
        SectionKind::Chapter
    } else if first.eq_ignore_ascii_case("part") || first.eq_ignore_ascii_case("book") {
        SectionKind::Part
    } else {
        return None;
    };
    let identifier = words
        .get(1)?
        .trim_matches(|character| matches!(character, ':' | '.' | '-' | '\u{2013}' | '\u{2014}'));
    if !is_heading_identifier(identifier) {
        return None;
    }
    let all_uppercase = line
        .chars()
        .filter(|character| character.is_alphabetic())
        .all(char::is_uppercase);
    let has_title_separator = words.get(1).is_some_and(|word| word.ends_with(':'))
        || words
            .get(2)
            .is_some_and(|word| matches!(*word, ":" | "-" | "\u{2013}" | "\u{2014}"));
    if words.len() > 2 && !all_uppercase && !has_title_separator {
        return None;
    }
    Some((kind, display_heading(line)))
}

fn is_heading_identifier(identifier: &str) -> bool {
    if identifier.is_empty() {
        return false;
    }
    if identifier
        .chars()
        .all(|character| character.is_ascii_digit())
    {
        return true;
    }
    let uppercase = identifier.to_ascii_uppercase();
    if uppercase
        .chars()
        .all(|character| matches!(character, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'))
    {
        return true;
    }
    matches!(
        uppercase.as_str(),
        "ONE"
            | "TWO"
            | "THREE"
            | "FOUR"
            | "FIVE"
            | "SIX"
            | "SEVEN"
            | "EIGHT"
            | "NINE"
            | "TEN"
            | "ELEVEN"
            | "TWELVE"
            | "THIRTEEN"
            | "FOURTEEN"
            | "FIFTEEN"
            | "SIXTEEN"
            | "SEVENTEEN"
            | "EIGHTEEN"
            | "NINETEEN"
            | "TWENTY"
            | "THIRTY"
            | "FORTY"
            | "FIFTY"
            | "SIXTY"
            | "SEVENTY"
            | "EIGHTY"
            | "NINETY"
    )
}

fn display_heading(line: &str) -> String {
    if line.chars().any(char::is_lowercase) {
        return line.to_owned();
    }
    line.split_whitespace()
        .map(|word| {
            if word.chars().all(|character| {
                !character.is_alphabetic() || matches!(character, 'I' | 'V' | 'X' | 'L' | 'C')
            }) {
                return word.to_owned();
            }
            let mut characters = word.chars();
            let Some(first) = characters.next() else {
                return String::new();
            };
            first
                .to_uppercase()
                .chain(characters.flat_map(char::to_lowercase))
                .collect()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn flush_paragraph(
    root: &mut Section,
    part: Option<&mut Section>,
    active: Option<&mut Section>,
    paragraph: &mut Vec<String>,
    paragraph_range: &mut Option<(usize, usize)>,
    source_id: &str,
) {
    if paragraph.is_empty() {
        return;
    }
    let block = Block::Paragraph(TextBlock {
        text: paragraph.join(" "),
        source_range: paragraph_range
            .take()
            .map(|(start, end)| text_source_range(source_id, start, end)),
    });
    if let Some(section) = active {
        section.blocks.push(block);
    } else if let Some(part) = part {
        part.blocks.push(block);
    } else {
        root.blocks.push(block);
    }
    paragraph.clear();
}

fn push_active_section(
    root: &mut Section,
    part: Option<&mut Section>,
    active: &mut Option<Section>,
) {
    let Some(section) = active.take() else {
        return;
    };
    if let Some(part) = part {
        part.children.push(section);
    } else {
        root.children.push(section);
    }
}
