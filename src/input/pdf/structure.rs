use crate::book::{Block, Provenance, Section, SectionKind, SourceRange, TextBlock};

use super::outline::OutlineItem;
use super::{PdfPage, empty_root, source_position};
use crate::input::heading_kind;
use crate::input::text::text_heading;

pub(super) struct InferredStructure {
    pub root: Section,
    pub heading_count: usize,
}

pub(super) fn from_outline(
    title: &str,
    source_id: &str,
    pages: &[PdfPage],
    items: &[OutlineItem],
) -> Section {
    let mut root = empty_root(title);
    let levels = normalized_levels(items);
    let mut blocks = vec![Vec::<Block>::new(); items.len()];
    for page in pages {
        let target = items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.page <= page.number)
            .max_by_key(|(index, item)| (item.page, item.level, *index))
            .map(|(index, _)| index);
        let (start, text) = target.map_or((0, page.text.as_str()), |index| {
            body_without_matching_heading(&page.text, &items[index].title)
        });
        if text.trim().is_empty() {
            continue;
        }
        let block = paragraph_block(
            source_id,
            page.number,
            start,
            page.text.chars().count(),
            text,
        );
        if let Some(index) = target {
            blocks[index].push(block);
        } else {
            root.blocks.push(block);
        }
    }
    let mut cursor = 0_usize;
    root.children = parse_outline_level(items, &levels, &mut blocks, &mut cursor, 1, source_id);
    root
}

fn normalized_levels(items: &[OutlineItem]) -> Vec<usize> {
    let mut levels = Vec::with_capacity(items.len());
    let mut previous = 0_usize;
    for item in items {
        let level = if levels.is_empty() {
            1
        } else {
            item.level.clamp(1, previous + 1)
        };
        levels.push(level);
        previous = level;
    }
    levels
}

fn parse_outline_level(
    items: &[OutlineItem],
    levels: &[usize],
    blocks: &mut [Vec<Block>],
    cursor: &mut usize,
    level: usize,
    source_id: &str,
) -> Vec<Section> {
    let mut sections = Vec::<Section>::new();
    while *cursor < items.len() {
        if levels[*cursor] < level {
            break;
        }
        if levels[*cursor] > level
            && let Some(parent) = sections.last_mut()
        {
            parent.children.extend(parse_outline_level(
                items,
                levels,
                blocks,
                cursor,
                level + 1,
                source_id,
            ));
            continue;
        }
        let index = *cursor;
        *cursor += 1;
        let item = &items[index];
        let mut section = Section::new(
            format!("pdf-outline-{}", index + 1),
            heading_kind(&item.title),
            Some(item.title.clone()),
            u8::try_from(level).unwrap_or(u8::MAX),
            Provenance::Authored,
        );
        let position = source_position(item.page, 0);
        section.source_range = Some(SourceRange {
            source_id: source_id.to_owned(),
            start: position.clone(),
            end: position,
        });
        section.blocks = std::mem::take(&mut blocks[index]);
        if *cursor < items.len() && levels[*cursor] > level {
            section.children =
                parse_outline_level(items, levels, blocks, cursor, level + 1, source_id);
        }
        sections.push(section);
    }
    sections
}

pub(super) fn infer(title: &str, source_id: &str, pages: &[PdfPage]) -> InferredStructure {
    let mut root = empty_root(title);
    let mut part = None;
    let mut active = None;
    let mut next_section_id = 1_usize;
    let mut heading_count = 0_usize;

    for page in pages {
        let mut paragraph = Vec::<String>::new();
        let mut paragraph_range = None;
        for (start, end, line) in character_lines(&page.text) {
            if let Some((kind, heading)) = text_heading(line) {
                flush_paragraph(
                    &mut root,
                    part.as_mut(),
                    active.as_mut(),
                    &mut paragraph,
                    &mut paragraph_range,
                    source_id,
                    page.number,
                );
                push_active(&mut root, part.as_mut(), &mut active);
                let level = if kind == SectionKind::Part {
                    if let Some(completed) = part.take() {
                        root.children.push(completed);
                    }
                    1
                } else {
                    u8::from(part.is_some()) + 1
                };
                let mut section = Section::new(
                    format!("pdf-inferred-{next_section_id}"),
                    kind,
                    Some(heading),
                    level,
                    Provenance::Inferred,
                );
                section.source_range = Some(pdf_range(source_id, page.number, start, end));
                if kind == SectionKind::Part {
                    part = Some(section);
                } else {
                    active = Some(section);
                }
                next_section_id += 1;
                heading_count += 1;
            } else if line.is_empty() {
                flush_paragraph(
                    &mut root,
                    part.as_mut(),
                    active.as_mut(),
                    &mut paragraph,
                    &mut paragraph_range,
                    source_id,
                    page.number,
                );
            } else {
                if let Some((_, range_end)) = paragraph_range.as_mut() {
                    *range_end = end;
                } else {
                    paragraph_range = Some((start, end));
                }
                paragraph.push(line.to_owned());
            }
        }
        flush_paragraph(
            &mut root,
            part.as_mut(),
            active.as_mut(),
            &mut paragraph,
            &mut paragraph_range,
            source_id,
            page.number,
        );
    }
    push_active(&mut root, part.as_mut(), &mut active);
    if let Some(completed) = part {
        root.children.push(completed);
    }
    if heading_count == 0 {
        let mut body = Section::new(
            "pdf-body",
            SectionKind::BodyMatter,
            Some("Body".to_owned()),
            1,
            Provenance::Derived,
        );
        body.blocks = std::mem::take(&mut root.blocks);
        root.children.push(body);
    }
    InferredStructure {
        root,
        heading_count,
    }
}

fn character_lines(text: &str) -> Vec<(usize, usize, &str)> {
    let mut lines = Vec::new();
    let mut byte_start = 0_usize;
    let mut character_start = 0_usize;
    for segment in text.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let characters = line.chars().count();
        lines.push((character_start, character_start + characters, line));
        byte_start += segment.len();
        character_start += segment.chars().count();
    }
    if byte_start < text.len() || text.is_empty() {
        lines.push((character_start, character_start, ""));
    }
    lines
}

fn flush_paragraph(
    root: &mut Section,
    part: Option<&mut Section>,
    active: Option<&mut Section>,
    paragraph: &mut Vec<String>,
    range: &mut Option<(usize, usize)>,
    source_id: &str,
    page: u32,
) {
    if paragraph.is_empty() {
        return;
    }
    let (start, end) = range.take().expect("non-empty PDF paragraph has a range");
    let block = paragraph_block(source_id, page, start, end, &paragraph.join(" "));
    if let Some(section) = active {
        section.blocks.push(block);
    } else if let Some(section) = part {
        section.blocks.push(block);
    } else {
        root.blocks.push(block);
    }
    paragraph.clear();
}

fn push_active(root: &mut Section, part: Option<&mut Section>, active: &mut Option<Section>) {
    let Some(section) = active.take() else {
        return;
    };
    if let Some(parent) = part {
        parent.children.push(section);
    } else {
        root.children.push(section);
    }
}

fn paragraph_block(source_id: &str, page: u32, start: usize, end: usize, text: &str) -> Block {
    Block::Paragraph(TextBlock {
        text: text.to_owned(),
        source_range: Some(pdf_range(source_id, page, start, end)),
    })
}

fn pdf_range(source_id: &str, page: u32, start: usize, end: usize) -> SourceRange {
    SourceRange {
        source_id: source_id.to_owned(),
        start: source_position(page, start),
        end: source_position(page, end),
    }
}

fn body_without_matching_heading<'a>(text: &'a str, title: &str) -> (usize, &'a str) {
    let Some((first, rest)) = text.split_once('\n') else {
        return if first_line_matches(text, title) {
            (text.chars().count(), "")
        } else {
            (0, text)
        };
    };
    if first_line_matches(first, title) {
        (first.chars().count() + 1, rest.trim_start_matches('\n'))
    } else {
        (0, text)
    }
}

fn first_line_matches(line: &str, title: &str) -> bool {
    line.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .eq_ignore_ascii_case(&title.split_whitespace().collect::<Vec<_>>().join(" "))
}
