//! Provider-independent synthesis, atomic caching, and timeline construction.

use std::cmp::Ordering;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::book::{SourcePosition, SourceRange};
use crate::chunk::chunk_text;
use crate::narration::{NarrationPlan, PlannedCue, PlannedPage};
use crate::timeline::{AudioCue, AudioTimeline, CueKind, TimingGranularity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsProviderIdentity {
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub language: Option<String>,
    pub max_characters: usize,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TtsRequest {
    pub text: String,
    pub speed: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TtsAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

pub trait TtsProvider {
    fn identity(&self) -> &TtsProviderIdentity;

    /// Synthesize one provider-sized request.
    ///
    /// # Errors
    ///
    /// Returns a provider, model, or audio generation error.
    fn synthesize(&mut self, request: &TtsRequest) -> Result<TtsAudio>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct SynthesisSettings {
    pub speed: f32,
    pub pause_ms: u32,
    pub max_retries: usize,
}

impl Default for SynthesisSettings {
    fn default() -> Self {
        Self {
            speed: 1.0,
            pause_ms: 120,
            max_retries: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedAudio {
    pub path: PathBuf,
    pub samples: u64,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedUnit {
    pub unit_id: String,
    pub chunks: Vec<CachedAudio>,
    pub start_sample: u64,
    pub end_sample: u64,
    pub silence_after_samples: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisResult {
    pub timeline: AudioTimeline,
    pub rendered_units: Vec<RenderedUnit>,
    pub provider_chunks: usize,
    pub cache_hits: usize,
    pub generated_chunks: usize,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentCache {
    root: PathBuf,
}

impl SegmentCache {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.wav"))
    }

    fn load(&self, key: &str) -> Option<CachedAudio> {
        let path = self.path_for(key);
        validate_cached_wav(&path)
            .ok()
            .map(|(samples, sample_rate)| CachedAudio {
                path,
                samples,
                sample_rate,
            })
    }

    fn store(&self, key: &str, audio: &TtsAudio) -> Result<CachedAudio> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create synthesis cache {}", self.root.display()))?;
        validate_audio(audio)?;
        let temporary = NamedTempFile::new_in(&self.root).with_context(|| {
            format!(
                "failed to create temporary cache file in {}",
                self.root.display()
            )
        })?;
        let (file, temporary) = temporary.into_parts();
        let spec = WavSpec {
            channels: 1,
            sample_rate: audio.sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer =
            WavWriter::new(BufWriter::new(file), spec).context("failed to create cached WAV")?;
        for &sample in &audio.samples {
            writer
                .write_sample(float_to_i16(sample * f32::from(i16::MAX)))
                .context("failed to write cached WAV")?;
        }
        writer.finalize().context("failed to finalize cached WAV")?;
        let path = self.path_for(key);
        temporary
            .persist(&path)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to persist synthesis cache {}", path.display()))?;
        Ok(CachedAudio {
            path,
            samples: u64::try_from(audio.samples.len()).context("audio sample count overflow")?,
            sample_rate: audio.sample_rate,
        })
    }
}

/// Synthesize a semantic plan with atomic cache writes and exact segment timing.
///
/// # Errors
///
/// Returns an error for invalid settings, provider failure, PCM, or cache I/O.
pub fn synthesize_plan<P: TtsProvider>(
    plan: &NarrationPlan,
    provider: &mut P,
    cache: &SegmentCache,
    settings: &SynthesisSettings,
) -> Result<SynthesisResult> {
    validate_settings(plan, provider.identity(), settings)?;
    let identity = provider.identity().clone();
    let mut rendered_units = Vec::with_capacity(plan.units.len());
    let mut unit_bounds = Vec::with_capacity(plan.units.len());
    let mut cursor = 0_u64;
    let pause_samples = u64::from(identity.sample_rate)
        .checked_mul(u64::from(settings.pause_ms))
        .context("pause duration overflow")?
        / 1_000;
    let mut provider_chunks = 0_usize;
    let mut cache_hits = 0_usize;
    let mut generated_chunks = 0_usize;

    for (unit_index, unit) in plan.units.iter().enumerate() {
        let mut chunks = Vec::new();
        for text in chunk_text(&unit.text, identity.max_characters)? {
            provider_chunks += 1;
            let request = TtsRequest {
                text,
                speed: settings.speed,
            };
            let key = cache_key(&identity, &request);
            let cached = if let Some(cached) = cache.load(&key) {
                cache_hits += 1;
                cached
            } else {
                let audio = request_with_retry(provider, &request, settings.max_retries)
                    .with_context(|| format!("failed to synthesize narration unit {}", unit.id))?;
                if audio.sample_rate != identity.sample_rate {
                    bail!(
                        "TTS provider returned {} Hz but declared {} Hz",
                        audio.sample_rate,
                        identity.sample_rate
                    );
                }
                generated_chunks += 1;
                cache.store(&key, &audio)?
            };
            cursor = cursor
                .checked_add(cached.samples)
                .context("audiobook sample count overflow")?;
            chunks.push(cached);
        }
        let start_sample = chunks.first().map_or(cursor, |_| {
            cursor - chunks.iter().map(|chunk| chunk.samples).sum::<u64>()
        });
        let end_sample = cursor;
        unit_bounds.push((start_sample, end_sample));
        let silence_after_samples = if unit_index + 1 < plan.units.len() {
            pause_samples
        } else {
            0
        };
        cursor = cursor
            .checked_add(silence_after_samples)
            .context("audiobook sample count overflow")?;
        rendered_units.push(RenderedUnit {
            unit_id: unit.id.clone(),
            chunks,
            start_sample,
            end_sample,
            silence_after_samples,
        });
    }

    let timeline = build_timeline(plan, &unit_bounds, cursor, identity.sample_rate);
    Ok(SynthesisResult {
        timeline,
        rendered_units,
        provider_chunks,
        cache_hits,
        generated_chunks,
        sample_rate: identity.sample_rate,
    })
}

fn validate_settings(
    plan: &NarrationPlan,
    identity: &TtsProviderIdentity,
    settings: &SynthesisSettings,
) -> Result<()> {
    if plan.units.is_empty() {
        bail!("narration plan contains no spoken units");
    }
    if !(0.5..=2.0).contains(&settings.speed) {
        bail!("speed must be between 0.5 and 2.0");
    }
    if settings.max_retries > 10 {
        bail!("retry count cannot exceed 10");
    }
    if identity.max_characters == 0 {
        bail!("TTS provider character limit must be greater than zero");
    }
    if identity.sample_rate == 0 {
        bail!("TTS provider sample rate must be greater than zero");
    }
    Ok(())
}

fn request_with_retry<P: TtsProvider>(
    provider: &mut P,
    request: &TtsRequest,
    max_retries: usize,
) -> Result<TtsAudio> {
    let mut last_error = None;
    for attempt in 0..=max_retries {
        match provider.synthesize(request) {
            Ok(audio) => {
                validate_audio(&audio)?;
                return Ok(audio);
            }
            Err(error) => last_error = Some((attempt, error)),
        }
    }
    let (attempt, error) = last_error.context("TTS provider did not run")?;
    Err(anyhow!(
        "TTS request failed after {} attempt(s): {error:#}",
        attempt + 1
    ))
}

fn validate_audio(audio: &TtsAudio) -> Result<()> {
    if audio.sample_rate == 0 {
        bail!("TTS provider returned a zero sample rate");
    }
    if audio.samples.is_empty() {
        bail!("TTS provider returned empty audio");
    }
    for &sample in &audio.samples {
        if !sample.is_finite() || sample.abs() >= 1.0 {
            bail!("TTS provider returned invalid PCM sample {sample}");
        }
    }
    Ok(())
}

fn cache_key(identity: &TtsProviderIdentity, request: &TtsRequest) -> String {
    let mut digest = Sha256::new();
    digest.update(b"kokoro-book-segment-cache-v1\0");
    for value in [
        identity.provider.as_str(),
        identity.model.as_str(),
        identity.voice.as_str(),
        identity.language.as_deref().unwrap_or_default(),
        request.text.as_str(),
    ] {
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    digest.update(request.speed.to_bits().to_le_bytes());
    digest.update(identity.sample_rate.to_le_bytes());
    format!("{:x}", digest.finalize())
}

fn validate_cached_wav(path: &Path) -> Result<(u64, u32)> {
    let mut reader = WavReader::open(path)
        .with_context(|| format!("failed to open cached WAV {}", path.display()))?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != SampleFormat::Int
        || spec.sample_rate == 0
    {
        bail!("cached WAV has an unsupported format");
    }
    let mut samples = 0_u64;
    for sample in reader.samples::<i16>() {
        sample.context("cached WAV contains invalid PCM")?;
        samples = samples.checked_add(1).context("cached WAV is too large")?;
    }
    if samples == 0 {
        bail!("cached WAV contains no audio");
    }
    Ok((samples, spec.sample_rate))
}

fn build_timeline(
    plan: &NarrationPlan,
    unit_bounds: &[(u64, u64)],
    duration_samples: u64,
    sample_rate: u32,
) -> AudioTimeline {
    let mut cues = plan
        .cues
        .iter()
        .enumerate()
        .filter_map(|(ordinal, cue)| {
            timeline_cue(cue, unit_bounds, sample_rate).map(|cue| (ordinal, cue))
        })
        .collect::<Vec<_>>();
    cues.extend(plan.pages.iter().enumerate().filter_map(|(index, page)| {
        page_cue(page, index, plan, unit_bounds, sample_rate)
            .map(|cue| (plan.cues.len() + index, cue))
    }));
    cues.sort_by_key(|(ordinal, cue)| (cue.start_ms, *ordinal));
    AudioTimeline {
        duration_ms: samples_to_ms(duration_samples, sample_rate),
        cues: cues.into_iter().map(|(_, cue)| cue).collect(),
    }
}

fn page_cue(
    page: &PlannedPage,
    index: usize,
    plan: &NarrationPlan,
    unit_bounds: &[(u64, u64)],
    sample_rate: u32,
) -> Option<AudioCue> {
    let unit_index = plan.units.iter().position(|unit| {
        unit.source_range
            .as_ref()
            .and_then(|range| compare_positions(&range.start, &page.position))
            .is_some_and(|ordering| ordering != Ordering::Less)
    })?;
    let start = unit_bounds.get(unit_index)?.0;
    Some(AudioCue {
        id: format!("page:{}:{}", page.label, index + 1),
        kind: CueKind::Page {
            label: page.label.clone(),
        },
        start_ms: samples_to_ms(start, sample_rate),
        end_ms: None,
        source_range: Some(SourceRange {
            source_id: page.source_id.clone(),
            start: page.position.clone(),
            end: page.position.clone(),
        }),
        section_id: plan.units[unit_index].section_id.clone(),
        timing: TimingGranularity::SegmentBoundary,
    })
}

fn compare_positions(left: &SourcePosition, right: &SourcePosition) -> Option<Ordering> {
    match (left, right) {
        (
            SourcePosition::Text { byte_offset: left },
            SourcePosition::Text { byte_offset: right },
        ) => Some(left.cmp(right)),
        (
            SourcePosition::Epub {
                resource: left_resource,
                fragment: left_fragment,
                character_offset: left_offset,
            },
            SourcePosition::Epub {
                resource: right_resource,
                fragment: right_fragment,
                character_offset: right_offset,
            },
        ) if left_resource == right_resource => match (left_offset, right_offset) {
            (Some(left), Some(right)) => Some(left.cmp(right)),
            _ if left_fragment.is_some() && left_fragment == right_fragment => {
                Some(Ordering::Equal)
            }
            _ => None,
        },
        (
            SourcePosition::Pdf {
                page_number: left_page,
                character_offset: left_offset,
            },
            SourcePosition::Pdf {
                page_number: right_page,
                character_offset: right_offset,
            },
        ) => Some(
            left_page
                .cmp(right_page)
                .then_with(|| left_offset.unwrap_or(0).cmp(&right_offset.unwrap_or(0))),
        ),
        _ => None,
    }
}

fn timeline_cue(
    cue: &PlannedCue,
    unit_bounds: &[(u64, u64)],
    sample_rate: u32,
) -> Option<AudioCue> {
    if cue.unit_end <= cue.unit_start {
        return None;
    }
    let start = unit_bounds.get(cue.unit_start)?.0;
    let end = unit_bounds.get(cue.unit_end - 1)?.1;
    Some(AudioCue {
        id: cue.id.clone(),
        kind: cue.kind.clone(),
        start_ms: samples_to_ms(start, sample_rate),
        end_ms: Some(samples_to_ms(end, sample_rate)),
        source_range: cue.source_range.clone(),
        section_id: cue.section_id.clone(),
        timing: TimingGranularity::SegmentBoundary,
    })
}

fn samples_to_ms(samples: u64, sample_rate: u32) -> u64 {
    samples.saturating_mul(1_000) / u64::from(sample_rate)
}

#[allow(clippy::cast_possible_truncation)]
fn float_to_i16(sample: f32) -> i16 {
    sample.round() as i16
}

#[derive(Debug, Clone)]
pub struct MockTtsProvider {
    identity: TtsProviderIdentity,
    requests: Vec<TtsRequest>,
    fail_on_request: Option<usize>,
}

impl MockTtsProvider {
    #[must_use]
    pub fn new(voice: &str, max_characters: usize, sample_rate: u32) -> Self {
        Self {
            identity: TtsProviderIdentity {
                provider: "mock".to_owned(),
                model: "deterministic-v1".to_owned(),
                voice: voice.to_owned(),
                language: Some("en".to_owned()),
                max_characters,
                sample_rate,
            },
            requests: Vec::new(),
            fail_on_request: None,
        }
    }

    pub fn fail_on_request(&mut self, request_number: usize) {
        self.fail_on_request = Some(request_number);
    }

    #[must_use]
    pub fn requests(&self) -> &[TtsRequest] {
        &self.requests
    }
}

impl TtsProvider for MockTtsProvider {
    fn identity(&self) -> &TtsProviderIdentity {
        &self.identity
    }

    fn synthesize(&mut self, request: &TtsRequest) -> Result<TtsAudio> {
        self.requests.push(request.clone());
        let request_number = self.requests.len();
        if self.fail_on_request == Some(request_number) {
            bail!("mock provider request {request_number} failed");
        }
        let samples = request.text.chars().count().saturating_mul(10).max(1);
        Ok(TtsAudio {
            samples: vec![0.125; samples],
            sample_rate: self.identity.sample_rate,
        })
    }
}
