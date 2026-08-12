//! AAC M4B assembly and independent `ffprobe` validation.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use hound::WavReader;
use serde::Deserialize;
use tempfile::{Builder as TempBuilder, TempDir};

use crate::audio::StreamingWav;
use crate::book::{CanonicalBook, Section, SectionKind};
use crate::synthesis::SynthesisResult;
use crate::timeline::AudioTimeline;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterPolicy {
    Chapters,
    Sections,
    Auto,
}

impl ChapterPolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chapters => "chapters",
            Self::Sections => "sections",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M4bChapter {
    pub section_id: String,
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M4bReport {
    pub path: PathBuf,
    pub duration_ms: u64,
    pub codec: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub has_cover: bool,
    pub chapters: Vec<M4bChapter>,
}

/// Confirm that the external AAC encoder and independent validator are ready.
///
/// # Errors
///
/// Returns an error when ffmpeg or ffprobe is missing or unusable.
pub fn ensure_media_tools() -> Result<()> {
    require_tool("ffmpeg")?;
    require_tool("ffprobe")
}

/// Assemble cached PCM into one atomic AAC M4B and validate it with `ffprobe`.
///
/// # Errors
///
/// Returns an error for missing media tools, invalid PCM, encoding, metadata,
/// or independent structural validation.
pub fn assemble_m4b(
    book: &CanonicalBook,
    synthesis: &SynthesisResult,
    output: &Path,
    policy: ChapterPolicy,
) -> Result<M4bReport> {
    ensure_media_tools()?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    let workspace = tempfile::tempdir_in(parent)
        .with_context(|| format!("failed to create M4B workspace in {}", parent.display()))?;
    let pcm = workspace.path().join("audiobook.wav");
    assemble_pcm(synthesis, &pcm)?;
    let chapters = select_chapters(book, &synthesis.timeline, policy);
    let metadata = workspace.path().join("metadata.ffmeta");
    write_metadata(book, &chapters, &metadata)?;
    let cover = write_cover(book, &workspace)?;
    let temporary = TempBuilder::new()
        .prefix(".audiobook-forge-")
        .suffix(".m4b")
        .tempfile_in(parent)
        .with_context(|| format!("failed to create temporary M4B in {}", parent.display()))?;
    let (_, temporary) = temporary.into_parts();

    encode_m4b(&pcm, &metadata, cover.as_deref(), &temporary)?;
    let report = validate_m4b(&temporary)?;
    validate_expected(book, &chapters, cover.is_some(), &report)?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to save {}", output.display()))?;
    validate_m4b(output)
}

/// Select stable visible chapters without exposing provider chunks.
#[must_use]
pub fn select_chapters(
    book: &CanonicalBook,
    timeline: &AudioTimeline,
    policy: ChapterPolicy,
) -> Vec<M4bChapter> {
    let mut sections = HashMap::new();
    collect_sections(&book.root, &mut sections);
    let effective_policy = if policy == ChapterPolicy::Auto {
        let major_count = sections
            .values()
            .filter(|section| is_major_section(section.kind))
            .count();
        if major_count == 0 {
            ChapterPolicy::Sections
        } else {
            ChapterPolicy::Chapters
        }
    } else {
        policy
    };
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for cue in &timeline.cues {
        let Some(section_id) = cue.section_id.as_deref() else {
            continue;
        };
        if !cue.id.starts_with("section:") || !seen.insert(section_id.to_owned()) {
            continue;
        }
        let Some(section) = sections.get(section_id) else {
            continue;
        };
        let include = match effective_policy {
            ChapterPolicy::Chapters | ChapterPolicy::Auto => is_major_section(section.kind),
            ChapterPolicy::Sections => is_visible_section(section.kind),
        };
        if include && let Some(title) = section.title.filter(|title| !title.trim().is_empty()) {
            selected.push(M4bChapter {
                section_id: section_id.to_owned(),
                title: title.trim().to_owned(),
                start_ms: cue.start_ms,
                end_ms: cue.end_ms.unwrap_or(timeline.duration_ms),
            });
        }
    }
    selected.sort_by_key(|chapter| chapter.start_ms);
    let mut deduplicated = Vec::<M4bChapter>::new();
    for chapter in selected {
        if deduplicated
            .last()
            .is_some_and(|previous| previous.start_ms == chapter.start_ms)
        {
            continue;
        }
        deduplicated.push(chapter);
    }
    if deduplicated.is_empty() && timeline.duration_ms > 0 {
        deduplicated.push(M4bChapter {
            section_id: book.root.id.clone(),
            title: book
                .metadata
                .title
                .clone()
                .unwrap_or_else(|| "Audiobook".to_owned()),
            start_ms: 0,
            end_ms: timeline.duration_ms,
        });
    }
    for index in 0..deduplicated.len() {
        let next_start = deduplicated
            .get(index + 1)
            .map_or(timeline.duration_ms, |chapter| chapter.start_ms);
        deduplicated[index].end_ms = next_start.max(deduplicated[index].start_ms + 1);
    }
    deduplicated
}

#[derive(Clone, Copy)]
struct SectionInfo<'a> {
    kind: SectionKind,
    title: Option<&'a str>,
}

fn collect_sections<'a>(section: &'a Section, output: &mut HashMap<&'a str, SectionInfo<'a>>) {
    output.insert(
        &section.id,
        SectionInfo {
            kind: section.kind,
            title: section.title.as_deref(),
        },
    );
    for child in &section.children {
        collect_sections(child, output);
    }
}

fn is_major_section(kind: SectionKind) -> bool {
    matches!(
        kind,
        SectionKind::Part
            | SectionKind::Chapter
            | SectionKind::FrontMatter
            | SectionKind::Appendix
            | SectionKind::Notes
            | SectionKind::BackMatter
    )
}

fn is_visible_section(kind: SectionKind) -> bool {
    !matches!(
        kind,
        SectionKind::Book | SectionKind::BodyMatter | SectionKind::Index
    )
}

fn assemble_pcm(synthesis: &SynthesisResult, output: &Path) -> Result<()> {
    let mut writer = StreamingWav::create(output)?;
    for unit in &synthesis.rendered_units {
        for chunk in &unit.chunks {
            if chunk.sample_rate != synthesis.sample_rate {
                bail!("cached audio sample rates do not match");
            }
            let mut reader = WavReader::open(&chunk.path)
                .with_context(|| format!("failed to read cached audio {}", chunk.path.display()))?;
            let mut samples = Vec::with_capacity(4_096);
            for sample in reader.samples::<i16>() {
                samples.push(f32::from(sample.context("cached PCM is invalid")?) / 32_768.0);
                if samples.len() == 4_096 {
                    writer.write_chunk(&samples)?;
                    samples.clear();
                }
            }
            if !samples.is_empty() {
                writer.write_chunk(&samples)?;
            }
        }
        writer.write_silence_samples(unit.silence_after_samples)?;
    }
    let report = writer.finish()?;
    if report.samples == 0 {
        bail!("assembled audiobook contains no audio");
    }
    Ok(())
}

fn write_metadata(book: &CanonicalBook, chapters: &[M4bChapter], output: &Path) -> Result<()> {
    let title = book.metadata.title.as_deref().unwrap_or("Untitled");
    let artist = book.metadata.authors.join("; ");
    let mut metadata = format!(
        ";FFMETADATA1\ntitle={}\nalbum={}\ngenre=Audiobook\nmedia_type=2\n",
        escape_metadata(title),
        escape_metadata(title)
    );
    if !artist.is_empty() {
        writeln!(&mut metadata, "artist={}", escape_metadata(&artist))?;
    }
    for chapter in chapters {
        write!(
            &mut metadata,
            "[CHAPTER]\nTIMEBASE=1/1000\nSTART={}\nEND={}\ntitle={}\n",
            chapter.start_ms,
            chapter.end_ms,
            escape_metadata(&chapter.title)
        )?;
    }
    fs::write(output, metadata)
        .with_context(|| format!("failed to write M4B metadata {}", output.display()))
}

fn escape_metadata(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '=' | ';' | '#' => {
                escaped.push('\\');
                escaped.push(character);
            }
            '\n' => escaped.push_str("\\n"),
            '\r' => {}
            _ => escaped.push(character),
        }
    }
    escaped
}

fn write_cover(book: &CanonicalBook, workspace: &TempDir) -> Result<Option<PathBuf>> {
    let Some(cover) = book.metadata.cover.as_ref() else {
        return Ok(None);
    };
    let extension = match cover.media_type.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        _ => {
            bail!("unsupported M4B cover media type {}", cover.media_type);
        }
    };
    let path = workspace.path().join(format!("cover.{extension}"));
    fs::write(&path, &cover.bytes)
        .with_context(|| format!("failed to stage M4B cover {}", path.display()))?;
    Ok(Some(path))
}

fn encode_m4b(pcm: &Path, metadata: &Path, cover: Option<&Path>, output: &Path) -> Result<()> {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .arg("-i")
        .arg(pcm)
        .args(["-f", "ffmetadata", "-i"])
        .arg(metadata);
    if let Some(cover) = cover {
        command.arg("-i").arg(cover);
    }
    command
        .args(["-map", "0:a:0", "-map_metadata", "1", "-map_chapters", "1"])
        .args(["-c:a", "aac", "-b:a", "64k", "-ac", "1"]);
    if cover.is_some() {
        command
            .args(["-map", "2:v:0", "-c:v", "copy"])
            .args(["-disposition:v:0", "attached_pic"])
            .args(["-metadata:s:v", "title=Cover"])
            .args(["-metadata:s:v", "comment=Cover (front)"]);
    }
    command
        .args(["-movflags", "+faststart", "-f", "ipod"])
        .arg(output);
    let result = command.output().context("failed to run ffmpeg")?;
    if !result.status.success() {
        bail!(
            "ffmpeg failed with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    Ok(())
}

/// Parse and validate an M4B with the independent `ffprobe` executable.
///
/// # Errors
///
/// Returns an error when the file, AAC stream, duration, chapters, or metadata
/// is invalid.
pub fn validate_m4b(path: &Path) -> Result<M4bReport> {
    require_tool("ffprobe")?;
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "-show_chapters",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("failed to inspect {} with ffprobe", path.display()))?;
    if !output.status.success() {
        bail!(
            "ffprobe failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let probe: Probe = serde_json::from_slice(&output.stdout).context("invalid ffprobe JSON")?;
    let duration_ms = parse_milliseconds(probe.format.duration.as_deref(), "M4B duration")?;
    if duration_ms == 0 {
        bail!("M4B duration is zero");
    }
    let audio = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"))
        .context("M4B contains no audio stream")?;
    let codec = audio.codec_name.clone().unwrap_or_default();
    if codec != "aac" {
        bail!("M4B audio codec is {codec:?}, expected AAC");
    }
    let mut chapters = Vec::new();
    let mut previous_start = None;
    for (index, chapter) in probe.chapters.iter().enumerate() {
        let start_ms = parse_milliseconds(chapter.start_time.as_deref(), "chapter start")?;
        let end_ms = parse_milliseconds(chapter.end_time.as_deref(), "chapter end")?;
        if end_ms <= start_ms
            || previous_start.is_some_and(|previous| start_ms < previous)
            || end_ms > duration_ms.saturating_add(1_000)
        {
            bail!("M4B chapter {} has invalid timestamps", index + 1);
        }
        previous_start = Some(start_ms);
        chapters.push(M4bChapter {
            section_id: String::new(),
            title: tag(&chapter.tags, "title")
                .map_or_else(|| format!("Chapter {}", index + 1), str::to_owned),
            start_ms,
            end_ms,
        });
    }
    let has_cover = probe.streams.iter().any(|stream| {
        stream.codec_type.as_deref() == Some("video") && stream.disposition.attached_pic == 1
    });
    Ok(M4bReport {
        path: path.to_path_buf(),
        duration_ms,
        codec,
        title: tag(&probe.format.tags, "title").map(str::to_owned),
        artist: tag(&probe.format.tags, "artist").map(str::to_owned),
        has_cover,
        chapters,
    })
}

fn validate_expected(
    book: &CanonicalBook,
    chapters: &[M4bChapter],
    expected_cover: bool,
    report: &M4bReport,
) -> Result<()> {
    if report.title.as_deref() != book.metadata.title.as_deref() {
        bail!("M4B title metadata does not match the imported title");
    }
    let expected_artist =
        (!book.metadata.authors.is_empty()).then(|| book.metadata.authors.join("; "));
    if report.artist != expected_artist {
        bail!("M4B author metadata does not match imported authors");
    }
    if report.has_cover != expected_cover {
        bail!("M4B embedded-cover validation failed");
    }
    if report.chapters.len() != chapters.len()
        || report
            .chapters
            .iter()
            .zip(chapters)
            .any(|(actual, expected)| actual.title != expected.title)
    {
        bail!("M4B chapter metadata does not match selected navigation");
    }
    Ok(())
}

fn require_tool(tool: &str) -> Result<()> {
    let output = Command::new(tool)
        .arg("-version")
        .output()
        .with_context(|| format!("{tool} is required for M4B output"))?;
    if !output.status.success() {
        bail!("{tool} is required for M4B output");
    }
    Ok(())
}

fn tag<'a>(tags: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    tags.iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()))
}

fn parse_milliseconds(value: Option<&str>, label: &str) -> Result<u64> {
    let seconds = value
        .context(format!("{label} is missing"))?
        .parse::<f64>()
        .with_context(|| format!("{label} is invalid"))?;
    if !seconds.is_finite() || seconds < 0.0 {
        bail!("{label} is invalid");
    }
    milliseconds_from_seconds(seconds)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn milliseconds_from_seconds(seconds: f64) -> Result<u64> {
    let milliseconds = seconds * 1_000.0;
    if milliseconds > u64::MAX as f64 {
        bail!("media duration exceeds supported range");
    }
    Ok(milliseconds.round() as u64)
}

#[derive(Debug, Deserialize, Default)]
struct Probe {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    chapters: Vec<ProbeChapter>,
    #[serde(default)]
    format: ProbeFormat,
}

#[derive(Debug, Deserialize, Default)]
struct ProbeFormat {
    duration: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
struct ProbeStream {
    codec_name: Option<String>,
    codec_type: Option<String>,
    #[serde(default)]
    disposition: ProbeDisposition,
}

#[derive(Debug, Deserialize, Default)]
struct ProbeDisposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Debug, Deserialize, Default)]
struct ProbeChapter {
    start_time: Option<String>,
    end_time: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}
