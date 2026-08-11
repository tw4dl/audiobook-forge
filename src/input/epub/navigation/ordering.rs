use std::collections::HashMap;

use crate::book::{Block, Section, SourcePosition, SourceRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ReadingPosition {
    spine_index: usize,
    character_offset: usize,
}

struct FlatSection {
    section: Section,
    ancestors: Vec<String>,
    ordinal: usize,
    position: Option<ReadingPosition>,
}

struct ArenaSection {
    section: Option<Section>,
    children: Vec<usize>,
}

pub(super) fn order_sections(
    root: &mut Section,
    spine_order: &HashMap<String, usize>,
    anchors: &HashMap<String, HashMap<String, Option<usize>>>,
) {
    let mut flat = Vec::new();
    let mut ordinal = 0_usize;
    for child in std::mem::take(&mut root.children) {
        flatten(child, &[], &mut ordinal, spine_order, anchors, &mut flat);
    }
    flat.sort_by_key(|item| {
        (
            item.position.unwrap_or(ReadingPosition {
                spine_index: usize::MAX,
                character_offset: usize::MAX,
            }),
            item.ordinal,
        )
    });

    let mut parents = vec![None; flat.len()];
    let mut stack = Vec::<usize>::new();
    for (index, item) in flat.iter().enumerate() {
        while stack.last().is_some_and(|candidate| {
            !item
                .ancestors
                .iter()
                .any(|ancestor| ancestor == &flat[*candidate].section.id)
        }) {
            stack.pop();
        }
        parents[index] = stack.last().copied();
        stack.push(index);
    }

    let mut arena = flat
        .into_iter()
        .map(|item| ArenaSection {
            section: Some(item.section),
            children: Vec::new(),
        })
        .collect::<Vec<_>>();
    let mut roots = Vec::new();
    for (index, parent) in parents.into_iter().enumerate() {
        if let Some(parent) = parent {
            arena[parent].children.push(index);
        } else {
            roots.push(index);
        }
    }
    root.children = roots
        .into_iter()
        .map(|index| materialize(index, 1, &mut arena))
        .collect();
}

fn flatten(
    mut section: Section,
    ancestors: &[String],
    ordinal: &mut usize,
    spine_order: &HashMap<String, usize>,
    anchors: &HashMap<String, HashMap<String, Option<usize>>>,
    flat: &mut Vec<FlatSection>,
) {
    let position = first_position(&section, spine_order, anchors);
    let children = std::mem::take(&mut section.children);
    let id = section.id.clone();
    flat.push(FlatSection {
        section,
        ancestors: ancestors.to_vec(),
        ordinal: *ordinal,
        position,
    });
    *ordinal = ordinal.saturating_add(1);

    let mut child_ancestors = ancestors.to_vec();
    child_ancestors.push(id);
    for child in children {
        flatten(child, &child_ancestors, ordinal, spine_order, anchors, flat);
    }
}

fn materialize(index: usize, level: u8, arena: &mut [ArenaSection]) -> Section {
    let children = std::mem::take(&mut arena[index].children);
    let mut section = arena[index]
        .section
        .take()
        .expect("each ordered section is materialized once");
    section.level = level;
    section.children = children
        .into_iter()
        .map(|child| materialize(child, level.saturating_add(1), arena))
        .collect();
    section
}

fn first_position(
    section: &Section,
    spine_order: &HashMap<String, usize>,
    anchors: &HashMap<String, HashMap<String, Option<usize>>>,
) -> Option<ReadingPosition> {
    let own = section
        .source_range
        .as_ref()
        .and_then(|range| range_position(range, spine_order, anchors));
    let block = section
        .blocks
        .iter()
        .filter_map(block_range)
        .filter_map(|range| range_position(range, spine_order, anchors))
        .min();
    let child = section
        .children
        .iter()
        .filter_map(|child| first_position(child, spine_order, anchors))
        .min();
    own.or(block).or(child)
}

fn range_position(
    range: &SourceRange,
    spine_order: &HashMap<String, usize>,
    anchors: &HashMap<String, HashMap<String, Option<usize>>>,
) -> Option<ReadingPosition> {
    let SourcePosition::Epub {
        resource,
        fragment,
        character_offset,
    } = &range.start
    else {
        return None;
    };
    let spine_index = spine_order.get(resource).copied()?;
    let character_offset = (*character_offset)
        .or_else(|| {
            fragment
                .as_ref()
                .and_then(|fragment| anchors.get(resource)?.get(fragment).copied().flatten())
        })
        .unwrap_or(0);
    Some(ReadingPosition {
        spine_index,
        character_offset,
    })
}

fn block_range(block: &Block) -> Option<&SourceRange> {
    match block {
        Block::Paragraph(block)
        | Block::Quote(block)
        | Block::Footnote(block)
        | Block::Aside(block)
        | Block::Navigation(block)
        | Block::Code(block) => block.source_range.as_ref(),
        Block::List(block) => block.source_range.as_ref(),
        Block::Figure(block) => block.source_range.as_ref(),
    }
}
