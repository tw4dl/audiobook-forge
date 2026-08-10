//! Framed worker protocol and one-shot split recovery.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command as ProcessCommand, Stdio};

use anyhow::{Context, Result, anyhow, bail};

mod native;
mod protocol;

pub use protocol::{read_request, read_response, write_request, write_response};

/// Fixed worker memory policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerLimits {
    pub cache_limit_bytes: u64,
    pub cached_threshold_bytes: u64,
    pub memory_limit_bytes: u64,
}

impl Default for WorkerLimits {
    fn default() -> Self {
        Self {
            cache_limit_bytes: 64 * 1_024 * 1_024,
            cached_threshold_bytes: 1_024 * 1_024,
            memory_limit_bytes: 4 * 1_024 * 1_024 * 1_024,
        }
    }
}

/// Files and memory policy used to launch the isolated worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerLaunch {
    pub model_dir: PathBuf,
    pub voice_file: PathBuf,
    pub limits: WorkerLimits,
}

/// One bounded synthesis request sent to the MLX worker.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerRequest {
    pub phonemes: String,
    pub speed: f32,
}

/// Memory values observed after one worker request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkerStats {
    pub active_bytes: u64,
    pub cached_bytes: u64,
    pub peak_bytes: u64,
}

/// One response from the MLX worker.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerResponse {
    Ready {
        model_load_seconds: f64,
        stats: WorkerStats,
    },
    Audio {
        samples: Vec<f32>,
        synthesis_seconds: f64,
        stats: WorkerStats,
    },
    Error {
        message: String,
    },
}

/// PCM and measurements for one successful request.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkAudio {
    pub samples: Vec<f32>,
    pub synthesis_seconds: f64,
    pub stats: WorkerStats,
}

impl ChunkAudio {
    /// Build an unmeasured chunk. Useful for process-boundary substitutes.
    pub fn from_samples(samples: Vec<f32>) -> Self {
        Self {
            samples,
            synthesis_seconds: 0.0,
            stats: WorkerStats::default(),
        }
    }
}

/// Narrow process boundary used by the streaming pipeline.
pub trait ChunkWorker {
    /// Synthesize one already-validated phoneme chunk.
    ///
    /// # Errors
    ///
    /// Returns a worker, protocol, generation, or memory-health failure.
    fn synthesize(&mut self, phonemes: &str, speed: f32) -> Result<ChunkAudio>;

    /// Replace the worker process and reload its cached model.
    ///
    /// # Errors
    ///
    /// Returns an error when the old process cannot be replaced by a ready one.
    fn restart(&mut self) -> Result<()>;
}

/// One owned, sequential MLX subprocess.
pub struct ProcessWorker {
    launch: WorkerLaunch,
    child: Option<Child>,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    total_model_load_seconds: f64,
    restarts: usize,
    requests: usize,
    latest_stats: WorkerStats,
}

impl ProcessWorker {
    /// Launch the current executable in hidden worker mode and wait for model
    /// readiness.
    ///
    /// # Errors
    ///
    /// Returns an error when the executable, process pipes, model, or ready
    /// frame fails.
    pub fn launch(launch: WorkerLaunch) -> Result<Self> {
        let executable = std::env::current_exe().context("cannot locate kokoro-book executable")?;
        Self::launch_with_executable(launch, &executable)
    }

    fn launch_with_executable(launch: WorkerLaunch, executable: &Path) -> Result<Self> {
        let timing_log = std::env::var_os("KOKORO_BOOK_WORKER_TIME_LOG").map(PathBuf::from);
        let mut child = worker_process_command(executable, timing_log.as_deref())
            .arg("__worker")
            .arg("--model-dir")
            .arg(&launch.model_dir)
            .arg("--voice-file")
            .arg(&launch.voice_file)
            .arg("--cache-limit-bytes")
            .arg(launch.limits.cache_limit_bytes.to_string())
            .arg("--cached-threshold-bytes")
            .arg(launch.limits.cached_threshold_bytes.to_string())
            .arg("--memory-limit-bytes")
            .arg(launch.limits.memory_limit_bytes.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("failed to launch isolated MLX worker")?;
        let input = child.stdin.take().context("worker stdin is unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("worker stdout is unavailable")?;
        let mut output = BufReader::new(stdout);
        let ready = read_response(&mut output).context("MLX worker did not become ready")?;
        match ready {
            WorkerResponse::Ready {
                model_load_seconds,
                stats,
            } => Ok(Self {
                launch,
                child: Some(child),
                input: Some(input),
                output,
                total_model_load_seconds: model_load_seconds,
                restarts: 0,
                requests: 0,
                latest_stats: stats,
            }),
            WorkerResponse::Error { message } => {
                let _ = child.wait();
                bail!("MLX worker initialization failed: {message}");
            }
            WorkerResponse::Audio { .. } => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("MLX worker sent audio before its ready frame");
            }
        }
    }

    pub const fn total_model_load_seconds(&self) -> f64 {
        self.total_model_load_seconds
    }

    pub const fn restarts(&self) -> usize {
        self.restarts
    }

    pub const fn requests(&self) -> usize {
        self.requests
    }

    pub const fn latest_stats(&self) -> WorkerStats {
        self.latest_stats
    }

    /// Close worker stdin and wait for a clean exit.
    ///
    /// # Errors
    ///
    /// Returns an error when the worker cannot be waited on or exits nonzero.
    pub fn finish(mut self) -> Result<()> {
        self.input.take();
        let status = self
            .child
            .take()
            .context("worker process is unavailable")?
            .wait()
            .context("failed to wait for MLX worker")?;
        if !status.success() {
            bail!("MLX worker exited with {status}");
        }
        Ok(())
    }

    fn terminate(&mut self) {
        self.input.take();
        if let Some(mut child) = self.child.take() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn worker_process_command(executable: &Path, timing_log: Option<&Path>) -> ProcessCommand {
    timing_log.map_or_else(
        || ProcessCommand::new(executable),
        |log| {
            let mut command = ProcessCommand::new("/usr/bin/time");
            command
                .arg("-a")
                .arg("-l")
                .arg("-o")
                .arg(log)
                .arg(executable);
            command
        },
    )
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn worker_process_command(executable: &Path, _timing_log: Option<&Path>) -> ProcessCommand {
    ProcessCommand::new(executable)
}

impl ChunkWorker for ProcessWorker {
    fn synthesize(&mut self, phonemes: &str, speed: f32) -> Result<ChunkAudio> {
        self.requests += 1;
        let request = WorkerRequest {
            phonemes: phonemes.to_owned(),
            speed,
        };
        write_request(
            self.input.as_mut().context("MLX worker stdin is closed")?,
            &request,
        )
        .context("failed to send request to MLX worker")?;
        match read_response(&mut self.output).context("failed to read MLX worker response")? {
            WorkerResponse::Audio {
                samples,
                synthesis_seconds,
                stats,
            } => {
                if samples.is_empty() {
                    bail!("MLX worker returned empty audio");
                }
                self.latest_stats = stats;
                Ok(ChunkAudio {
                    samples,
                    synthesis_seconds,
                    stats,
                })
            }
            WorkerResponse::Error { message } => bail!("MLX worker failed: {message}"),
            WorkerResponse::Ready { .. } => bail!("MLX worker sent a duplicate ready frame"),
        }
    }

    fn restart(&mut self) -> Result<()> {
        self.terminate();
        let previous_load_seconds = self.total_model_load_seconds;
        let previous_restarts = self.restarts;
        let previous_requests = self.requests;
        let replacement = Self::launch(self.launch.clone())?;
        *self = replacement;
        self.total_model_load_seconds += previous_load_seconds;
        self.restarts = previous_restarts + 1;
        self.requests = previous_requests;
        Ok(())
    }
}

impl Drop for ProcessWorker {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// Run the hidden sequential worker loop on stdin/stdout.
///
/// # Errors
///
/// Returns an error for unsupported hardware, model setup, protocol I/O, or
/// MLX memory-control failure.
pub fn run_mlx_worker(launch: &WorkerLaunch) -> Result<()> {
    native::run(launch)
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use std::path::Path;

    use super::worker_process_command;

    #[test]
    fn wraps_only_the_worker_with_apple_time_when_requested() {
        let command = worker_process_command(
            Path::new("/tmp/kokoro-book"),
            Some(Path::new("/tmp/worker.time")),
        );

        assert_eq!(command.get_program(), "/usr/bin/time");
        assert_eq!(
            command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["-a", "-l", "-o", "/tmp/worker.time", "/tmp/kokoro-book"]
        );
    }
}

/// Try one request, then restart and split it once if the worker fails.
///
/// # Errors
///
/// Returns the original, restart, or split-retry failure. A failed half is not
/// recursively split.
pub fn synthesize_with_split_retry<W: ChunkWorker>(
    worker: &mut W,
    phonemes: &str,
    speed: f32,
) -> Result<Vec<ChunkAudio>> {
    match worker.synthesize(phonemes, speed) {
        Ok(audio) => Ok(vec![audio]),
        Err(initial_error) => {
            worker
                .restart()
                .with_context(|| format!("worker failed ({initial_error:#}) and restart failed"))?;
            let (left, right) = split_for_retry(phonemes).with_context(|| {
                format!("worker failed ({initial_error:#}) and chunk cannot be split")
            })?;

            let left_audio = worker.synthesize(left, speed).map_err(|retry_error| {
                anyhow!("worker failed ({initial_error:#}); split retry failed: {retry_error:#}")
            })?;
            let right_audio = worker.synthesize(right, speed).map_err(|retry_error| {
                anyhow!("worker failed ({initial_error:#}); split retry failed: {retry_error:#}")
            })?;
            Ok(vec![left_audio, right_audio])
        }
    }
}

fn split_for_retry(phonemes: &str) -> Result<(&str, &str)> {
    let character_count = phonemes.chars().count();
    if character_count < 2 {
        bail!("phoneme chunk is too short to split");
    }
    let midpoint = character_count / 2;
    let mut best_boundary = None;
    let mut best_distance = usize::MAX;
    for (character_index, (byte_index, character)) in phonemes.char_indices().enumerate() {
        if character.is_whitespace() {
            let distance = character_index.abs_diff(midpoint);
            if distance < best_distance {
                best_boundary = Some(byte_index);
                best_distance = distance;
            }
        }
    }

    let boundary = best_boundary.unwrap_or_else(|| {
        phonemes
            .char_indices()
            .nth(midpoint)
            .map_or(phonemes.len(), |(index, _)| index)
    });
    let left = phonemes[..boundary].trim();
    let right = phonemes[boundary..].trim();
    if left.is_empty() || right.is_empty() {
        bail!("phoneme chunk cannot be split into two non-empty requests");
    }
    Ok((left, right))
}
