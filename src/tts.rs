//! Kokoro MLX provider adapter.

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::audio::SAMPLE_RATE;
use crate::model::{MODEL_BUNDLE_NAME, MODEL_REVISION, ModelAssets};
use crate::phoneme::Pronunciation;
use crate::pipeline::phonemize_book;
use crate::synthesis::{
    PhonemeNormalizationReport, TtsAudio, TtsInputMode, TtsProvider, TtsProviderDiagnostics,
    TtsProviderIdentity, TtsRequest,
};
use crate::vocab;
use crate::voice::Voice;
use crate::worker::{
    ChunkWorker, ProcessWorker, WorkerLaunch, WorkerLimits, WorkerStats,
    synthesize_with_split_retry,
};

pub const DEFAULT_MAX_PHONEMES: usize = 200;
pub const DEFAULT_PROVIDER_MAX_CHARACTERS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct KokoroProviderReport {
    pub(crate) worker_requests: usize,
    pub(crate) worker_restarts: usize,
    pub(crate) model_load_seconds: f64,
    pub(crate) synthesis_seconds: f64,
    pub(crate) memory: WorkerStats,
}

pub(crate) struct KokoroTtsProvider<W = ProcessWorker> {
    identity: TtsProviderIdentity,
    worker: Option<W>,
    voice: Voice,
    pronunciations: Vec<Pronunciation>,
    max_phonemes: usize,
    synthesis_seconds: f64,
    phoneme_normalization: PhonemeNormalizationReport,
}

impl KokoroTtsProvider<ProcessWorker> {
    pub(crate) fn launch(
        assets: &ModelAssets,
        voice: Voice,
        pronunciations: &[Pronunciation],
        max_phonemes: usize,
        limits: WorkerLimits,
    ) -> Result<Self> {
        validate_settings(1.0, max_phonemes)?;
        let worker = ProcessWorker::launch(WorkerLaunch {
            model_dir: assets.root.clone(),
            voice_file: assets.voice.clone(),
            limits,
        })?;
        Ok(Self::new(worker, voice, pronunciations, max_phonemes))
    }

    pub(crate) fn finish(mut self) -> Result<KokoroProviderReport> {
        let worker = self.worker.take().context("MLX worker is unavailable")?;
        let report = KokoroProviderReport {
            worker_requests: worker.requests(),
            worker_restarts: worker.restarts(),
            model_load_seconds: worker.total_model_load_seconds(),
            synthesis_seconds: self.synthesis_seconds,
            memory: worker.latest_stats(),
        };
        worker.finish()?;
        Ok(report)
    }
}

impl<W: ChunkWorker> KokoroTtsProvider<W> {
    fn new(worker: W, voice: Voice, pronunciations: &[Pronunciation], max_phonemes: usize) -> Self {
        let overrides = pronunciations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        Self {
            identity: TtsProviderIdentity {
                provider: "kokoro-mlx".to_owned(),
                model: format!("{MODEL_BUNDLE_NAME}@{MODEL_REVISION}"),
                voice: voice.name().to_owned(),
                language: Some(if voice.is_british() { "en-GB" } else { "en-US" }.to_owned()),
                configuration_hash: configuration_hash(voice, &overrides, max_phonemes),
                max_characters: DEFAULT_PROVIDER_MAX_CHARACTERS,
                sample_rate: SAMPLE_RATE,
            },
            worker: Some(worker),
            voice,
            pronunciations: pronunciations.to_vec(),
            max_phonemes,
            synthesis_seconds: 0.0,
            phoneme_normalization: PhonemeNormalizationReport::default(),
        }
    }
}

impl<W: ChunkWorker> TtsProvider for KokoroTtsProvider<W> {
    fn identity(&self) -> &TtsProviderIdentity {
        &self.identity
    }

    fn input_mode(&self) -> TtsInputMode {
        TtsInputMode::PreparedPhonemes
    }

    fn diagnostics(&self) -> TtsProviderDiagnostics {
        TtsProviderDiagnostics {
            phoneme_normalization: self.phoneme_normalization.clone(),
        }
    }

    fn synthesize(&mut self, request: &TtsRequest) -> Result<TtsAudio> {
        let chunks = if let Some(prepared) = request.phoneme_chunks.as_ref() {
            for chunk in prepared {
                vocab::token_ids(chunk)?;
            }
            prepared.clone()
        } else {
            let phonemization = phonemize_book(
                &request.text,
                self.voice,
                &self.pronunciations,
                self.max_phonemes,
            )?;
            self.phoneme_normalization.automatic_repairs +=
                phonemization.normalization.automatic_repairs;
            self.phoneme_normalization.syllabic_consonant +=
                phonemization.normalization.syllabic_consonant;
            phonemization.chunks
        };
        let worker = self.worker.as_mut().context("MLX worker is unavailable")?;
        let mut samples = Vec::new();
        for chunk in chunks {
            for audio in synthesize_with_split_retry(worker, &chunk, request.speed)? {
                self.synthesis_seconds += audio.synthesis_seconds;
                samples
                    .try_reserve(audio.samples.len())
                    .context("Kokoro audio response is too large")?;
                samples.extend_from_slice(&audio.samples);
            }
        }
        if samples.is_empty() {
            bail!("Kokoro returned no audio samples");
        }
        Ok(TtsAudio {
            samples,
            sample_rate: SAMPLE_RATE,
        })
    }
}

pub(crate) fn configuration_hash(
    voice: Voice,
    overrides: &[String],
    max_phonemes: usize,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"audiobook-forge-provider-v1\0");
    digest.update(crate::vocab::PHONEME_NORMALIZATION_VERSION.to_le_bytes());
    digest.update(MODEL_REVISION.as_bytes());
    digest.update([0]);
    digest.update(voice.name().as_bytes());
    digest.update([0]);
    digest.update(max_phonemes.to_le_bytes());
    for value in overrides {
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    format!("{:x}", digest.finalize())
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

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::{KokoroTtsProvider, TtsProvider, TtsRequest};
    use crate::phoneme::Pronunciation;
    use crate::voice::Voice;
    use crate::worker::{ChunkAudio, ChunkWorker};

    #[derive(Default)]
    struct RecordingWorker {
        requests: Vec<String>,
    }

    impl ChunkWorker for RecordingWorker {
        fn synthesize(&mut self, phonemes: &str, _speed: f32) -> Result<ChunkAudio> {
            self.requests.push(phonemes.to_owned());
            Ok(ChunkAudio::from_samples(vec![0.125; phonemes.len().max(1)]))
        }

        fn restart(&mut self) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn provider_phonemizes_semantic_text_and_returns_pcm() {
        let voice: Voice = "af_heart".parse().expect("voice");
        let mut provider = KokoroTtsProvider::new(RecordingWorker::default(), voice, &[], 200);

        let audio = provider
            .synthesize(&TtsRequest {
                text: "Hello world.".to_owned(),
                speed: 1.0,
                phoneme_chunks: None,
            })
            .expect("audio");

        assert_eq!(audio.sample_rate, 24_000);
        assert!(!audio.samples.is_empty());
        assert_eq!(provider.worker.expect("worker").requests.len(), 1);
    }

    #[test]
    fn provider_reports_silent_syllabic_consonant_repairs() {
        let voice: Voice = "af_heart".parse().expect("voice");
        let mut provider = KokoroTtsProvider::new(RecordingWorker::default(), voice, &[], 200);

        provider
            .synthesize(&TtsRequest {
                text: "Written and certain.".to_owned(),
                speed: 1.0,
                phoneme_chunks: None,
            })
            .expect("audio");

        let report = provider.diagnostics().phoneme_normalization;
        assert!(report.automatic_repairs >= 2);
        assert_eq!(report.automatic_repairs, report.syllabic_consonant);
    }

    #[test]
    fn cache_identity_changes_with_pronunciation_and_chunk_policy() {
        let voice: Voice = "af_heart".parse().expect("voice");
        let override_value: Pronunciation = "Elena=ɪlˈeɪnə".parse().expect("override");
        let baseline = KokoroTtsProvider::new(RecordingWorker::default(), voice, &[], 200);
        let overridden =
            KokoroTtsProvider::new(RecordingWorker::default(), voice, &[override_value], 200);
        let smaller = KokoroTtsProvider::new(RecordingWorker::default(), voice, &[], 100);

        assert_ne!(
            baseline.identity().configuration_hash,
            overridden.identity().configuration_hash
        );
        assert_ne!(
            baseline.identity().configuration_hash,
            smaller.identity().configuration_hash
        );
    }
}
