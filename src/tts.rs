//! Kokoro synthesis and streaming WAV output.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use hound::{SampleFormat, WavSpec, WavWriter};
use indicatif::{ProgressBar, ProgressStyle};
use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig,
    OfflineTtsModelConfig,
};
use tempfile::NamedTempFile;

use crate::chunk::chunk_text;
use crate::model::ModelAssets;
use crate::voice::Voice;

pub const SAMPLE_RATE: u32 = 24_000;
const SILENCE_MS: u32 = 120;

#[derive(Debug, Clone)]
pub struct SynthesisReport {
    pub output: PathBuf,
    pub chunks: usize,
    pub model_load_seconds: f64,
    pub synthesis_seconds: f64,
    pub audio_seconds: f64,
    pub rtf: f64,
}

pub struct SynthesisOptions<'a> {
    pub text: &'a str,
    pub output: &'a Path,
    pub voice: Voice,
    pub speed: f32,
    pub chunk_chars: usize,
    pub threads: i32,
    pub quiet: bool,
}

/// Check synthesis settings before model setup.
///
/// # Errors
///
/// Returns an error when speed, chunk size, or thread count is outside its valid range.
pub fn validate_settings(speed: f32, chunk_chars: usize, threads: i32) -> Result<()> {
    if !(0.5..=2.0).contains(&speed) {
        bail!("speed must be between 0.5 and 2.0");
    }
    if chunk_chars == 0 {
        bail!("chunk size must be greater than zero");
    }
    if threads < 1 {
        bail!("threads must be greater than zero");
    }
    Ok(())
}

/// Load Kokoro once, synthesize each text chunk, and atomically save one WAV.
///
/// # Errors
///
/// Returns an error for invalid options, model failures, empty audio, or output failures.
pub fn synthesize_to_wav(
    assets: &ModelAssets,
    options: &SynthesisOptions<'_>,
) -> Result<SynthesisReport> {
    validate_settings(options.speed, options.chunk_chars, options.threads)?;
    let chunks = chunk_text(options.text, options.chunk_chars)?;
    if chunks.is_empty() {
        bail!("input contains no readable text");
    }

    let load_started = Instant::now();
    let engine = create_engine(assets, options.threads)?;
    let model_load_seconds = load_started.elapsed().as_secs_f64();
    if engine.sample_rate() != SAMPLE_RATE.cast_signed() {
        bail!(
            "Kokoro returned an unexpected sample rate: {}",
            engine.sample_rate()
        );
    }
    if options.voice.speaker_id() >= engine.num_speakers() {
        bail!(
            "voice {} is not present in this model bundle",
            options.voice
        );
    }

    let output_parent = options.output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent).with_context(|| {
        format!(
            "failed to create output directory {}",
            output_parent.display()
        )
    })?;
    let mut temporary = NamedTempFile::new_in(output_parent).with_context(|| {
        format!(
            "failed to create temporary output in {}",
            output_parent.display()
        )
    })?;
    let progress = synthesis_progress(chunks.len() as u64, options.quiet);
    let mut synthesis_seconds = 0.0;
    let mut generated_samples = 0_u64;

    {
        let spec = WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer =
            WavWriter::new(&mut temporary, spec).context("failed to start WAV output")?;
        for (index, chunk) in chunks.iter().enumerate() {
            let started = Instant::now();
            let generated = engine
                .generate_with_config(
                    chunk,
                    &GenerationConfig {
                        sid: options.voice.speaker_id(),
                        speed: options.speed,
                        ..Default::default()
                    },
                    None::<fn(&[f32], f32) -> bool>,
                )
                .ok_or_else(|| anyhow!("Kokoro failed to synthesize chunk {}", index + 1))?;
            let elapsed = started.elapsed().as_secs_f64();
            if generated.samples().is_empty() {
                bail!("Kokoro returned empty audio for chunk {}", index + 1);
            }
            let sample_count =
                u64::try_from(generated.samples().len()).context("generated chunk is too large")?;
            synthesis_seconds += elapsed;
            generated_samples += sample_count;
            write_samples(&mut writer, generated.samples())?;

            if index + 1 < chunks.len() {
                let silence_samples = u64::from(SAMPLE_RATE) * u64::from(SILENCE_MS) / 1_000;
                for _ in 0..silence_samples {
                    writer
                        .write_sample(0_i16)
                        .context("failed to write WAV silence")?;
                }
                generated_samples += silence_samples;
            }
            progress.inc(1);
        }
        writer.finalize().context("failed to finalize WAV output")?;
    }
    progress.finish_and_clear();
    temporary
        .persist(options.output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to save {}", options.output.display()))?;

    let audio_seconds = samples_to_seconds(generated_samples);
    Ok(SynthesisReport {
        output: options.output.to_path_buf(),
        chunks: chunks.len(),
        model_load_seconds,
        synthesis_seconds,
        audio_seconds,
        rtf: synthesis_seconds / audio_seconds,
    })
}

fn create_engine(assets: &ModelAssets, threads: i32) -> Result<OfflineTts> {
    let path = |path: &Path| path.to_string_lossy().into_owned();
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            kokoro: OfflineTtsKokoroModelConfig {
                model: Some(path(&assets.model)),
                voices: Some(path(&assets.voices)),
                tokens: Some(path(&assets.tokens)),
                data_dir: Some(path(&assets.data_dir)),
                dict_dir: None,
                lexicon: Some(path(&assets.lexicon_us)),
                lang: Some("en-us".to_owned()),
                length_scale: 1.0,
            },
            num_threads: threads,
            debug: false,
            ..Default::default()
        },
        ..Default::default()
    };
    OfflineTts::create(&config).ok_or_else(|| anyhow!("failed to load Kokoro model"))
}

fn write_samples<W: std::io::Write + std::io::Seek>(
    writer: &mut WavWriter<W>,
    samples: &[f32],
) -> Result<()> {
    for &sample in samples {
        if !sample.is_finite() {
            bail!("Kokoro returned non-finite audio");
        }
        let scaled = sample.clamp(-1.0, 1.0) * f32::from(i16::MAX);
        writer
            .write_sample(float_to_i16(scaled))
            .context("failed to write WAV sample")?;
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn samples_to_seconds(sample_count: u64) -> f64 {
    // A classic WAV cannot hold enough samples to exceed f64's exact integer range.
    sample_count as f64 / f64::from(SAMPLE_RATE)
}

#[allow(clippy::cast_possible_truncation)]
fn float_to_i16(sample: f32) -> i16 {
    // Callers clamp and scale the sample to the full i16 range first.
    sample.round() as i16
}

fn synthesis_progress(length: u64, quiet: bool) -> ProgressBar {
    if quiet {
        return ProgressBar::hidden();
    }
    let progress = ProgressBar::new(length);
    progress.set_style(
        ProgressStyle::with_template("{spinner:.cyan} [{bar:32.cyan/blue}] {pos}/{len} chunks")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    progress
}
