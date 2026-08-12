use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use lopdf::{Dictionary, Document, Error as PdfError, Object, ObjectId};

const MAX_OUTLINE_ENTRIES: usize = 10_000;
const MAX_OUTLINE_DEPTH: usize = 128;

#[derive(Debug)]
pub(super) struct OutlineItem {
    pub title: String,
    pub level: usize,
    pub page: u32,
}

pub(super) enum OutlineRead {
    Found {
        items: Vec<OutlineItem>,
        warnings: Vec<String>,
    },
    Missing {
        warning: Option<String>,
    },
}

pub(super) fn read_outline(document: &Document, page_count: usize) -> Result<OutlineRead> {
    if !preflight_outline(document)? {
        return Ok(OutlineRead::Missing { warning: None });
    }
    preflight_named_destinations(document)?;
    let toc = match document.get_toc() {
        Ok(toc) => toc,
        Err(PdfError::NoOutline) => return Ok(OutlineRead::Missing { warning: None }),
        Err(error) => {
            return Ok(OutlineRead::Missing {
                warning: Some(format!(
                    "PDF outline could not be read; using deterministic fallback: {error}"
                )),
            });
        }
    };
    let mut warnings = toc
        .errors
        .into_iter()
        .map(|error| format!("PDF outline warning: {error}"))
        .collect::<Vec<_>>();
    let mut items = Vec::new();
    for entry in toc.toc {
        let title = entry.title.split_whitespace().collect::<Vec<_>>().join(" ");
        let Ok(page) = u32::try_from(entry.page) else {
            warnings.push("PDF outline entry has an invalid page number; skipped".to_owned());
            continue;
        };
        if title.is_empty() || page == 0 || entry.page > page_count {
            warnings.push(format!(
                "PDF outline entry {title:?} points outside the document; skipped"
            ));
            continue;
        }
        items.push(OutlineItem {
            title,
            level: entry.level.clamp(1, MAX_OUTLINE_DEPTH),
            page,
        });
    }
    if items.is_empty() {
        Ok(OutlineRead::Missing {
            warning: Some(
                "PDF outline contains no usable destinations; using deterministic fallback"
                    .to_owned(),
            ),
        })
    } else {
        Ok(OutlineRead::Found { items, warnings })
    }
}

fn preflight_outline(document: &Document) -> Result<bool> {
    let catalog = document.catalog().context("PDF catalog is missing")?;
    let Ok(outlines) = catalog.get(b"Outlines") else {
        return Ok(false);
    };
    let root = resolve_dictionary(document, outlines).context("PDF outline root is malformed")?;
    let Ok(first) = root.get(b"First") else {
        return Ok(false);
    };
    walk_linked_tree(document, first.clone(), "outline")?;
    Ok(true)
}

fn walk_linked_tree(document: &Document, first: Object, label: &str) -> Result<()> {
    let mut stack = vec![(first, 1_usize)];
    let mut visited = HashSet::<ObjectId>::new();
    let mut count = 0_usize;
    while let Some((object, depth)) = stack.pop() {
        if depth > MAX_OUTLINE_DEPTH {
            bail!("PDF {label} nesting exceeds {MAX_OUTLINE_DEPTH} levels");
        }
        if let Object::Reference(id) = object
            && !visited.insert(id)
        {
            bail!("PDF {label} contains a reference cycle");
        }
        count += 1;
        if count > MAX_OUTLINE_ENTRIES {
            bail!("PDF {label} contains more than {MAX_OUTLINE_ENTRIES} entries");
        }
        let dictionary = resolve_dictionary(document, &object)
            .with_context(|| format!("PDF {label} entry is malformed"))?;
        if let Ok(next) = dictionary.get(b"Next") {
            stack.push((next.clone(), depth));
        }
        if let Ok(child) = dictionary.get(b"First") {
            stack.push((child.clone(), depth + 1));
        }
    }
    Ok(())
}

fn preflight_named_destinations(document: &Document) -> Result<()> {
    let catalog = document.catalog().context("PDF catalog is missing")?;
    let tree = if let Ok(tree) = catalog.get(b"Dests") {
        Some(tree)
    } else if let Ok(names) = catalog.get(b"Names") {
        resolve_dictionary(document, names)
            .ok()
            .and_then(|dictionary| dictionary.get(b"Dests").ok())
    } else {
        None
    };
    let Some(tree) = tree else {
        return Ok(());
    };
    let mut stack = vec![(tree.clone(), 1_usize)];
    let mut visited = HashSet::<ObjectId>::new();
    let mut count = 0_usize;
    while let Some((object, depth)) = stack.pop() {
        if depth > MAX_OUTLINE_DEPTH {
            bail!("PDF named destination nesting exceeds {MAX_OUTLINE_DEPTH} levels");
        }
        if let Object::Reference(id) = object
            && !visited.insert(id)
        {
            bail!("PDF named destinations contain a reference cycle");
        }
        count += 1;
        if count > MAX_OUTLINE_ENTRIES {
            bail!("PDF named destinations contain more than {MAX_OUTLINE_ENTRIES} nodes");
        }
        let dictionary = resolve_dictionary(document, &object)
            .context("PDF named destination tree is malformed")?;
        if let Ok(kids) = dictionary.get(b"Kids").and_then(Object::as_array) {
            for child in kids.iter().rev() {
                stack.push((child.clone(), depth + 1));
            }
        }
    }
    Ok(())
}

fn resolve_dictionary<'a>(
    document: &'a Document,
    object: &'a Object,
) -> lopdf::Result<&'a Dictionary> {
    match object {
        Object::Reference(id) => document.get_dictionary(*id),
        Object::Dictionary(dictionary) => Ok(dictionary),
        _ => object.as_dict(),
    }
}
