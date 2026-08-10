//! Download and validation for the single pinned Kokoro model bundle.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use bzip2::read::BzDecoder;
use directories::BaseDirs;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use tempfile::{NamedTempFile, tempdir_in};

pub const MODEL_BUNDLE_NAME: &str = "kokoro-multi-lang-v1_0";
pub const MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_0.tar.bz2";
pub const MODEL_ARCHIVE_SHA256: &str =
    "c133d26353d776da730870dac7da07dbfc9a5e3bc80cc5e8e83ab6e823be7046";
const VERIFIED_MARKER: &str = ".kokoro-book-verified-v1";
const MODEL_SHA256: &str = "c436dc6a842b62aba06af67e40bafcfb9c60ac3af895358f1974ad9a7f7c026b";
const VOICES_SHA256: &str = "8a77c0d397026208d22211f37670b5b3b11e03f190756b25a1d24041fced82a9";
const TOKENS_SHA256: &str = "6ebb6bb288f20f3ae8d004d3c2ca27697da27c037d75e81a60e2a6a663f95425";
const LEXICON_SHA256: &str = "7daaab53a181be9885b853a8582bf1838186317e5dadacbcef9c426d6fa0da14";

#[derive(Debug, Clone)]
pub struct ModelAssets {
    pub model: PathBuf,
    pub voices: PathBuf,
    pub tokens: PathBuf,
    pub data_dir: PathBuf,
    pub lexicon_us: PathBuf,
}

impl ModelAssets {
    /// Resolve and check the files required by the Kokoro runtime.
    ///
    /// # Errors
    ///
    /// Returns an error that lists every missing model file or directory.
    pub fn from_dir(root: &Path) -> Result<Self> {
        let assets = Self {
            model: root.join("model.onnx"),
            voices: root.join("voices.bin"),
            tokens: root.join("tokens.txt"),
            data_dir: root.join("espeak-ng-data"),
            lexicon_us: root.join("lexicon-us-en.txt"),
        };
        let mut missing = Vec::new();
        for path in [
            &assets.model,
            &assets.voices,
            &assets.tokens,
            &assets.data_dir,
            &assets.lexicon_us,
        ] {
            if !path.exists() {
                missing.push(path.file_name().unwrap_or_default().to_string_lossy());
            }
        }
        if !missing.is_empty() {
            bail!(
                "model bundle is incomplete; missing: {}",
                missing.join(", ")
            );
        }
        Ok(assets)
    }

    /// Verify pinned hashes for the model, voices, tokens, and English lexicon.
    ///
    /// # Errors
    ///
    /// Returns an error when a file cannot be read or a hash does not match.
    pub fn verify_hashes(&self) -> Result<()> {
        for (path, expected) in [
            (&self.model, MODEL_SHA256),
            (&self.voices, VOICES_SHA256),
            (&self.tokens, TOKENS_SHA256),
            (&self.lexicon_us, LEXICON_SHA256),
        ] {
            let actual = sha256(path)?;
            if actual != expected {
                bail!(
                    "model asset failed SHA-256 verification: {}",
                    path.display()
                );
            }
        }
        Ok(())
    }
}

/// Return the model cache, including an optional environment override.
///
/// # Errors
///
/// Returns an error when the operating system has no user cache directory.
pub fn default_cache_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("KOKORO_BOOK_CACHE_DIR") {
        return Ok(PathBuf::from(path));
    }
    let base = BaseDirs::new().ok_or_else(|| anyhow!("cannot find the user cache directory"))?;
    Ok(base.cache_dir().join("kokoro-book"))
}

/// Return a verified model bundle, downloading the pinned bundle when absent.
///
/// # Errors
///
/// Returns an error for cache, network, archive, or integrity failures.
pub fn ensure_model(cache_dir: &Path) -> Result<ModelAssets> {
    let model_dir = cache_dir.join(MODEL_BUNDLE_NAME);
    if marker_is_valid(&model_dir) {
        return ModelAssets::from_dir(&model_dir);
    }

    fs::create_dir_all(cache_dir)
        .with_context(|| format!("failed to create model cache {}", cache_dir.display()))?;
    let _lock = DownloadLock::acquire(&cache_dir.join(".download.lock"))?;

    if marker_is_valid(&model_dir) {
        return ModelAssets::from_dir(&model_dir);
    }
    if model_dir.exists() {
        let assets = ModelAssets::from_dir(&model_dir)
            .with_context(|| format!("invalid existing model cache at {}", model_dir.display()))?;
        assets.verify_hashes()?;
        write_marker(&model_dir)?;
        return Ok(assets);
    }

    eprintln!("Downloading Kokoro v1.0 model (333 MiB)…");
    let downloaded = download_archive(cache_dir)?;
    let decoder = BzDecoder::new(BufReader::new(downloaded.reopen()?));
    let staging = tempdir_in(cache_dir).context("failed to create model staging directory")?;
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(staging.path())
        .context("failed to extract the model archive")?;
    let extracted = staging.path().join(MODEL_BUNDLE_NAME);
    let assets = ModelAssets::from_dir(&extracted)?;
    assets.verify_hashes()?;
    write_marker(&extracted)?;
    fs::rename(&extracted, &model_dir).with_context(|| {
        format!(
            "failed to install model from {} to {}",
            extracted.display(),
            model_dir.display()
        )
    })?;
    ModelAssets::from_dir(&model_dir)
}

fn write_marker(model_dir: &Path) -> Result<()> {
    fs::write(model_dir.join(VERIFIED_MARKER), marker_contents())
        .context("failed to write model verification marker")
}

fn marker_is_valid(model_dir: &Path) -> bool {
    fs::read_to_string(model_dir.join(VERIFIED_MARKER))
        .is_ok_and(|contents| contents == marker_contents())
}

fn marker_contents() -> String {
    format!(
        "bundle={MODEL_BUNDLE_NAME}\narchive_sha256={MODEL_ARCHIVE_SHA256}\nmodel_sha256={MODEL_SHA256}\n"
    )
}

fn download_archive(cache_dir: &Path) -> Result<NamedTempFile> {
    let response = ureq::get(MODEL_URL)
        .call()
        .map_err(|error| anyhow!("model download failed: {error}"))?;
    let total = response
        .header("content-length")
        .and_then(|value| value.parse::<u64>().ok());
    let progress = download_progress(total);
    let reader = ProgressReader::new(response.into_reader(), progress.clone());
    let mut downloaded =
        NamedTempFile::new_in(cache_dir).context("failed to create temporary model archive")?;
    let actual = copy_and_hash(reader, downloaded.as_file_mut())?;
    progress.finish_and_clear();
    if actual != MODEL_ARCHIVE_SHA256 {
        bail!(
            "model archive failed SHA-256 verification: expected {MODEL_ARCHIVE_SHA256}, got {actual}"
        );
    }
    Ok(downloaded)
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
    let digest = digest.finalize();
    Ok(format!("{digest:x}"))
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

    use super::copy_and_hash;

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
}
