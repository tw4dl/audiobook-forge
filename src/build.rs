//! Complete semantic audiobook build orchestration.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use tempfile::{Builder as TempBuilder, NamedTempFile};

use crate::book::{BookAsset, CanonicalBook};
use crate::m4b::{ChapterPolicy, M4bReport, assemble_m4b, ensure_media_tools};
use crate::narration::{NarrationPolicy, plan_narration};
use crate::sidecar::{ManifestOptions, write_audionav, write_manifest};
use crate::synthesis::{
    SegmentCache, SynthesisResult, SynthesisSettings, TtsProvider, synthesize_plan,
};

#[derive(Debug, Clone, PartialEq)]
pub struct AudiobookBuildOptions {
    pub output_dir: PathBuf,
    pub base_name: String,
    pub chapters: ChapterPolicy,
    pub narration: NarrationPolicy,
    pub synthesis: SynthesisSettings,
    pub pronunciation_overrides: Vec<String>,
    pub build_timestamp_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudiobookBuildReport {
    pub m4b_path: PathBuf,
    pub audionav_path: PathBuf,
    pub manifest_path: PathBuf,
    pub cover_path: Option<PathBuf>,
    pub m4b: M4bReport,
    pub synthesis: SynthesisResult,
}

/// Build one M4B, one navigation sidecar, one manifest, and an optional cover.
///
/// # Errors
///
/// Returns an error for unsafe output names, media tools, synthesis, encoding,
/// cover conversion, source hashing, or sidecar persistence.
pub fn build_audiobook<P: TtsProvider>(
    book: &CanonicalBook,
    provider: &mut P,
    cache: &SegmentCache,
    options: &AudiobookBuildOptions,
) -> Result<AudiobookBuildReport> {
    validate_build_options(options)?;
    ensure_media_tools()?;
    fs::create_dir_all(&options.output_dir).with_context(|| {
        format!(
            "failed to create audiobook output directory {}",
            options.output_dir.display()
        )
    })?;

    let plan = plan_narration(book, options.narration);
    let synthesis = synthesize_plan(&plan, provider, cache, &options.synthesis)?;
    let m4b_path = options
        .output_dir
        .join(format!("{}.m4b", options.base_name));
    let audionav_path = options
        .output_dir
        .join(format!("{}.audionav.json", options.base_name));
    let manifest_path = options
        .output_dir
        .join(format!("{}.manifest.json", options.base_name));

    let m4b = assemble_m4b(book, &synthesis, &m4b_path, options.chapters)?;
    write_audionav(book, &synthesis.timeline, &audionav_path)?;
    let cover_path = export_cover(book.metadata.cover.as_ref(), &options.output_dir)?;
    let mut output_files = vec![
        file_name(&m4b_path)?,
        file_name(&audionav_path)?,
        file_name(&manifest_path)?,
    ];
    if let Some(cover) = cover_path.as_ref() {
        output_files.push(file_name(cover)?);
    }
    write_manifest(
        book,
        &plan,
        &synthesis,
        &ManifestOptions {
            provider: provider.identity().clone(),
            speed: options.synthesis.speed,
            footnotes: options.narration.footnotes,
            chapters: options.chapters,
            pause_ms: options.synthesis.pause_ms,
            max_retries: options.synthesis.max_retries,
            pronunciation_overrides: options.pronunciation_overrides.clone(),
            output_files,
            build_timestamp_unix_seconds: options.build_timestamp_unix_seconds,
        },
        &manifest_path,
    )?;

    Ok(AudiobookBuildReport {
        m4b_path,
        audionav_path,
        manifest_path,
        cover_path,
        m4b,
        synthesis,
    })
}

/// Validate output paths before model setup or synthesis.
///
/// # Errors
///
/// Returns an error for an unsafe base name or an occupied output file path.
pub fn validate_build_options(options: &AudiobookBuildOptions) -> Result<()> {
    let mut components = Path::new(&options.base_name).components();
    let valid_component =
        matches!(components.next(), Some(Component::Normal(value)) if !value.is_empty());
    if !valid_component
        || components.next().is_some()
        || options.base_name.contains(char::is_control)
    {
        bail!("output base name must be one safe file-name component");
    }
    if options.output_dir.exists() && !options.output_dir.is_dir() {
        bail!(
            "audiobook output path is not a directory: {}",
            options.output_dir.display()
        );
    }
    Ok(())
}

fn export_cover(cover: Option<&BookAsset>, output_dir: &Path) -> Result<Option<PathBuf>> {
    let Some(cover) = cover else {
        return Ok(None);
    };
    let output = output_dir.join("cover.jpg");
    if cover.media_type == "image/jpeg" {
        write_atomic(&output, &cover.bytes)?;
        return Ok(Some(output));
    }
    let extension = cover_extension(&cover.media_type)?;
    let mut input = TempBuilder::new()
        .prefix(".audiobook-forge-cover-input-")
        .suffix(extension)
        .tempfile_in(output_dir)
        .context("failed to stage source cover")?;
    input
        .write_all(&cover.bytes)
        .context("failed to stage source cover bytes")?;
    input
        .flush()
        .context("failed to flush staged source cover")?;
    let temporary = TempBuilder::new()
        .prefix(".audiobook-forge-cover-")
        .suffix(".jpg")
        .tempfile_in(output_dir)
        .context("failed to create temporary JPEG cover")?
        .into_temp_path();
    let result = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input.path())
        .args(["-frames:v", "1", "-q:v", "2", "-f", "image2"])
        .arg(&temporary)
        .output()
        .context("failed to run ffmpeg for cover conversion")?;
    if !result.status.success() {
        bail!(
            "ffmpeg cover conversion failed with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    temporary
        .persist(&output)
        .with_context(|| format!("failed to save cover {}", output.display()))?;
    Ok(Some(output))
}

fn cover_extension(media_type: &str) -> Result<&'static str> {
    match media_type {
        "image/png" => Ok(".png"),
        "image/webp" => Ok(".webp"),
        "image/gif" => Ok(".gif"),
        _ => bail!("unsupported cover media type {media_type}"),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary cover in {}", parent.display()))?;
    temporary
        .write_all(bytes)
        .context("failed to write cover")?;
    temporary.flush().context("failed to flush cover")?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to save cover {}", path.display()))?;
    Ok(())
}

fn file_name(path: &Path) -> Result<PathBuf> {
    path.file_name()
        .map(PathBuf::from)
        .with_context(|| format!("output has no file name: {}", path.display()))
}
