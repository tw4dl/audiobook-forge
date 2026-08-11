use anyhow::{Context, Result, bail};

use crate::input::html;

const MAX_TAG_BYTES: usize = 8 * 1_024;
const MAX_LINK_TEXT_BYTES: usize = 64 * 1_024;
const MAX_NAVIGATION_ENTRIES: usize = 10_000;

pub(super) struct PreparedMarkup {
    pub(super) markup: String,
    pub(super) warnings: Vec<String>,
}

pub(super) fn prepare(source: &str, file_version: u32) -> Result<PreparedMarkup> {
    if file_version >= 8 {
        let (markup, stripped) = strip_generated_inline_toc(source);
        let warnings = stripped
            .then(|| "Removed redundant generated KF8 table of contents from narration".to_owned())
            .into_iter()
            .collect();
        return Ok(PreparedMarkup { markup, warnings });
    }

    let Some(toc_target) = guide_toc_target(source)? else {
        return Ok(PreparedMarkup {
            markup: source.to_owned(),
            warnings: vec![
                "Legacy MOBI has no readable guide/TOC navigation; using HTML heading inference"
                    .to_owned(),
            ],
        });
    };
    let cutoff = block_start_before(source, toc_target).unwrap_or(toc_target.min(source.len()));
    let entries = navigation_entries(source, toc_target, cutoff)?;
    if entries.is_empty() {
        return Ok(PreparedMarkup {
            markup: source[..cutoff].to_owned(),
            warnings: vec![
                "Legacy MOBI table of contents has no usable filepos links; using HTML heading inference"
                    .to_owned(),
            ],
        });
    }
    let markup = inject_navigation_headings(&source[..cutoff], &entries);
    Ok(PreparedMarkup {
        markup,
        warnings: vec![
            "Restored legacy MOBI chapter structure from authored filepos navigation".to_owned(),
            "Removed redundant generated MOBI table of contents from narration".to_owned(),
        ],
    })
}

#[derive(Debug)]
struct NavigationEntry {
    target: usize,
    level: u8,
    title: String,
}

fn guide_toc_target(source: &str) -> Result<Option<usize>> {
    let mut cursor = 0_usize;
    while let Some(start) = find_ascii_case_insensitive(source, b"<reference", cursor) {
        let Some(end) = bounded_tag_end(source, start) else {
            bail!("legacy MOBI guide contains an unterminated reference tag");
        };
        let tag = &source[start..=end];
        if attribute(tag, "type").is_some_and(|value| value.eq_ignore_ascii_case("toc")) {
            return attribute(tag, "filepos")
                .map(parse_decimal_offset)
                .transpose();
        }
        cursor = end + 1;
    }
    Ok(None)
}

fn navigation_entries(source: &str, start: usize, cutoff: usize) -> Result<Vec<NavigationEntry>> {
    let mut entries = Vec::new();
    let mut cursor = start.min(source.len());
    let mut blockquote_depth = 0_usize;
    while let Some(tag_start) = source[cursor..].find('<').map(|offset| cursor + offset) {
        let Some(tag_end) = bounded_tag_end(source, tag_start) else {
            break;
        };
        let tag = &source[tag_start..=tag_end];
        if starts_with_tag(tag, "blockquote", true) {
            blockquote_depth = blockquote_depth.saturating_sub(1);
        } else if starts_with_tag(tag, "blockquote", false) {
            blockquote_depth = blockquote_depth.saturating_add(1);
        } else if starts_with_tag(tag, "a", false)
            && let Some(filepos) = attribute(tag, "filepos")
        {
            let target = parse_decimal_offset(filepos)?;
            let close = find_ascii_case_insensitive(source, b"</a", tag_end + 1)
                .filter(|close| close.saturating_sub(tag_end + 1) <= MAX_LINK_TEXT_BYTES);
            if let Some(close) = close {
                let title = html::plain_text(&source[tag_end + 1..close])?;
                let title = title.trim();
                if !title.is_empty() && target < cutoff {
                    if entries.len() >= MAX_NAVIGATION_ENTRIES {
                        bail!(
                            "legacy MOBI navigation contains more than {MAX_NAVIGATION_ENTRIES} entries"
                        );
                    }
                    entries.push(NavigationEntry {
                        target,
                        level: u8::try_from(blockquote_depth.saturating_add(1).min(6))
                            .expect("level is at most six"),
                        title: title.to_owned(),
                    });
                }
            }
        }
        cursor = tag_end + 1;
    }
    entries.sort_by_key(|entry| entry.target);
    entries.dedup_by(|left, right| left.target == right.target && left.title == right.title);
    Ok(entries)
}

fn inject_navigation_headings(source: &str, entries: &[NavigationEntry]) -> String {
    let mut insertions = entries
        .iter()
        .filter_map(|entry| {
            block_start_before(source, entry.target).map(|offset| {
                let title = escape_html(&entry.title);
                (
                    offset,
                    format!(
                        "<h{level} data-kokoro-mobi-nav=\"true\">{title}</h{level}>",
                        level = entry.level
                    ),
                )
            })
        })
        .collect::<Vec<_>>();
    insertions.sort_by_key(|insertion| std::cmp::Reverse(insertion.0));
    insertions.dedup_by(|left, right| left.0 == right.0);
    let added = insertions.iter().map(|(_, value)| value.len()).sum();
    let mut output = String::with_capacity(source.len().saturating_add(added));
    output.push_str(source);
    for (offset, heading) in insertions {
        output.insert_str(offset, &heading);
    }
    output
}

fn strip_generated_inline_toc(source: &str) -> (String, bool) {
    let markers = [
        b"id=\"calibre_generated_inline_toc\"".as_slice(),
        b"id='calibre_generated_inline_toc'".as_slice(),
    ];
    let marker = markers
        .iter()
        .filter_map(|marker| find_ascii_case_insensitive(source, marker, 0))
        .min();
    let Some(marker) = marker else {
        return (source.to_owned(), false);
    };
    let document_start = rfind_ascii_case_insensitive(source, b"<?xml", marker)
        .or_else(|| rfind_ascii_case_insensitive(source, b"<html", marker))
        .unwrap_or(marker);
    (source[..document_start].to_owned(), true)
}

fn block_start_before(source: &str, target: usize) -> Option<usize> {
    let target = target.min(source.len());
    [
        b"<mbp:pagebreak".as_slice(),
        b"<p".as_slice(),
        b"<div".as_slice(),
        b"<h1".as_slice(),
        b"<h2".as_slice(),
        b"<h3".as_slice(),
        b"<h4".as_slice(),
        b"<h5".as_slice(),
        b"<h6".as_slice(),
        b"<?xml".as_slice(),
        b"<html".as_slice(),
    ]
    .into_iter()
    .filter_map(|needle| rfind_ascii_case_insensitive(source, needle, target))
    .max()
}

fn bounded_tag_end(source: &str, start: usize) -> Option<usize> {
    let limit = start.saturating_add(MAX_TAG_BYTES).min(source.len());
    source
        .get(start..limit)?
        .find('>')
        .map(|offset| start + offset)
}

fn starts_with_tag(tag: &str, name: &str, closing: bool) -> bool {
    let bytes = tag.as_bytes();
    let prefix = if closing { 2 } else { 1 };
    bytes.get(..prefix)
        == Some(if closing {
            b"</".as_slice()
        } else {
            b"<".as_slice()
        })
        && bytes
            .get(prefix..prefix + name.len())
            .is_some_and(|value| value.eq_ignore_ascii_case(name.as_bytes()))
        && bytes
            .get(prefix + name.len())
            .is_none_or(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))
}

fn attribute<'a>(tag: &'a str, expected: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut cursor = 1_usize;
    while cursor < bytes.len() {
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'<' | b'/' | b'>'))
        {
            cursor += 1;
        }
        let name_start = cursor;
        while bytes
            .get(cursor)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'=' | b'/' | b'>'))
        {
            cursor += 1;
        }
        let name = bytes.get(name_start..cursor)?;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'=') {
            continue;
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let quote = bytes
            .get(cursor)
            .copied()
            .filter(|byte| matches!(byte, b'\'' | b'"'));
        if quote.is_some() {
            cursor += 1;
        }
        let value_start = cursor;
        while bytes.get(cursor).is_some_and(|byte| {
            quote.map_or_else(
                || !byte.is_ascii_whitespace() && !matches!(byte, b'/' | b'>'),
                |quote| *byte != quote,
            )
        }) {
            cursor += 1;
        }
        if name.eq_ignore_ascii_case(expected.as_bytes()) {
            return tag.get(value_start..cursor);
        }
        if quote.is_some() {
            cursor = cursor.saturating_add(1);
        }
    }
    None
}

fn parse_decimal_offset(value: &str) -> Result<usize> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("legacy MOBI filepos value {value:?} is not a decimal offset");
    }
    value
        .parse()
        .with_context(|| format!("legacy MOBI filepos value {value:?} overflows"))
}

fn escape_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
    output
}

fn find_ascii_case_insensitive(source: &str, needle: &[u8], start: usize) -> Option<usize> {
    source
        .as_bytes()
        .get(start..)?
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|offset| start + offset)
}

fn rfind_ascii_case_insensitive(source: &str, needle: &[u8], end: usize) -> Option<usize> {
    source
        .as_bytes()
        .get(..end.min(source.len()))?
        .windows(needle.len())
        .rposition(|window| window.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::{attribute, prepare};

    #[test]
    fn parses_quoted_and_unquoted_legacy_attributes() {
        assert_eq!(attribute("<a filepos=00042>", "filepos"), Some("00042"));
        assert_eq!(attribute("<a TYPE='toc'>", "type"), Some("toc"));
    }

    #[test]
    fn restores_legacy_headings_and_removes_the_inline_toc() {
        let source = "<html><head><guide><reference type=\"toc\" filepos=0000000000 /></guide></head><body><p><b>Chapter One</b></p><p>Text.</p><p>Table of Contents</p><a filepos=0000000000>Chapter One</a></body></html>";
        let chapter = source.find("<p><b>Chapter One").expect("chapter offset");
        let toc = source.find("<p>Table of Contents").expect("TOC offset");
        let source = source.replacen("filepos=0000000000", &format!("filepos={toc:010}"), 1);
        let source = source.replacen("filepos=0000000000", &format!("filepos={chapter:010}"), 1);
        let prepared = prepare(&source, 6).expect("valid legacy navigation");
        assert!(prepared.markup.contains("data-kokoro-mobi-nav"));
        assert!(!prepared.markup.contains("Table of Contents"));
    }
}
