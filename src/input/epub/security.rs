use std::borrow::Cow;
use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::Path;

use anyhow::{Context, Result, bail};
use quick_xml::Reader as XmlReader;
use quick_xml::events::Event as XmlEvent;
use zip8::ZipArchive;

use super::protection::{font_obfuscation_references, validate_font_references};

const MIB: u64 = 1_024 * 1_024;
const MAX_EPUB_ENTRIES: usize = 10_000;
const MAX_EXPANDED_ENTRY_BYTES: u64 = 32 * MIB;
const MAX_EXPANDED_TOTAL_BYTES: u64 = 512 * MIB;
const END_OF_CENTRAL_DIRECTORY_BYTES: usize = 22;
const MAX_ZIP_COMMENT_BYTES: usize = u16::MAX as usize;
const CENTRAL_DIRECTORY_HEADER_BYTES: usize = 46;
const MAX_MARKUP_DEPTH: usize = 128;

pub(super) fn validate_archive(bytes: &[u8], path: &Path) -> Result<()> {
    validate_declared_entry_count(bytes, path)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .with_context(|| format!("failed to open EPUB {}", path.display()))?;
    if archive.len() > MAX_EPUB_ENTRIES {
        bail!("EPUB contains more than {MAX_EPUB_ENTRIES} archive entries");
    }

    let mut declared_total = 0_u64;
    let mut actual_total = 0_u64;
    let mut archive_names = HashSet::with_capacity(archive.len());
    let mut container_document = None;
    let mut font_references = Vec::new();
    for index in 0..archive.len() {
        let (entry_name, is_encryption_manifest, is_container_document) = {
            let entry = archive
                .by_index_raw(index)
                .with_context(|| format!("failed to inspect EPUB archive entry {index}"))?;
            validate_entry_metadata(&entry)?;
            declared_total = declared_total
                .checked_add(entry.size())
                .context("EPUB expanded size overflow")?;
            if declared_total > MAX_EXPANDED_TOTAL_BYTES {
                bail!("EPUB exceeds 512 MiB cumulative expanded limit");
            }
            let entry_name = entry.name().to_owned();
            (
                entry_name.clone(),
                entry_name.eq_ignore_ascii_case("META-INF/encryption.xml"),
                entry_name.eq_ignore_ascii_case("META-INF/container.xml"),
            )
        };
        if !archive_names.insert(entry_name.clone()) {
            bail!("EPUB contains a duplicate archive path");
        }

        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("failed to open EPUB archive entry {index}"))?;
        let mut actual_entry_size = 0_u64;
        let mut buffer = [0_u8; 16 * 1_024];
        let mut expanded_bytes = Vec::new();
        loop {
            let read = entry
                .read(&mut buffer)
                .with_context(|| format!("failed to decompress EPUB archive entry {entry_name}"))?;
            if read == 0 {
                break;
            }
            let expanded = u64::try_from(read).context("EPUB expanded size overflow")?;
            actual_entry_size = actual_entry_size
                .checked_add(expanded)
                .context("EPUB expanded size overflow")?;
            actual_total = actual_total
                .checked_add(expanded)
                .context("EPUB expanded size overflow")?;
            if actual_entry_size > MAX_EXPANDED_ENTRY_BYTES {
                bail!("EPUB entry exceeds 32 MiB expanded limit");
            }
            if actual_total > MAX_EXPANDED_TOTAL_BYTES {
                bail!("EPUB exceeds 512 MiB cumulative expanded limit");
            }
            expanded_bytes.extend_from_slice(&buffer[..read]);
        }

        if is_markup_entry(&entry_name) || looks_like_markup(&expanded_bytes) {
            validate_markup_depth(&expanded_bytes, &entry_name)?;
        }

        if is_encryption_manifest {
            let references = font_obfuscation_references(&expanded_bytes)?;
            if font_references.len().saturating_add(references.len()) > MAX_EPUB_ENTRIES {
                bail!("EPUB encryption manifest contains too many resources");
            }
            font_references.extend(references);
        }
        if is_container_document && container_document.replace(expanded_bytes).is_some() {
            bail!("EPUB contains duplicate container metadata");
        }
    }

    if !font_references.is_empty() {
        let container = container_document
            .as_deref()
            .context("Unsupported encrypted/DRM-protected input.")?;
        validate_font_references(&mut archive, container, &archive_names, &font_references)?;
    }
    Ok(())
}

fn is_markup_entry(entry_name: &str) -> bool {
    let extension = entry_name.rsplit_once('.').map(|(_, extension)| extension);
    extension.is_some_and(|extension| {
        ["xhtml", "html", "htm", "ncx", "opf", "xml"]
            .iter()
            .any(|expected| extension.eq_ignore_ascii_case(expected))
    })
}

fn looks_like_markup(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"\xff\xfe") || bytes.starts_with(b"\xfe\xff") {
        return true;
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    bytes
        .iter()
        .copied()
        .find(|byte| !matches!(byte, b'\t' | b'\n' | b'\r' | b' '))
        == Some(b'<')
}

fn validate_markup_depth(markup: &[u8], entry_name: &str) -> Result<()> {
    let markup = markup_as_utf8(markup, entry_name)?;
    let mut reader = XmlReader::from_reader(markup.as_ref());
    let mut depth = 0_usize;
    loop {
        match reader.read_event() {
            Ok(XmlEvent::Start(_)) => {
                depth = depth.saturating_add(1);
                if depth > MAX_MARKUP_DEPTH {
                    bail!("EPUB markup {entry_name} nesting exceeds {MAX_MARKUP_DEPTH} levels");
                }
            }
            Ok(XmlEvent::End(_)) => depth = depth.saturating_sub(1),
            Ok(XmlEvent::Eof) | Err(_) => return Ok(()),
            Ok(_) => {}
        }
    }
}

fn markup_as_utf8<'a>(markup: &'a [u8], entry_name: &str) -> Result<Cow<'a, [u8]>> {
    let (little_endian, body) = if let Some(body) = markup.strip_prefix(b"\xff\xfe") {
        (true, body)
    } else if let Some(body) = markup.strip_prefix(b"\xfe\xff") {
        (false, body)
    } else {
        return Ok(Cow::Borrowed(markup));
    };
    let (chunks, remainder) = body.as_chunks::<2>();
    if !remainder.is_empty() {
        bail!("EPUB markup {entry_name} contains invalid UTF-16");
    }
    let units = chunks.iter().copied().map(|bytes| {
        if little_endian {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    });
    let decoded = char::decode_utf16(units)
        .collect::<std::result::Result<String, _>>()
        .with_context(|| format!("EPUB markup {entry_name} contains invalid UTF-16"))?;
    Ok(Cow::Owned(decoded.into_bytes()))
}

fn validate_entry_metadata<R: Read>(entry: &zip8::read::ZipFile<'_, R>) -> Result<()> {
    let entry_name = entry.name();
    let entry_name_bytes = entry.name_raw();
    let has_windows_drive_prefix = entry_name_bytes.len() >= 2
        && entry_name_bytes[0].is_ascii_alphabetic()
        && entry_name_bytes[1] == b':';
    let has_parent_component = entry_name
        .split(['/', '\\'])
        .any(|component| component == "..");
    if entry.enclosed_name().is_none()
        || entry_name_bytes.contains(&0)
        || entry_name.starts_with('/')
        || entry_name.starts_with('\\')
        || has_windows_drive_prefix
        || has_parent_component
    {
        bail!("EPUB contains an unsafe archive path");
    }
    if entry.encrypted() {
        bail!("Unsupported encrypted/DRM-protected input.");
    }
    if entry.size() > MAX_EXPANDED_ENTRY_BYTES {
        bail!("EPUB entry exceeds 32 MiB expanded limit");
    }
    Ok(())
}

fn validate_declared_entry_count(bytes: &[u8], path: &Path) -> Result<()> {
    let Some(last_start) = bytes.len().checked_sub(END_OF_CENTRAL_DIRECTORY_BYTES) else {
        bail!(
            "failed to open EPUB {}: ZIP footer is missing",
            path.display()
        );
    };
    let search_start = bytes
        .len()
        .saturating_sub(END_OF_CENTRAL_DIRECTORY_BYTES + MAX_ZIP_COMMENT_BYTES);

    for offset in (search_start..=last_start).rev() {
        if bytes.get(offset..offset + 4) != Some(b"PK\x05\x06") {
            continue;
        }
        let Some(comment_length) = read_u16(bytes, offset + 20).map(usize::from) else {
            continue;
        };
        if offset
            .checked_add(END_OF_CENTRAL_DIRECTORY_BYTES)
            .and_then(|end| end.checked_add(comment_length))
            != Some(bytes.len())
        {
            continue;
        }

        let Some(disk_number) = read_u16(bytes, offset + 4) else {
            continue;
        };
        let Some(directory_disk) = read_u16(bytes, offset + 6) else {
            continue;
        };
        let Some(entries_on_disk) = read_u16(bytes, offset + 8) else {
            continue;
        };
        let Some(entry_count) = read_u16(bytes, offset + 10) else {
            continue;
        };
        let Some(directory_size) = read_u32(bytes, offset + 12).map(u64::from) else {
            continue;
        };
        let Some(directory_start) = read_u32(bytes, offset + 16).map(u64::from) else {
            continue;
        };
        if disk_number != 0 || directory_disk != 0 || entries_on_disk != entry_count {
            bail!(
                "failed to open EPUB {}: multi-disk ZIP is unsupported",
                path.display()
            );
        }
        if usize::from(entry_count) > MAX_EPUB_ENTRIES {
            bail!("EPUB contains more than {MAX_EPUB_ENTRIES} archive entries");
        }
        let Ok(directory_start) = usize::try_from(directory_start) else {
            continue;
        };
        let Ok(directory_size) = usize::try_from(directory_size) else {
            continue;
        };
        if directory_start.checked_add(directory_size) != Some(offset) {
            continue;
        }
        if central_directory_matches(bytes, directory_start, offset, usize::from(entry_count)) {
            return Ok(());
        }
    }

    bail!(
        "failed to open EPUB {}: valid ZIP footer was not found",
        path.display()
    );
}

fn central_directory_matches(
    bytes: &[u8],
    mut cursor: usize,
    directory_end: usize,
    entry_count: usize,
) -> bool {
    for _ in 0..entry_count {
        let Some(fixed_end) = cursor.checked_add(CENTRAL_DIRECTORY_HEADER_BYTES) else {
            return false;
        };
        if fixed_end > directory_end || bytes.get(cursor..cursor + 4) != Some(b"PK\x01\x02") {
            return false;
        }
        let Some(name_length) = read_u16(bytes, cursor + 28).map(usize::from) else {
            return false;
        };
        let Some(extra_length) = read_u16(bytes, cursor + 30).map(usize::from) else {
            return false;
        };
        let Some(comment_length) = read_u16(bytes, cursor + 32).map(usize::from) else {
            return false;
        };
        let Some(next) = cursor
            .checked_add(CENTRAL_DIRECTORY_HEADER_BYTES)
            .and_then(|value| value.checked_add(name_length))
            .and_then(|value| value.checked_add(extra_length))
            .and_then(|value| value.checked_add(comment_length))
        else {
            return false;
        };
        if next > directory_end {
            return false;
        }
        cursor = next;
    }
    cursor == directory_end
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}
