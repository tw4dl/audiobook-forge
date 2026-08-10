//! Kokoro MLX worker orchestration and streamed audiobook output.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};

use crate::audio::{SAMPLE_RATE, StreamingWav};
use crate::model::ModelAssets;
use crate::phoneme::Pronunciation;
use crate::pipeline::phonemize_book;
use crate::vocab;
use crate::voice::Voice;
use crate::worker::{
    ProcessWorker, WorkerLaunch, WorkerLimits, WorkerStats, synthesize_with_split_retry,
};

const SILENCE_MS: u32 = 120;
pub const DEFAULT_MAX_PHONEMES: usize = 200;

#[derive(Debug, Clone)]
pub(crate) struct SynthesisReport {
    pub(crate) output: PathBuf,
    pub(crate) chunks: usize,
    pub(crate) worker_requests: usize,
    pub(crate) worker_restarts: usize,
    pub(crate) model_load_seconds: f64,
    pub(crate) synthesis_seconds: f64,
    pub(crate) audio_seconds: f64,
    pub(crate) rtf: f64,
    pub(crate) peak_amplitude: f32,
    pub(crate) memory: WorkerStats,
}

pub(crate) struct SynthesisOptions<'a> {
    pub(crate) text: &'a str,
    pub(crate) output: &'a Path,
    pub(crate) voice: Voice,
    pub(crate) pronunciations: &'a [Pronunciation],
    pub(crate) speed: f32,
    pub(crate) max_phonemes: usize,
    pub(crate) quiet: bool,
    pub(crate) worker_limits: WorkerLimits,
}

/// Check synthesis settings before model setup.
///
/// # Errors
///
/// Returns an error when speed or the phoneme limit is outside its valid range.
pub fn validate_settings(speed: f32, max_phonemes: usize) -> Result<()> {
    if !(0.5..=2.0).contains(&speed) {
        bail!("speed must be between 0.5 and 2.0");
    }
    if max_phonemes == 0 {
        bail!("phoneme limit must be greater than zero");
    }
    if max_phonemes > vocab::MAX_PHONEMES {
        bail!("phoneme limit cannot exceed {}", vocab::MAX_PHONEMES);
    }
    Ok(())
}

/// Confirm that the native MLX runtime is available before any model download.
///
/// # Errors
///
/// Returns an error outside Apple Silicon macOS.
pub fn ensure_supported_platform() -> Result<()> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok(())
    } else {
        bail!("Kokoro MLX requires Apple Silicon macOS")
    }
}

/// Load Kokoro once in an isolated worker, synthesize bounded phoneme chunks,
/// and atomically stream one WAV.
///
/// # Errors
///
/// Returns an error for G2P, worker, memory, PCM, or output failures.
pub(crate) fn synthesize_to_wav(
    assets: &ModelAssets,
    options: &SynthesisOptions<'_>,
) -> Result<SynthesisReport> {
    validate_settings(options.speed, options.max_phonemes)?;
    ensure_supported_platform()?;
    let chunks = phonemize_book(
        options.text,
        options.voice,
        options.pronunciations,
        options.max_phonemes,
    )?;
    if chunks.is_empty() {
        bail!("input contains no readable text");
    }

    let launch = WorkerLaunch {
        model_dir: assets.root.clone(),
        voice_file: assets.voice.clone(),
        limits: options.worker_limits,
    };
    let mut worker = ProcessWorker::launch(launch)?;
    let mut output = StreamingWav::create(options.output)?;
    let progress = synthesis_progress(chunks.len() as u64, options.quiet);
    let mut synthesis_seconds = 0.0;
    let mut speech_samples = 0_u64;
    let mut memory = worker.latest_stats();

    for (index, chunk) in chunks.iter().enumerate() {
        let generated = synthesize_with_split_retry(&mut worker, chunk, options.speed)?;
        for audio in generated {
            synthesis_seconds += audio.synthesis_seconds;
            speech_samples = speech_samples
                .checked_add(
                    u64::try_from(audio.samples.len()).context("audio chunk is too large")?,
                )
                .context("audiobook sample count overflow")?;
            memory.active_bytes = audio.stats.active_bytes;
            memory.cached_bytes = audio.stats.cached_bytes;
            memory.peak_bytes = memory.peak_bytes.max(audio.stats.peak_bytes);
            output.write_chunk(&audio.samples)?;
        }
        if index + 1 < chunks.len() {
            output.write_silence_ms(SILENCE_MS)?;
        }
        progress.inc(1);
    }
    progress.finish_and_clear();

    let worker_requests = worker.requests();
    let worker_restarts = worker.restarts();
    let model_load_seconds = worker.total_model_load_seconds();
    worker.finish()?;
    let audio = output.finish()?;
    let audio_seconds = samples_to_seconds(audio.samples);
    let speech_seconds = samples_to_seconds(speech_samples);
    if speech_seconds == 0.0 {
        bail!("Kokoro returned no audio samples");
    }

    Ok(SynthesisReport {
        output: audio.output,
        chunks: chunks.len(),
        worker_requests,
        worker_restarts,
        model_load_seconds,
        synthesis_seconds,
        audio_seconds,
        rtf: synthesis_seconds / speech_seconds,
        peak_amplitude: audio.peak_amplitude,
        memory,
    })
}

#[allow(clippy::cast_precision_loss)]
fn samples_to_seconds(sample_count: u64) -> f64 {
    sample_count as f64 / f64::from(SAMPLE_RATE)
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
