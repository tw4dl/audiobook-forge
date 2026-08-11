use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use lopdf::{Dictionary, Document, Object, ObjectId};

const MAX_LABEL_TREE_NODES: usize = 10_000;
const MAX_LABEL_TREE_DEPTH: usize = 128;

#[derive(Debug)]
struct LabelRange {
    start_index: usize,
    prefix: String,
    style: Option<Vec<u8>>,
    start_number: u32,
}

pub(super) fn read_page_labels(
    document: &Document,
    page_count: usize,
) -> Result<Option<Vec<String>>> {
    let catalog = document.catalog().context("PDF catalog is missing")?;
    let Ok(root) = catalog.get(b"PageLabels") else {
        return Ok(None);
    };
    let mut ranges = collect_ranges(document, root.clone(), page_count)?;
    if ranges.is_empty() {
        return Ok(None);
    }
    ranges.sort_by_key(|range| range.start_index);
    if ranges[0].start_index != 0 {
        ranges.insert(
            0,
            LabelRange {
                start_index: 0,
                prefix: String::new(),
                style: Some(b"D".to_vec()),
                start_number: 1,
            },
        );
    }
    if ranges
        .windows(2)
        .any(|pair| pair[0].start_index == pair[1].start_index)
    {
        bail!("duplicate page-label range index");
    }

    let mut labels = Vec::with_capacity(page_count);
    let mut range_index = 0_usize;
    for page_index in 0..page_count {
        while range_index + 1 < ranges.len() && ranges[range_index + 1].start_index <= page_index {
            range_index += 1;
        }
        let range = &ranges[range_index];
        let offset = u32::try_from(page_index - range.start_index)
            .context("PDF page-label index is too large")?;
        let number = range
            .start_number
            .checked_add(offset)
            .context("PDF page-label number overflow")?;
        let suffix = format_number(range.style.as_deref(), number)?;
        labels.push(format!("{}{}", range.prefix, suffix));
    }
    Ok(Some(labels))
}

fn collect_ranges(document: &Document, root: Object, page_count: usize) -> Result<Vec<LabelRange>> {
    let mut stack = vec![(root, 1_usize)];
    let mut visited = HashSet::<ObjectId>::new();
    let mut nodes = 0_usize;
    let mut ranges = Vec::new();
    while let Some((object, depth)) = stack.pop() {
        if depth > MAX_LABEL_TREE_DEPTH {
            bail!("page-label tree exceeds {MAX_LABEL_TREE_DEPTH} levels");
        }
        if let Object::Reference(id) = object
            && !visited.insert(id)
        {
            bail!("page-label tree contains a reference cycle");
        }
        nodes += 1;
        if nodes > MAX_LABEL_TREE_NODES {
            bail!("page-label tree contains more than {MAX_LABEL_TREE_NODES} nodes");
        }
        let dictionary = resolve_dictionary(document, &object)
            .context("page-label tree node is not a dictionary")?;
        if let Ok(kids) = dictionary.get(b"Kids").and_then(Object::as_array) {
            for kid in kids.iter().rev() {
                stack.push((kid.clone(), depth + 1));
            }
        }
        if let Ok(numbers) = dictionary.get(b"Nums").and_then(Object::as_array) {
            let (pairs, remainder) = numbers.as_chunks::<2>();
            for pair in pairs {
                let index = pair[0]
                    .as_i64()
                    .ok()
                    .and_then(|value| usize::try_from(value).ok())
                    .context("page-label index is invalid")?;
                if index >= page_count {
                    bail!("page-label index {index} is outside the document");
                }
                ranges.push(read_range(document, index, &pair[1])?);
            }
            if !remainder.is_empty() {
                bail!("page-label number array has an unmatched value");
            }
        }
    }
    Ok(ranges)
}

fn read_range(document: &Document, start_index: usize, object: &Object) -> Result<LabelRange> {
    let dictionary =
        resolve_dictionary(document, object).context("page-label range is not a dictionary")?;
    let prefix = dictionary
        .get(b"P")
        .ok()
        .and_then(|value| resolve_object(document, value).ok())
        .and_then(|value| lopdf::decode_text_string(value).ok())
        .unwrap_or_default();
    let style = dictionary
        .get(b"S")
        .ok()
        .map(|value| resolve_object(document, value))
        .transpose()?
        .map(Object::as_name)
        .transpose()?
        .map(<[u8]>::to_vec);
    let start_number = dictionary
        .get(b"St")
        .ok()
        .map(|value| resolve_object(document, value))
        .transpose()?
        .map(Object::as_i64)
        .transpose()?
        .map(u32::try_from)
        .transpose()
        .context("page-label start number is invalid")?
        .unwrap_or(1);
    if start_number == 0 {
        bail!("page-label start number must be positive");
    }
    Ok(LabelRange {
        start_index,
        prefix,
        style,
        start_number,
    })
}

fn format_number(style: Option<&[u8]>, number: u32) -> Result<String> {
    match style {
        None => Ok(String::new()),
        Some(b"D") => Ok(number.to_string()),
        Some(b"R") => Ok(roman(number).to_ascii_uppercase()),
        Some(b"r") => Ok(roman(number)),
        Some(b"A") => Ok(alphabetic(number).to_ascii_uppercase()),
        Some(b"a") => Ok(alphabetic(number)),
        Some(other) => bail!(
            "unsupported page-label numbering style /{}",
            String::from_utf8_lossy(other)
        ),
    }
}

fn roman(mut number: u32) -> String {
    let mut output = String::new();
    for (value, symbol) in [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ] {
        while number >= value {
            output.push_str(symbol);
            number -= value;
        }
    }
    output
}

fn alphabetic(mut number: u32) -> String {
    let mut reversed = Vec::new();
    while number > 0 {
        number -= 1;
        reversed.push(char::from(
            b'a' + u8::try_from(number % 26).expect("alphabet index"),
        ));
        number /= 26;
    }
    reversed.into_iter().rev().collect()
}

fn resolve_dictionary<'a>(
    document: &'a Document,
    object: &'a Object,
) -> lopdf::Result<&'a Dictionary> {
    resolve_object(document, object)?.as_dict()
}

fn resolve_object<'a>(document: &'a Document, object: &'a Object) -> lopdf::Result<&'a Object> {
    match object {
        Object::Reference(id) => document.get_object(*id),
        _ => Ok(object),
    }
}
