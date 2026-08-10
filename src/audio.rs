//! Atomic streaming WAV output.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use hound::{SampleFormat, WavSpec, WavWriter};
use tempfile::{NamedTempFile, TempPath};

pub const SAMPLE_RATE: u32 = 24_000;

/// Summary from a completed streamed WAV.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioReport {
    pub output: PathBuf,
    pub samples: u64,
    pub peak_amplitude: f32,
}

/// One atomic WAV writer that never retains prior audio chunks.
pub struct StreamingWav {
    writer: Option<WavWriter<BufWriter<File>>>,
    temporary: Option<TempPath>,
    output: PathBuf,
    samples: u64,
    peak_amplitude: f32,
}

impl StreamingWav {
    /// Create a temporary mono, 16-bit PCM, 24 kHz WAV beside `output`.
    ///
    /// # Errors
    ///
    /// Returns an error when the output directory or temporary file cannot be
    /// created.
    pub fn create(output: &Path) -> Result<Self> {
        let output_parent = output.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(output_parent).with_context(|| {
            format!(
                "failed to create output directory {}",
                output_parent.display()
            )
        })?;
        let temporary = NamedTempFile::new_in(output_parent).with_context(|| {
            format!(
                "failed to create temporary output in {}",
                output_parent.display()
            )
        })?;
        let (file, temporary) = temporary.into_parts();
        let spec = WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let writer = WavWriter::new(BufWriter::new(file), spec)
            .context("failed to start streaming WAV output")?;
        Ok(Self {
            writer: Some(writer),
            temporary: Some(temporary),
            output: output.to_path_buf(),
            samples: 0,
            peak_amplitude: 0.0,
        })
    }

    /// Append one copied worker PCM chunk.
    ///
    /// # Errors
    ///
    /// Rejects non-finite and out-of-range samples instead of clipping them.
    pub fn write_chunk(&mut self, samples: &[f32]) -> Result<()> {
        let writer = self
            .writer
            .as_mut()
            .context("WAV output is already finished")?;
        for &sample in samples {
            if !sample.is_finite() || sample.abs() >= 1.0 {
                bail!(
                    "invalid PCM sample {sample}; expected a finite value strictly between -1.0 and 1.0"
                );
            }
            self.peak_amplitude = self.peak_amplitude.max(sample.abs());
            writer
                .write_sample(float_to_i16(sample * f32::from(i16::MAX)))
                .context("failed to stream WAV sample")?;
        }
        self.samples = self
            .samples
            .checked_add(u64::try_from(samples.len()).context("audio chunk is too large")?)
            .context("audio sample count overflow")?;
        Ok(())
    }

    /// Append exact digital silence.
    ///
    /// # Errors
    ///
    /// Returns an error when the duration overflows or WAV writing fails.
    pub fn write_silence_ms(&mut self, milliseconds: u32) -> Result<()> {
        let silence_samples = u64::from(SAMPLE_RATE)
            .checked_mul(u64::from(milliseconds))
            .context("silence duration overflow")?
            / 1_000;
        let writer = self
            .writer
            .as_mut()
            .context("WAV output is already finished")?;
        for _ in 0..silence_samples {
            writer
                .write_sample(0_i16)
                .context("failed to stream WAV silence")?;
        }
        self.samples = self
            .samples
            .checked_add(silence_samples)
            .context("audio sample count overflow")?;
        Ok(())
    }

    /// Finalize and atomically move the temporary WAV to its output path.
    ///
    /// # Errors
    ///
    /// Returns an error when finalization or persistence fails.
    pub fn finish(mut self) -> Result<AudioReport> {
        self.writer
            .take()
            .context("WAV output is already finished")?
            .finalize()
            .context("failed to finalize WAV output")?;
        self.temporary
            .take()
            .context("temporary WAV is already persisted")?
            .persist(&self.output)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to save {}", self.output.display()))?;
        Ok(AudioReport {
            output: self.output,
            samples: self.samples,
            peak_amplitude: self.peak_amplitude,
        })
    }
}

#[allow(clippy::cast_possible_truncation)]
fn float_to_i16(sample: f32) -> i16 {
    sample.round() as i16
}
