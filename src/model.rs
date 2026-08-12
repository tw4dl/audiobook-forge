//! Download and validation for the pinned Kokoro model and selected voice.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use directories::BaseDirs;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::voice::Voice;

pub const MODEL_REVISION: &str = "a71e4d38b236d968966a2002c4c895dbd12b1c3c";
pub const MODEL_BUNDLE_NAME: &str = "Kokoro-82M-bf16-a71e4d38";
pub const MODEL_SHA256: &str = "4e9ecdf03b8b6cf906070390237feda473dc13327cb8d56a43deaa374c02acd8";
const MODEL_FILE: &str = "kokoro-v1_0.safetensors";
const REPOSITORY: &str = "mlx-community/Kokoro-82M-bf16";

#[derive(Debug, Clone)]
pub struct ModelAssets {
    pub root: PathBuf,
    pub model: PathBuf,
    pub voice: PathBuf,
}

impl ModelAssets {
    /// Resolve the files needed for one voice.
    ///
    /// # Errors
    ///
    /// Returns an error that lists every missing file.
    pub fn from_dir(root: &Path, voice: Voice) -> Result<Self> {
        let assets = Self {
            root: root.to_path_buf(),
            model: root.join(MODEL_FILE),
            voice: root
                .join("voices")
                .join(format!("{}.safetensors", voice.name())),
        };
        let missing = [&assets.model, &assets.voice]
            .into_iter()
            .filter(|path| !path.is_file())
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            bail!("model cache is incomplete; missing: {}", missing.join(", "));
        }
        Ok(assets)
    }

    /// Verify the model and selected voice against pinned SHA-256 hashes.
    ///
    /// # Errors
    ///
    /// Returns an error when a file cannot be read or has changed.
    pub fn verify_hashes(&self, voice: Voice) -> Result<()> {
        verify_file(&self.model, MODEL_SHA256)?;
        verify_file(&self.voice, voice.sha256())
    }
}

/// Return the model cache, including an optional environment override.
///
/// # Errors
///
/// Returns an error when the operating system has no user cache directory.
pub fn default_cache_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("AUDIOBOOK_FORGE_CACHE_DIR") {
        return Ok(PathBuf::from(path));
    }
    let base = BaseDirs::new().ok_or_else(|| anyhow!("cannot find the user cache directory"))?;
    Ok(base.cache_dir().join("audiobook-forge"))
}

/// Return verified model assets, downloading missing files from a pinned revision.
///
/// # Errors
///
/// Returns an error for cache, network, or integrity failures.
pub fn ensure_model(cache_dir: &Path, voice: Voice) -> Result<ModelAssets> {
    let model_dir = cache_dir.join(MODEL_BUNDLE_NAME);
    if let Ok(assets) = ModelAssets::from_dir(&model_dir, voice) {
        assets.verify_hashes(voice)?;
        return Ok(assets);
    }

    fs::create_dir_all(model_dir.join("voices"))
        .with_context(|| format!("failed to create model cache {}", model_dir.display()))?;
    let _lock = DownloadLock::acquire(&cache_dir.join(".download.lock"))?;

    let model = model_dir.join(MODEL_FILE);
    ensure_asset(&model, &model_url(), MODEL_SHA256, "Kokoro model")?;
    let voice_path = model_dir
        .join("voices")
        .join(format!("{}.safetensors", voice.name()));
    ensure_asset(
        &voice_path,
        &voice_url(voice),
        voice.sha256(),
        &format!("Kokoro voice {}", voice.name()),
    )?;

    let assets = ModelAssets::from_dir(&model_dir, voice)?;
    assets.verify_hashes(voice)?;
    Ok(assets)
}

fn model_url() -> String {
    format!("https://huggingface.co/{REPOSITORY}/resolve/{MODEL_REVISION}/{MODEL_FILE}")
}

fn voice_url(voice: Voice) -> String {
    format!(
        "https://huggingface.co/{REPOSITORY}/resolve/{MODEL_REVISION}/voices/{}.safetensors",
        voice.name()
    )
}

fn ensure_asset(path: &Path, url: &str, expected_hash: &str, label: &str) -> Result<()> {
    if path.is_file() {
        return verify_file(path, expected_hash);
    }

    eprintln!("Downloading {label}…");
    let response = ureq::get(url)
        .call()
        .map_err(|error| anyhow!("{label} download failed: {error}"))?;
    let total = response
        .header("content-length")
        .and_then(|value| value.parse::<u64>().ok());
    let progress = download_progress(total);
    let reader = ProgressReader::new(response.into_reader(), progress.clone());
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("asset path has no parent: {}", path.display()))?;
    let mut downloaded =
        NamedTempFile::new_in(parent).context("failed to create temporary model file")?;
    let actual = copy_and_hash(reader, downloaded.as_file_mut())?;
    progress.finish_and_clear();
    if actual != expected_hash {
        bail!("{label} failed SHA-256 verification: expected {expected_hash}, got {actual}");
    }
    downloaded
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to save {}", path.display()))?;
    Ok(())
}

fn verify_file(path: &Path, expected: &str) -> Result<()> {
    let actual = sha256(path)?;
    if actual != expected {
        bail!(
            "cached model asset failed SHA-256 verification: {}; remove this file and retry",
            path.display()
        );
    }
    Ok(())
}

fn copy_and_hash<R: Read, W: Write>(mut reader: R, mut writer: W) -> Result<String> {
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1_024];
    loop {
        let count = reader.read(&mut buffer).context("model download failed")?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        writer
            .write_all(&buffer[..count])
            .context("failed to save model download")?;
    }
    writer.flush().context("failed to flush model download")?;
    Ok(format!("{:x}", digest.finalize()))
}

fn sha256(path: &Path) -> Result<String> {
    let mut file = BufReader::new(
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?,
    );
    let mut digest = Sha256::new();
    io::copy(&mut file, &mut digest)
        .with_context(|| format!("failed to hash {}", path.display()))?;
    Ok(format!("{:x}", digest.finalize()))
}

fn download_progress(total: Option<u64>) -> ProgressBar {
    let progress = total.map_or_else(ProgressBar::new_spinner, ProgressBar::new);
    progress.set_style(
        ProgressStyle::with_template(
            "{spinner:.cyan} {bytes}/{total_bytes} [{bar:32.cyan/blue}] {eta}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    progress
}

struct ProgressReader<R> {
    inner: R,
    progress: ProgressBar,
}

impl<R> ProgressReader<R> {
    const fn new(inner: R, progress: ProgressBar) -> Self {
        Self { inner, progress }
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.progress.inc(count as u64);
        Ok(count)
    }
}

struct DownloadLock {
    path: PathBuf,
    _file: File,
}

impl DownloadLock {
    fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| {
                format!(
                    "another model download may be active; lock exists at {}",
                    path.display()
                )
            })?;
        Ok(Self {
            path: path.to_path_buf(),
            _file: file,
        })
    }
}

impl Drop for DownloadLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tempfile::NamedTempFile;

    use super::{copy_and_hash, verify_file};

    #[test]
    fn hashes_downloaded_bytes_while_copying_them() {
        let source = b"abc";
        let mut copied = Vec::new();

        let hash = copy_and_hash(Cursor::new(source), &mut copied).expect("copy and hash");

        assert_eq!(copied, source);
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn rejects_a_corrupt_cached_asset() {
        let file = NamedTempFile::new().expect("cache fixture");
        std::fs::write(file.path(), b"corrupt").expect("cache fixture data");

        let error = verify_file(file.path(), &"0".repeat(64)).expect_err("hash must fail");

        assert!(error.to_string().contains("remove this file and retry"));
    }
}
