//! Kokoro synthesis and atomic WAV output.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use hound::{SampleFormat, WavSpec, WavWriter};
use indicatif::{ProgressBar, ProgressStyle};
use ndarray::Array;
use ort::inputs;
use ort::session::Session;
use ort::value::TensorRef;
use tempfile::NamedTempFile;

use crate::chunk::chunk_text;
use crate::model::ModelAssets;
use crate::phoneme::{Phonemizer, Pronunciation};
use crate::vocab;
use crate::voice::Voice;

const SAMPLE_RATE: u32 = 24_000;
const SILENCE_MS: u32 = 120;
const STYLE_SIZE: usize = 256;
const STYLE_FRAME_BYTES: usize = STYLE_SIZE * size_of::<f32>();

#[derive(Debug, Clone)]
pub(crate) struct SynthesisReport {
    pub(crate) output: PathBuf,
    pub(crate) chunks: usize,
    pub(crate) model_load_seconds: f64,
    pub(crate) synthesis_seconds: f64,
    pub(crate) audio_seconds: f64,
    pub(crate) rtf: f64,
}

pub(crate) struct SynthesisOptions<'a> {
    pub(crate) text: &'a str,
    pub(crate) output: &'a Path,
    pub(crate) voice: Voice,
    pub(crate) pronunciations: &'a [Pronunciation],
    pub(crate) speed: f32,
    pub(crate) chunk_chars: usize,
    pub(crate) threads: i32,
    pub(crate) quiet: bool,
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
/// Returns an error for invalid options, phonemes, model output, or file output.
pub(crate) fn synthesize_to_wav(
    assets: &ModelAssets,
    options: &SynthesisOptions<'_>,
) -> Result<SynthesisReport> {
    validate_settings(options.speed, options.chunk_chars, options.threads)?;
    let chunks = chunk_text(options.text, options.chunk_chars)?;
    if chunks.is_empty() {
        bail!("input contains no readable text");
    }

    let load_started = Instant::now();
    let mut engine = Engine::new(assets, options.threads)?;
    let phonemizer = Phonemizer::new(options.voice.is_british(), options.pronunciations);
    let model_load_seconds = load_started.elapsed().as_secs_f64();

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
            let phonemes = phonemizer
                .phonemize(chunk)
                .with_context(|| format!("failed to phonemize chunk {}", index + 1))?;
            let tokens = vocab::token_ids(&phonemes)
                .with_context(|| format!("invalid phonemes in chunk {}", index + 1))?;
            let started = Instant::now();
            let generated = engine
                .generate(&tokens, options.speed)
                .with_context(|| format!("Kokoro failed to synthesize chunk {}", index + 1))?;
            let elapsed = started.elapsed().as_secs_f64();
            if generated.is_empty() {
                bail!("Kokoro returned empty audio for chunk {}", index + 1);
            }
            let sample_count =
                u64::try_from(generated.len()).context("generated chunk is too large")?;
            synthesis_seconds += elapsed;
            generated_samples += sample_count;
            write_samples(&mut writer, &generated)?;

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

struct Engine {
    session: Session,
    styles: Vec<f32>,
}

impl Engine {
    fn new(assets: &ModelAssets, threads: i32) -> Result<Self> {
        let threads = usize::try_from(threads).context("thread count is too large")?;
        let session = Session::builder()
            .context("failed to configure ONNX Runtime")?
            .with_intra_threads(threads)
            .context("failed to set ONNX Runtime threads")?
            .commit_from_file(&assets.model)
            .with_context(|| format!("failed to load Kokoro model {}", assets.model.display()))?;
        let styles = read_voice(&assets.voice)?;
        Ok(Self { session, styles })
    }

    fn generate(&mut self, tokens: &[i64], speed: f32) -> Result<Vec<f32>> {
        let phoneme_count = tokens
            .len()
            .checked_sub(2)
            .ok_or_else(|| anyhow!("invalid padded token sequence"))?;
        let style = self.style(phoneme_count)?;
        let input_ids = Array::from_shape_vec((1, tokens.len()), tokens.to_vec())?;
        let style = Array::from_shape_vec((1, STYLE_SIZE), style)?;
        let speed = Array::from_vec(vec![speed]);
        let outputs = self.session.run(inputs![
            "input_ids" => TensorRef::from_array_view(&input_ids)?,
            "style" => TensorRef::from_array_view(&style)?,
            "speed" => TensorRef::from_array_view(&speed)?,
        ])?;
        let (_, waveform) = outputs["waveform"].try_extract_tensor::<f32>()?;
        Ok(waveform.to_vec())
    }

    fn style(&self, phoneme_count: usize) -> Result<Vec<f32>> {
        style_frame(&self.styles, phoneme_count)
    }
}

fn style_frame(styles: &[f32], phoneme_count: usize) -> Result<Vec<f32>> {
    let frames = styles.len() / STYLE_SIZE;
    if phoneme_count >= frames {
        bail!("voice has {frames} style frames but phoneme sequence needs index {phoneme_count}");
    }
    let start = phoneme_count * STYLE_SIZE;
    Ok(styles[start..start + STYLE_SIZE].to_vec())
}

fn read_voice(path: &Path) -> Result<Vec<f32>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.is_empty() || bytes.len() % STYLE_FRAME_BYTES != 0 {
        bail!("invalid Kokoro voice file: {}", path.display());
    }
    Ok(bytes
        .as_chunks::<{ size_of::<f32>() }>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect())
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
    sample_count as f64 / f64::from(SAMPLE_RATE)
}

#[allow(clippy::cast_possible_truncation)]
fn float_to_i16(sample: f32) -> i16 {
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use hound::{SampleFormat, WavSpec, WavWriter};
    use tempfile::tempdir;

    use super::{SAMPLE_RATE, STYLE_SIZE, read_voice, style_frame, write_samples};

    #[test]
    fn rejects_a_malformed_voice_file() {
        let temp = tempdir().expect("temp dir");
        let path = temp.path().join("voice.bin");
        std::fs::write(&path, [0_u8; 3]).expect("voice fixture");

        let error = read_voice(&path).expect_err("malformed voice must fail");

        assert!(error.to_string().contains("invalid Kokoro voice file"));
    }

    #[test]
    fn rejects_a_style_index_outside_the_voice() {
        let styles = vec![0.0; STYLE_SIZE * 2];

        assert!(style_frame(&styles, 1).is_ok());
        let error = style_frame(&styles, 2).expect_err("out-of-range style must fail");

        assert!(error.to_string().contains("needs index 2"));
    }

    #[test]
    fn rejects_non_finite_audio() {
        let spec = WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::new(Cursor::new(Vec::new()), spec).expect("WAV writer");

        let error = write_samples(&mut writer, &[f32::NAN]).expect_err("NaN must fail");

        assert!(error.to_string().contains("non-finite audio"));
    }
}
