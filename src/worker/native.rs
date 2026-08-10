//! Apple MLX synthesis loop and its unsupported-platform stub.

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::io::{self, BufReader, BufWriter, Write};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::time::Instant;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use anyhow::Context;
use anyhow::{Result, bail};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use mlx_memory_control::{clear_cache, memory_stats, set_cache_limit};

use super::{WorkerLaunch, WorkerStats};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::{WorkerRequest, WorkerResponse, read_request, write_response};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(super) fn run(launch: &WorkerLaunch) -> Result<()> {
    let cache_limit = usize::try_from(launch.limits.cache_limit_bytes)
        .context("MLX cache limit does not fit this platform")?;
    set_cache_limit(cache_limit).context("failed to set MLX allocation-cache limit")?;

    let model_dir = launch
        .model_dir
        .to_str()
        .context("model directory is not valid UTF-8")?;
    let load_started = Instant::now();
    let mut model = voice_tts::load_model(model_dir).context("failed to load Kokoro MLX model")?;
    let voice = voice_tts::voice::load_voice_from_file(&launch.voice_file)
        .context("failed to load Kokoro voice")?;
    clear_cache().context("failed to clear MLX cache after model load")?;
    let baseline = checked_stats(launch)?;
    let model_load_seconds = load_started.elapsed().as_secs_f64();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = BufReader::new(stdin.lock());
    let mut output = BufWriter::new(stdout.lock());
    write_response(
        &mut output,
        &WorkerResponse::Ready {
            model_load_seconds,
            stats: baseline,
        },
    )?;

    while let Some(request) = read_request(&mut input)? {
        let result = synthesize_one(&mut model, &voice, &request, launch);
        match result {
            Ok(response) => write_response(&mut output, &response)?,
            Err(error) => {
                let clear_error = clear_cache().err();
                let message = clear_error.map_or_else(
                    || format!("{error:#}"),
                    |cache_error| format!("{error:#}; cache cleanup also failed: {cache_error}"),
                );
                write_response(&mut output, &WorkerResponse::Error { message })?;
                output.flush()?;
                return Ok(());
            }
        }
    }
    output.flush()?;
    Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn synthesize_one(
    model: &mut voice_tts::KokoroModel,
    voice: &voice_tts::Array,
    request: &WorkerRequest,
    launch: &WorkerLaunch,
) -> Result<WorkerResponse> {
    validate_request(request)?;
    let started = Instant::now();
    let audio = voice_tts::generate(model, &request.phonemes, voice, request.speed)
        .context("Kokoro generation failed")?;
    audio.eval().context("failed to evaluate Kokoro PCM")?;
    let samples = audio
        .try_as_slice::<f32>()
        .context("failed to copy Kokoro PCM")?
        .to_vec();
    let synthesis_seconds = started.elapsed().as_secs_f64();
    drop(audio);

    clear_cache().context("failed to clear MLX cache after chunk")?;
    let stats = checked_stats(launch)?;
    Ok(WorkerResponse::Audio {
        samples,
        synthesis_seconds,
        stats,
    })
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn validate_request(request: &WorkerRequest) -> Result<()> {
    if request.phonemes.is_empty() {
        bail!("phoneme request is empty");
    }
    if request.phonemes.chars().count() > 510 {
        bail!("phoneme request exceeds Kokoro's 510-token context");
    }
    if !(0.5..=2.0).contains(&request.speed) {
        bail!("speed must be between 0.5 and 2.0");
    }
    Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn checked_stats(launch: &WorkerLaunch) -> Result<WorkerStats> {
    let native = memory_stats().context("failed to read MLX memory counters")?;
    let stats = WorkerStats {
        active_bytes: u64::try_from(native.active_bytes)
            .context("MLX active memory counter overflow")?,
        cached_bytes: u64::try_from(native.cached_bytes)
            .context("MLX cached memory counter overflow")?,
        peak_bytes: u64::try_from(native.peak_bytes).context("MLX peak memory counter overflow")?,
    };
    if stats.cached_bytes > launch.limits.cached_threshold_bytes {
        bail!(
            "MLX cache remained at {} bytes; limit is {} bytes",
            stats.cached_bytes,
            launch.limits.cached_threshold_bytes
        );
    }
    if memory_limit_crossed(stats, launch.limits.memory_limit_bytes) {
        bail!(
            "MLX memory crossed {} bytes (active {}, peak {})",
            launch.limits.memory_limit_bytes,
            stats.active_bytes,
            stats.peak_bytes
        );
    }
    Ok(stats)
}

const fn memory_limit_crossed(stats: WorkerStats, limit: u64) -> bool {
    stats.active_bytes >= limit || stats.peak_bytes >= limit
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub(super) fn run(_launch: &WorkerLaunch) -> Result<()> {
    bail!("Kokoro MLX requires Apple Silicon macOS")
}

#[cfg(test)]
mod tests {
    use super::memory_limit_crossed;
    use crate::worker::WorkerStats;

    #[test]
    fn treats_the_memory_limit_as_an_exclusive_ceiling() {
        let limit = 4 * 1_024 * 1_024 * 1_024;
        let below = WorkerStats {
            active_bytes: limit - 1,
            cached_bytes: 0,
            peak_bytes: limit - 1,
        };
        let at_limit = WorkerStats {
            peak_bytes: limit,
            ..below
        };

        assert!(!memory_limit_crossed(below, limit));
        assert!(memory_limit_crossed(at_limit, limit));
    }
}
