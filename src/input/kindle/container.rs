use std::path::Path;

use anyhow::{Context, Result, bail};
use encoding_rs::WINDOWS_1252;

use crate::book::{BookAsset, BookMetadata, SourceFormat};

use super::palmdoc;

const PDB_HEADER_BYTES: usize = 78;
const PDB_RECORD_ENTRY_BYTES: usize = 8;
const PALMDOC_HEADER_BYTES: usize = 16;
const MAX_RECORDS: usize = 50_000;
const MAX_EXTH_RECORDS: usize = 4_096;
const MAX_DECODED_TEXT_BYTES: usize = 128 * 1_024 * 1_024;

pub(super) struct KindleContainer<'a> {
    database: PalmDatabase<'a>,
    header: KindleHeader,
    metadata: ExthMetadata,
    full_name: Option<String>,
}

impl<'a> KindleContainer<'a> {
    pub(super) fn parse(bytes: &'a [u8]) -> Result<Self> {
        let database = PalmDatabase::parse(bytes)?;
        let record_zero = database
            .records
            .first()
            .copied()
            .context("MOBI has no record zero")?;
        let header = KindleHeader::parse(record_zero, database.records.len())?;
        let metadata = ExthMetadata::parse(record_zero, &header)?;
        let full_name = header
            .full_name_range(record_zero.len())
            .and_then(|range| record_zero.get(range))
            .and_then(|value| decode_metadata(value, header.text_encoding).ok());
        Ok(Self {
            database,
            header,
            metadata,
            full_name,
        })
    }

    pub(super) fn source_format(&self) -> SourceFormat {
        if self.header.file_version >= 8 {
            SourceFormat::Azw3
        } else {
            SourceFormat::Mobi
        }
    }

    pub(super) fn format_version(&self) -> String {
        self.header.file_version.to_string()
    }

    pub(super) fn file_version(&self) -> u32 {
        self.header.file_version
    }

    pub(super) fn metadata(&self, path: &Path) -> BookMetadata {
        let path_title = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Untitled")
            .to_owned();
        let database_title = String::from_utf8_lossy(self.database.name)
            .trim_matches(char::from(0))
            .replace('_', " ");
        let title = self
            .metadata
            .title
            .clone()
            .or_else(|| self.full_name.clone())
            .or_else(|| (!database_title.trim().is_empty()).then_some(database_title))
            .unwrap_or(path_title);
        BookMetadata {
            title: Some(title),
            authors: self.metadata.authors.clone(),
            language: self.metadata.language.clone(),
            cover: self.cover_asset(),
        }
    }

    pub(super) fn decode_markup(&self) -> Result<String> {
        match self.header.compression {
            1 | 2 => {}
            17_480 => bail!(
                "HUFF/CDIC-compressed MOBI/KF8 is not supported; convert to an uncompressed or PalmDOC-compressed DRM-free file"
            ),
            value => bail!("MOBI/KF8 uses unsupported compression type {value}"),
        }
        let mut output = Vec::with_capacity(self.header.text_length.min(8 * 1_024 * 1_024));
        for record_index in 1..=self.header.text_record_count {
            let record = self
                .database
                .records
                .get(record_index)
                .copied()
                .with_context(|| format!("MOBI text record {record_index} is missing"))?;
            let payload = palmdoc::strip_trailing_data(record, self.header.extra_data_flags)
                .with_context(|| format!("invalid MOBI text record {record_index}"))?;
            match self.header.compression {
                1 => append_bounded(&mut output, payload, self.header.text_length),
                2 => {
                    let remaining = self.header.text_length.saturating_sub(output.len());
                    let decoded = palmdoc::decompress(payload, remaining)
                        .with_context(|| format!("invalid PalmDOC text record {record_index}"))?;
                    append_bounded(&mut output, &decoded, self.header.text_length);
                }
                _ => unreachable!("compression was validated before decoding"),
            }
            if output.len() == self.header.text_length {
                break;
            }
        }
        if output.len() < self.header.text_length {
            bail!(
                "MOBI text is truncated: declared {} bytes but decoded {}",
                self.header.text_length,
                output.len()
            );
        }
        output.truncate(self.header.text_length);
        decode_content(&output, self.header.text_encoding)
    }

    pub(super) fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.header.file_version < 8 && self.metadata.kf8_boundary.is_some() {
            warnings.push(
                "Combined MOBI/KF8 file detected; imported its legacy MOBI rendition".to_owned(),
            );
        }
        if self.metadata.cover_offset.is_some() && self.cover_asset().is_none() {
            warnings.push("Kindle cover record is missing or has an unknown image type".to_owned());
        }
        warnings
    }

    fn cover_asset(&self) -> Option<BookAsset> {
        let first = self.header.first_image_index?;
        let offset = self.metadata.cover_offset?;
        let index = first.checked_add(offset)?;
        let bytes = self.database.records.get(index)?.to_vec();
        let media_type = image_media_type(&bytes)?;
        Some(BookAsset {
            source_id: format!("kindle:record:{index}"),
            media_type: media_type.to_owned(),
            bytes,
        })
    }
}

struct PalmDatabase<'a> {
    name: &'a [u8],
    records: Vec<&'a [u8]>,
}

impl<'a> PalmDatabase<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() < PDB_HEADER_BYTES {
            bail!("invalid MOBI Palm database: header is truncated");
        }
        if bytes.get(60..68) != Some(b"BOOKMOBI") {
            bail!("input is not a MOBI/KF8 Palm database (expected BOOKMOBI)");
        }
        let record_count = usize::from(read_u16(bytes, 76, "PDB record count")?);
        if record_count == 0 {
            bail!("invalid MOBI Palm database: no records");
        }
        if record_count > MAX_RECORDS {
            bail!("MOBI contains more than {MAX_RECORDS} records");
        }
        let table_bytes = record_count
            .checked_mul(PDB_RECORD_ENTRY_BYTES)
            .and_then(|value| PDB_HEADER_BYTES.checked_add(value))
            .context("MOBI record table length overflows")?;
        if table_bytes > bytes.len() {
            bail!("invalid MOBI Palm database: record table is truncated");
        }
        let mut offsets = Vec::with_capacity(record_count + 1);
        for index in 0..record_count {
            let entry = PDB_HEADER_BYTES + index * PDB_RECORD_ENTRY_BYTES;
            let offset = usize::try_from(read_u32(bytes, entry, "PDB record offset")?)
                .context("MOBI record offset does not fit this platform")?;
            if offset < table_bytes || offset >= bytes.len() {
                bail!("invalid MOBI record offsets: record {index} starts outside the data area");
            }
            if offsets.last().is_some_and(|previous| offset <= *previous) {
                bail!("invalid MOBI record offsets: entries are not strictly increasing");
            }
            offsets.push(offset);
        }
        offsets.push(bytes.len());
        let records = offsets
            .windows(2)
            .map(|range| &bytes[range[0]..range[1]])
            .collect();
        Ok(Self {
            name: &bytes[..32],
            records,
        })
    }
}

struct KindleHeader {
    compression: u16,
    text_length: usize,
    text_record_count: usize,
    text_encoding: u32,
    file_version: u32,
    full_name_offset: usize,
    full_name_length: usize,
    first_image_index: Option<usize>,
    extra_data_flags: u32,
    mobi_header_length: usize,
    exth_flags: u32,
}

impl KindleHeader {
    fn parse(record: &[u8], total_records: usize) -> Result<Self> {
        if record.len() < PALMDOC_HEADER_BYTES + 116 {
            bail!("invalid MOBI record zero: header is truncated");
        }
        let compression = read_u16(record, 0, "PalmDOC compression")?;
        let text_length = usize::try_from(read_u32(record, 4, "PalmDOC text length")?)
            .context("MOBI text length does not fit this platform")?;
        if text_length == 0 {
            bail!("MOBI declares no readable text");
        }
        if text_length > MAX_DECODED_TEXT_BYTES {
            bail!("decoded MOBI text exceeds 128 MiB limit");
        }
        let text_record_count = usize::from(read_u16(record, 8, "PalmDOC record count")?);
        if text_record_count == 0 || text_record_count >= total_records {
            bail!("invalid MOBI text record count {text_record_count}");
        }
        let encryption = read_u16(record, 12, "PalmDOC encryption")?;
        if encryption != 0 {
            bail!("encrypted MOBI/KF8 is not supported; provide a DRM-free file");
        }
        if record.get(16..20) != Some(b"MOBI") {
            bail!("invalid MOBI record zero: MOBI header is missing");
        }
        let mobi_header_length = usize::try_from(read_u32(record, 20, "MOBI header length")?)
            .context("MOBI header length does not fit this platform")?;
        let header_end = PALMDOC_HEADER_BYTES
            .checked_add(mobi_header_length)
            .context("MOBI header length overflows")?;
        if mobi_header_length < 116 || header_end > record.len() {
            bail!("invalid MOBI header length {mobi_header_length}");
        }
        if mobi_header_length >= 168 {
            let drm_offset = read_u32(record, PALMDOC_HEADER_BYTES + 152, "MOBI DRM offset")?;
            let drm_count = read_u32(record, PALMDOC_HEADER_BYTES + 156, "MOBI DRM count")?;
            if drm_offset != u32::MAX && drm_count != 0 {
                bail!("encrypted MOBI/KF8 is not supported; provide a DRM-free file");
            }
        }
        let full_name_offset = usize::try_from(read_u32(
            record,
            PALMDOC_HEADER_BYTES + 68,
            "MOBI full-name offset",
        )?)
        .context("MOBI full-name offset does not fit this platform")?;
        let full_name_length = usize::try_from(read_u32(
            record,
            PALMDOC_HEADER_BYTES + 72,
            "MOBI full-name length",
        )?)
        .context("MOBI full-name length does not fit this platform")?;
        let first_image = usize::try_from(read_u32(
            record,
            PALMDOC_HEADER_BYTES + 92,
            "MOBI first-image index",
        )?)
        .ok()
        .filter(|value| *value != usize::try_from(u32::MAX).unwrap_or(usize::MAX));
        let extra_data_flags = if mobi_header_length >= 228 {
            read_u32(record, PALMDOC_HEADER_BYTES + 224, "MOBI extra-data flags")?
        } else {
            0
        };
        Ok(Self {
            compression,
            text_length,
            text_record_count,
            text_encoding: read_u32(record, PALMDOC_HEADER_BYTES + 12, "MOBI text encoding")?,
            file_version: read_u32(record, PALMDOC_HEADER_BYTES + 20, "MOBI format version")?,
            full_name_offset,
            full_name_length,
            first_image_index: first_image,
            extra_data_flags,
            mobi_header_length,
            exth_flags: read_u32(record, PALMDOC_HEADER_BYTES + 112, "MOBI EXTH flags")?,
        })
    }

    fn full_name_range(&self, record_len: usize) -> Option<std::ops::Range<usize>> {
        let end = self.full_name_offset.checked_add(self.full_name_length)?;
        (end <= record_len).then_some(self.full_name_offset..end)
    }
}

#[derive(Default)]
struct ExthMetadata {
    title: Option<String>,
    authors: Vec<String>,
    language: Option<String>,
    cover_offset: Option<usize>,
    kf8_boundary: Option<usize>,
}

impl ExthMetadata {
    fn parse(record: &[u8], header: &KindleHeader) -> Result<Self> {
        if header.exth_flags & 0x40 == 0 {
            return Ok(Self::default());
        }
        let start = PALMDOC_HEADER_BYTES
            .checked_add(header.mobi_header_length)
            .context("MOBI EXTH offset overflows")?;
        if record.get(start..start + 4) != Some(b"EXTH") {
            bail!("MOBI EXTH flag is set but the EXTH header is missing");
        }
        let total_length = usize::try_from(read_u32(record, start + 4, "EXTH length")?)
            .context("EXTH length does not fit this platform")?;
        let end = start
            .checked_add(total_length)
            .context("EXTH length overflows")?;
        if total_length < 12 || end > record.len() {
            bail!("invalid MOBI EXTH length {total_length}");
        }
        let count = usize::try_from(read_u32(record, start + 8, "EXTH record count")?)
            .context("EXTH record count does not fit this platform")?;
        if count > MAX_EXTH_RECORDS {
            bail!("MOBI EXTH contains more than {MAX_EXTH_RECORDS} records");
        }
        let mut metadata = Self::default();
        let mut cursor = start + 12;
        for _ in 0..count {
            if cursor.checked_add(8).is_none_or(|value| value > end) {
                bail!("MOBI EXTH record header is truncated");
            }
            let kind = read_u32(record, cursor, "EXTH record type")?;
            let length = usize::try_from(read_u32(record, cursor + 4, "EXTH record length")?)
                .context("EXTH record length does not fit this platform")?;
            if length < 8 {
                bail!("MOBI EXTH record length is less than 8 bytes");
            }
            let record_end = cursor
                .checked_add(length)
                .context("EXTH record length overflows")?;
            if record_end > end {
                bail!("MOBI EXTH record extends past its header");
            }
            let value = &record[cursor + 8..record_end];
            match kind {
                100 => metadata
                    .authors
                    .push(decode_metadata(value, header.text_encoding)?),
                121 => metadata.kf8_boundary = parse_exth_index(value),
                201 => metadata.cover_offset = parse_exth_index(value),
                503 => metadata.title = Some(decode_metadata(value, header.text_encoding)?),
                524 => metadata.language = Some(decode_metadata(value, header.text_encoding)?),
                _ => {}
            }
            cursor = record_end;
        }
        metadata.authors.retain(|author| !author.is_empty());
        metadata.title = metadata.title.filter(|value| !value.is_empty());
        metadata.language = metadata.language.filter(|value| !value.is_empty());
        Ok(metadata)
    }
}

fn parse_exth_index(value: &[u8]) -> Option<usize> {
    let bytes: [u8; 4] = value.try_into().ok()?;
    usize::try_from(u32::from_be_bytes(bytes)).ok()
}

fn decode_metadata(value: &[u8], encoding: u32) -> Result<String> {
    let decoded = decode_content(value, encoding)?;
    Ok(decoded.trim_matches(char::from(0)).trim().to_owned())
}

fn decode_content(value: &[u8], encoding: u32) -> Result<String> {
    match encoding {
        65_001 => String::from_utf8(
            value
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(value)
                .to_vec(),
        )
        .context("MOBI/KF8 text is not valid UTF-8"),
        1_252 => {
            let (decoded, _, had_errors) = WINDOWS_1252.decode(value);
            if had_errors {
                bail!("MOBI/KF8 text is not valid Windows-1252");
            }
            Ok(decoded.into_owned())
        }
        value => bail!("MOBI/KF8 uses unsupported text encoding {value}"),
    }
}

fn append_bounded(output: &mut Vec<u8>, bytes: &[u8], limit: usize) {
    let remaining = limit.saturating_sub(output.len());
    output.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
}

fn image_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn read_u16(bytes: &[u8], offset: usize, field: &str) -> Result<u16> {
    let end = offset
        .checked_add(2)
        .context("MOBI field offset overflows")?;
    let value: [u8; 2] = bytes
        .get(offset..end)
        .with_context(|| format!("{field} is truncated"))?
        .try_into()
        .expect("two-byte range");
    Ok(u16::from_be_bytes(value))
}

fn read_u32(bytes: &[u8], offset: usize, field: &str) -> Result<u32> {
    let end = offset
        .checked_add(4)
        .context("MOBI field offset overflows")?;
    let value: [u8; 4] = bytes
        .get(offset..end)
        .with_context(|| format!("{field} is truncated"))?
        .try_into()
        .expect("four-byte range");
    Ok(u32::from_be_bytes(value))
}
