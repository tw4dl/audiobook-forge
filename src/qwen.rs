//! Optional Qwen3-TTS MLX provider and isolated Python worker protocol.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::synthesis::{TtsAudio, TtsInputMode, TtsProvider, TtsProviderIdentity, TtsRequest};

pub(crate) const DEFAULT_QWEN_VOICE: &str = "Aiden";
pub(crate) const QWEN_MODEL: &str = "mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-6bit";
pub(crate) const QWEN_MODEL_REVISION: &str = "7dc92af14613355896fcab13b268c19ede233139";
pub(crate) const QWEN_RUNTIME_COMMIT: &str = "49596ac8b69b9ed377db311a73df838795f38a3d";
pub(crate) const QWEN_SAMPLE_RATE: u32 = 24_000;
pub(crate) const QWEN_MAX_CHARACTERS: usize = 280;

const QWEN_WORKER_SOURCE: &str = include_str!("qwen_worker.py");
const QWEN_MIN_GENERATION_TOKENS: usize = 96;
const QWEN_MAX_GENERATION_TOKENS: usize = 640;
const QWEN_GENERATION_ATTEMPTS: usize = 2;
const MAX_RESPONSE_SAMPLES: usize = 24_000 * 60;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct QwenProviderReport {
    pub(crate) worker_requests: usize,
    pub(crate) worker_restarts: usize,
    pub(crate) generation_retries: usize,
    pub(crate) model_load_seconds: f64,
    pub(crate) synthesis_seconds: f64,
    pub(crate) peak_memory_bytes: u64,
}

#[derive(Debug, Clone)]
struct QwenWorkerLaunch {
    python: PathBuf,
    voice: String,
}

pub(crate) struct QwenTtsProvider {
    identity: TtsProviderIdentity,
    worker: Option<QwenProcessWorker>,
}

impl QwenTtsProvider {
    pub(crate) fn launch(python: PathBuf, voice: &str) -> Result<Self> {
        ensure_supported_platform()?;
        let voice = canonical_voice(voice)?;
        let worker = QwenProcessWorker::launch(QwenWorkerLaunch {
            python,
            voice: voice.clone(),
        })?;
        Ok(Self {
            identity: TtsProviderIdentity {
                provider: "qwen".to_owned(),
                model: format!("{QWEN_MODEL}@{QWEN_MODEL_REVISION}"),
                voice: voice.clone(),
                language: Some("en".to_owned()),
                configuration_hash: configuration_hash(&voice),
                max_characters: QWEN_MAX_CHARACTERS,
                sample_rate: QWEN_SAMPLE_RATE,
            },
            worker: Some(worker),
        })
    }

    pub(crate) fn finish(mut self) -> Result<QwenProviderReport> {
        let worker = self.worker.take().context("Qwen worker is unavailable")?;
        let report = worker.report();
        worker.finish()?;
        Ok(report)
    }
}

impl TtsProvider for QwenTtsProvider {
    fn identity(&self) -> &TtsProviderIdentity {
        &self.identity
    }

    fn input_mode(&self) -> TtsInputMode {
        TtsInputMode::RawText
    }

    fn synthesize(&mut self, request: &TtsRequest) -> Result<TtsAudio> {
        if request.phoneme_chunks.is_some() {
            bail!("Qwen provider accepts normalized text, not Kokoro phonemes");
        }
        validate_speed(request.speed)?;
        let worker = self.worker.as_mut().context("Qwen worker is unavailable")?;
        match worker.synthesize(&request.text, request.speed) {
            Ok(samples) => Ok(TtsAudio {
                samples,
                sample_rate: QWEN_SAMPLE_RATE,
            }),
            Err(error) => {
                worker.restart().with_context(|| {
                    format!("Qwen request failed ({error:#}) and worker restart failed")
                })?;
                Err(error.context("Qwen request failed; worker restarted"))
            }
        }
    }
}

pub(crate) fn default_python(cache_root: &Path) -> PathBuf {
    std::env::var_os("KOKORO_BOOK_QWEN_PYTHON")
        .map_or_else(|| cache_root.join("qwen-runtime/bin/python"), PathBuf::from)
}

pub(crate) fn validate_speed(speed: f32) -> Result<()> {
    if (speed - 1.0).abs() > f32::EPSILON {
        bail!("Qwen provider currently supports only --speed 1.0");
    }
    Ok(())
}

fn canonical_voice(voice: &str) -> Result<String> {
    ["Aiden", "Ryan"]
        .into_iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(voice))
        .map(str::to_owned)
        .with_context(|| "Qwen English voice must be Aiden or Ryan")
}

fn configuration_hash(voice: &str) -> String {
    let mut digest = Sha256::new();
    for value in [
        "kokoro-book-qwen-v2",
        QWEN_RUNTIME_COMMIT,
        QWEN_MODEL,
        QWEN_MODEL_REVISION,
        voice,
        "English",
        "temperature=0.9",
        "top_k=50",
        "top_p=1",
        "repetition_penalty=1.05",
        "max_tokens=clamp(characters*2,96,640)",
        "seed=sha256(text,attempt)",
        "generation_attempts=2",
        "speed=1",
        "max_characters=280",
    ] {
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    format!("{:x}", digest.finalize())
}

fn generation_max_tokens(text: &str) -> usize {
    text.chars()
        .count()
        .saturating_mul(2)
        .clamp(QWEN_MIN_GENERATION_TOKENS, QWEN_MAX_GENERATION_TOKENS)
}

fn generation_seed(text: &str, attempt: u32) -> u32 {
    let mut digest = Sha256::new();
    digest.update(b"kokoro-book-qwen-seed-v1\0");
    digest.update(text.as_bytes());
    digest.update([0]);
    digest.update(attempt.to_le_bytes());
    let bytes: [u8; 4] = digest.finalize()[..4]
        .try_into()
        .expect("SHA-256 prefix is four bytes");
    u32::from_le_bytes(bytes)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(clippy::unnecessary_wraps)]
fn ensure_supported_platform() -> Result<()> {
    Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn ensure_supported_platform() -> Result<()> {
    bail!("Qwen provider requires Apple Silicon macOS")
}

#[derive(Debug, Serialize)]
struct WorkerRequest<'a> {
    text: &'a str,
    speed: f32,
    max_tokens: usize,
    seeds: [u32; QWEN_GENERATION_ATTEMPTS],
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum WorkerHeader {
    Ready {
        model_load_seconds: f64,
        peak_bytes: u64,
        sample_rate: u32,
    },
    Audio {
        sample_count: usize,
        sample_rate: u32,
        synthesis_seconds: f64,
        peak_bytes: u64,
        generation_attempts: usize,
    },
    Error {
        message: String,
    },
}

struct QwenProcessWorker {
    launch: QwenWorkerLaunch,
    child: Option<Child>,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    model_load_seconds: f64,
    synthesis_seconds: f64,
    peak_memory_bytes: u64,
    requests: usize,
    restarts: usize,
    generation_retries: usize,
}

impl QwenProcessWorker {
    fn launch(launch: QwenWorkerLaunch) -> Result<Self> {
        if !launch.python.is_file() {
            bail!(
                "Qwen Python runtime not found at {}; run scripts/setup-qwen-runtime.sh or set KOKORO_BOOK_QWEN_PYTHON",
                launch.python.display()
            );
        }
        let mut child = Command::new(&launch.python)
            .arg("-u")
            .arg("-c")
            .arg(QWEN_WORKER_SOURCE)
            .arg("--model")
            .arg(QWEN_MODEL)
            .arg("--revision")
            .arg(QWEN_MODEL_REVISION)
            .arg("--voice")
            .arg(&launch.voice)
            .arg("--language")
            .arg("English")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to launch Qwen Python runtime {}; set KOKORO_BOOK_QWEN_PYTHON",
                    launch.python.display()
                )
            })?;
        let input = child
            .stdin
            .take()
            .context("Qwen worker stdin is unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("Qwen worker stdout is unavailable")?;
        let mut output = BufReader::new(stdout);
        match read_header(&mut output).context("Qwen worker did not become ready")? {
            WorkerHeader::Ready {
                model_load_seconds,
                peak_bytes,
                sample_rate,
            } => {
                if sample_rate != QWEN_SAMPLE_RATE {
                    terminate_child(&mut child);
                    bail!("Qwen worker declared {sample_rate} Hz; expected {QWEN_SAMPLE_RATE} Hz");
                }
                Ok(Self {
                    launch,
                    child: Some(child),
                    input: Some(input),
                    output,
                    model_load_seconds,
                    synthesis_seconds: 0.0,
                    peak_memory_bytes: peak_bytes,
                    requests: 0,
                    restarts: 0,
                    generation_retries: 0,
                })
            }
            WorkerHeader::Error { message } => {
                let _ = child.wait();
                bail!("Qwen worker initialization failed: {message}");
            }
            WorkerHeader::Audio { .. } => {
                terminate_child(&mut child);
                bail!("Qwen worker sent audio before its ready frame");
            }
        }
    }

    fn synthesize(&mut self, text: &str, speed: f32) -> Result<Vec<f32>> {
        self.requests += 1;
        let input = self.input.as_mut().context("Qwen worker stdin is closed")?;
        let max_tokens = generation_max_tokens(text);
        let seeds = [generation_seed(text, 0), generation_seed(text, 1)];
        serde_json::to_writer(
            &mut *input,
            &WorkerRequest {
                text,
                speed,
                max_tokens,
                seeds,
            },
        )
        .context("failed to encode Qwen worker request")?;
        input
            .write_all(b"\n")
            .context("failed to frame Qwen worker request")?;
        input
            .flush()
            .context("failed to send Qwen worker request")?;

        match read_header(&mut self.output).context("failed to read Qwen worker response")? {
            WorkerHeader::Audio {
                sample_count,
                sample_rate,
                synthesis_seconds,
                peak_bytes,
                generation_attempts,
            } => {
                if sample_rate != QWEN_SAMPLE_RATE {
                    bail!("Qwen worker returned {sample_rate} Hz; expected {QWEN_SAMPLE_RATE} Hz");
                }
                if sample_count == 0 || sample_count > MAX_RESPONSE_SAMPLES {
                    bail!("Qwen worker returned invalid sample count {sample_count}");
                }
                let byte_count = sample_count
                    .checked_mul(size_of::<f32>())
                    .context("Qwen PCM byte count overflow")?;
                let mut bytes = vec![0_u8; byte_count];
                self.output
                    .read_exact(&mut bytes)
                    .context("Qwen worker returned truncated PCM")?;
                let (chunks, remainder) = bytes.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                let samples = chunks
                    .iter()
                    .map(|chunk| f32::from_le_bytes(*chunk))
                    .collect::<Vec<_>>();
                self.synthesis_seconds += synthesis_seconds;
                self.peak_memory_bytes = self.peak_memory_bytes.max(peak_bytes);
                self.generation_retries += generation_attempts.saturating_sub(1);
                Ok(samples)
            }
            WorkerHeader::Error { message } => bail!("Qwen worker failed: {message}"),
            WorkerHeader::Ready { .. } => bail!("Qwen worker sent a duplicate ready frame"),
        }
    }

    fn restart(&mut self) -> Result<()> {
        self.terminate();
        let previous_load = self.model_load_seconds;
        let previous_synthesis = self.synthesis_seconds;
        let previous_peak = self.peak_memory_bytes;
        let previous_requests = self.requests;
        let previous_restarts = self.restarts;
        let previous_generation_retries = self.generation_retries;
        let mut replacement = Self::launch(self.launch.clone())?;
        replacement.model_load_seconds += previous_load;
        replacement.synthesis_seconds = previous_synthesis;
        replacement.peak_memory_bytes = replacement.peak_memory_bytes.max(previous_peak);
        replacement.requests = previous_requests;
        replacement.restarts = previous_restarts + 1;
        replacement.generation_retries = previous_generation_retries;
        *self = replacement;
        Ok(())
    }

    fn report(&self) -> QwenProviderReport {
        QwenProviderReport {
            worker_requests: self.requests,
            worker_restarts: self.restarts,
            generation_retries: self.generation_retries,
            model_load_seconds: self.model_load_seconds,
            synthesis_seconds: self.synthesis_seconds,
            peak_memory_bytes: self.peak_memory_bytes,
        }
    }

    fn finish(mut self) -> Result<()> {
        self.input.take();
        let status = self
            .child
            .take()
            .context("Qwen worker process is unavailable")?
            .wait()
            .context("failed to wait for Qwen worker")?;
        if !status.success() {
            bail!("Qwen worker exited with {status}");
        }
        Ok(())
    }

    fn terminate(&mut self) {
        self.input.take();
        if let Some(mut child) = self.child.take() {
            terminate_child(&mut child);
        }
    }
}

impl Drop for QwenProcessWorker {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn terminate_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn read_header(reader: &mut BufReader<ChildStdout>) -> Result<WorkerHeader> {
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .context("failed to read Qwen worker header")?;
    if bytes == 0 {
        bail!("Qwen worker closed its output");
    }
    serde_json::from_str(&line).map_err(|error| anyhow!("invalid Qwen worker header: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_voice, configuration_hash, generation_max_tokens, generation_seed, validate_speed,
    };

    #[test]
    fn canonicalizes_supported_english_voices() {
        assert_eq!(canonical_voice("aiden").expect("voice"), "Aiden");
        assert_eq!(canonical_voice("RYAN").expect("voice"), "Ryan");
        assert!(canonical_voice("af_heart").is_err());
    }

    #[test]
    fn configuration_separates_voice_and_rejects_speed_changes() {
        assert_ne!(configuration_hash("Aiden"), configuration_hash("Ryan"));
        validate_speed(1.0).expect("default speed");
        assert!(validate_speed(1.1).is_err());
    }

    #[test]
    fn generation_token_budget_scales_with_text_and_stays_bounded() {
        assert_eq!(generation_max_tokens("short"), 96);
        assert_eq!(generation_max_tokens(&"a".repeat(190)), 380);
        assert_eq!(generation_max_tokens(&"a".repeat(400)), 640);
    }

    #[test]
    fn generation_seed_is_stable_but_changes_for_the_fallback_attempt() {
        assert_eq!(
            generation_seed("same text", 0),
            generation_seed("same text", 0)
        );
        assert_ne!(
            generation_seed("same text", 0),
            generation_seed("same text", 1)
        );
        assert_ne!(
            generation_seed("same text", 0),
            generation_seed("other text", 0)
        );
    }
}
