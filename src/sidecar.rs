//! Versioned navigation and reproducibility sidecars.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::book::{CanonicalBook, Section, SourceRange};
use crate::m4b::ChapterPolicy;
use crate::narration::{FootnoteMode, NarrationPlan};
use crate::synthesis::{SynthesisResult, TtsProviderIdentity};
use crate::timeline::{AudioCue, AudioTimeline, CueKind};

pub const AUDIONAV_SCHEMA_VERSION: u32 = 1;
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct ManifestOptions {
    pub provider: TtsProviderIdentity,
    pub speed: f32,
    pub footnotes: FootnoteMode,
    pub chapters: ChapterPolicy,
    pub output_files: Vec<PathBuf>,
    pub build_timestamp_unix_seconds: u64,
}

/// Write deterministic, versioned semantic navigation as atomic JSON.
///
/// # Errors
///
/// Returns an error for output-directory, serialization, or persistence failure.
pub fn write_audionav(book: &CanonicalBook, timeline: &AudioTimeline, output: &Path) -> Result<()> {
    let section_cues = timeline
        .cues
        .iter()
        .filter(|cue| cue.id.starts_with("section:"))
        .filter_map(|cue| cue.section_id.as_deref().map(|id| (id, cue)))
        .collect::<HashMap<_, _>>();
    let toc = book
        .root
        .children
        .iter()
        .map(|section| toc_entry(section, &section_cues))
        .collect();
    let pages = timeline
        .cues
        .iter()
        .filter_map(|cue| match &cue.kind {
            CueKind::Page { label } => Some(PageEntry {
                id: cue.id.clone(),
                label: label.clone(),
                start_ms: cue.start_ms,
                source_range: cue.source_range.clone(),
            }),
            _ => None,
        })
        .collect();
    let paragraphs = cue_mappings(timeline, |kind| kind == &CueKind::Paragraph);
    let sentences = cue_mappings(timeline, |kind| kind == &CueKind::Sentence);
    let document = AudionavDocument {
        schema_version: AUDIONAV_SCHEMA_VERSION,
        title: book
            .metadata
            .title
            .clone()
            .unwrap_or_else(|| "Untitled".to_owned()),
        duration_ms: timeline.duration_ms,
        toc,
        pages,
        paragraphs,
        sentences,
    };
    write_json_atomic(output, &document)
}

fn toc_entry(section: &Section, cues: &HashMap<&str, &AudioCue>) -> TocEntry {
    let cue = cues.get(section.id.as_str()).copied();
    TocEntry {
        id: section.id.clone(),
        kind: section_kind(section),
        title: section.title.clone(),
        narrated: cue.is_some(),
        start_ms: cue.map(|cue| cue.start_ms),
        end_ms: cue.and_then(|cue| cue.end_ms),
        source_range: section.source_range.clone(),
        children: section
            .children
            .iter()
            .map(|child| toc_entry(child, cues))
            .collect(),
    }
}

fn section_kind(section: &Section) -> &'static str {
    use crate::book::SectionKind;
    match section.kind {
        SectionKind::Book => "book",
        SectionKind::FrontMatter => "front_matter",
        SectionKind::BodyMatter => "body_matter",
        SectionKind::Part => "part",
        SectionKind::Chapter => "chapter",
        SectionKind::Section => "section",
        SectionKind::Appendix => "appendix",
        SectionKind::Notes => "notes",
        SectionKind::Bibliography => "bibliography",
        SectionKind::Index => "index",
        SectionKind::BackMatter => "back_matter",
        SectionKind::Other => "other",
    }
}

fn cue_mappings(timeline: &AudioTimeline, include: impl Fn(&CueKind) -> bool) -> Vec<TimedMapping> {
    timeline
        .cues
        .iter()
        .filter(|cue| include(&cue.kind))
        .map(|cue| TimedMapping {
            id: cue.id.clone(),
            start_ms: cue.start_ms,
            end_ms: cue.end_ms,
            section_id: cue.section_id.clone(),
            source_range: cue.source_range.clone(),
        })
        .collect()
}

/// Write a source-hashed, credential-free build manifest as atomic JSON.
///
/// # Errors
///
/// Returns an error for source hashing, output, serialization, or persistence.
pub fn write_manifest(
    book: &CanonicalBook,
    plan: &NarrationPlan,
    synthesis: &SynthesisResult,
    options: &ManifestOptions,
    output: &Path,
) -> Result<()> {
    let source_hash = sha256_file(&book.source.path)?;
    let narrated_text = plan
        .units
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut warnings = book.warnings.clone();
    warnings.extend(plan.warnings.iter().cloned());
    let document = ManifestDocument {
        schema_version: MANIFEST_SCHEMA_VERSION,
        build_timestamp_unix_seconds: options.build_timestamp_unix_seconds,
        tool: ToolInfo {
            name: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
            importer_version: env!("CARGO_PKG_VERSION"),
        },
        source: SourceInfo {
            file: book.source.path.display().to_string(),
            sha256: source_hash,
            format: book.source.format.to_string(),
            format_version: book.source.format_version.clone(),
        },
        metadata: MetadataInfo::from_book(book),
        narration: NarrationInfo {
            provider: options.provider.provider.clone(),
            model: options.provider.model.clone(),
            voice: options.provider.voice.clone(),
            language: options.provider.language.clone(),
            speed: options.speed,
            footnotes: footnote_mode(options.footnotes),
            provider_character_limit: options.provider.max_characters,
            sample_rate: options.provider.sample_rate,
        },
        encoding: EncodingInfo {
            container: "m4b",
            codec: "aac",
            bitrate_bps: 64_000,
            channels: 1,
        },
        chapter_policy: options.chapters.as_str(),
        omitted_or_skipped: plan.warnings.clone(),
        warnings,
        output_files: options
            .output_files
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        duration_ms: synthesis.timeline.duration_ms,
        counts: CountInfo {
            source_words: book.word_count(),
            narrated_words: narrated_text.split_whitespace().count(),
            narrated_characters: narrated_text.chars().count(),
            narration_units: plan.units.len(),
            provider_chunks: synthesis.provider_chunks,
            cache_hits: synthesis.cache_hits,
            generated_chunks: synthesis.generated_chunks,
        },
    };
    write_json_atomic(output, &document)
}

fn footnote_mode(mode: FootnoteMode) -> &'static str {
    match mode {
        FootnoteMode::Inline => "inline",
        FootnoteMode::Skip => "skip",
        FootnoteMode::End => "end",
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("failed to open source for hashing {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1_024];
    loop {
        let count = reader
            .read(&mut buffer)
            .with_context(|| format!("failed to hash source {}", path.display()))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn write_json_atomic<T: Serialize>(path: &Path, document: &T) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create sidecar directory {}", parent.display()))?;
    let temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary sidecar in {}", parent.display()))?;
    let (file, temporary) = temporary.into_parts();
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, document)
        .context("failed to serialize sidecar JSON")?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    drop(writer);
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to save sidecar {}", path.display()))?;
    Ok(())
}

#[derive(Serialize)]
struct AudionavDocument {
    schema_version: u32,
    title: String,
    duration_ms: u64,
    toc: Vec<TocEntry>,
    pages: Vec<PageEntry>,
    paragraphs: Vec<TimedMapping>,
    sentences: Vec<TimedMapping>,
}

#[derive(Serialize)]
struct TocEntry {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    title: Option<String>,
    narrated: bool,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    source_range: Option<SourceRange>,
    children: Vec<Self>,
}

#[derive(Serialize)]
struct PageEntry {
    id: String,
    label: String,
    start_ms: u64,
    source_range: Option<SourceRange>,
}

#[derive(Serialize)]
struct TimedMapping {
    id: String,
    start_ms: u64,
    end_ms: Option<u64>,
    section_id: Option<String>,
    source_range: Option<SourceRange>,
}

#[derive(Serialize)]
struct ManifestDocument {
    schema_version: u32,
    build_timestamp_unix_seconds: u64,
    tool: ToolInfo,
    source: SourceInfo,
    metadata: MetadataInfo,
    narration: NarrationInfo,
    encoding: EncodingInfo,
    chapter_policy: &'static str,
    omitted_or_skipped: Vec<String>,
    warnings: Vec<String>,
    output_files: Vec<String>,
    duration_ms: u64,
    counts: CountInfo,
}

#[derive(Serialize)]
struct ToolInfo {
    name: &'static str,
    version: &'static str,
    importer_version: &'static str,
}

#[derive(Serialize)]
struct SourceInfo {
    file: String,
    sha256: String,
    format: String,
    format_version: Option<String>,
}

#[derive(Serialize)]
struct MetadataInfo {
    title: Option<String>,
    authors: Vec<String>,
    language: Option<String>,
    cover: Option<CoverInfo>,
}

impl MetadataInfo {
    fn from_book(book: &CanonicalBook) -> Self {
        Self {
            title: book.metadata.title.clone(),
            authors: book.metadata.authors.clone(),
            language: book.metadata.language.clone(),
            cover: book.metadata.cover.as_ref().map(|cover| CoverInfo {
                source_id: cover.source_id.clone(),
                media_type: cover.media_type.clone(),
                bytes: cover.bytes.len(),
            }),
        }
    }
}

#[derive(Serialize)]
struct CoverInfo {
    source_id: String,
    media_type: String,
    bytes: usize,
}

#[derive(Serialize)]
struct NarrationInfo {
    provider: String,
    model: String,
    voice: String,
    language: Option<String>,
    speed: f32,
    footnotes: &'static str,
    provider_character_limit: usize,
    sample_rate: u32,
}

#[derive(Serialize)]
struct EncodingInfo {
    container: &'static str,
    codec: &'static str,
    bitrate_bps: u32,
    channels: u8,
}

#[derive(Serialize)]
struct CountInfo {
    source_words: usize,
    narrated_words: usize,
    narrated_characters: usize,
    narration_units: usize,
    provider_chunks: usize,
    cache_hits: usize,
    generated_chunks: usize,
}
