//! Command-line interface.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::error::ErrorKind;
use clap::{Parser, Subcommand, ValueEnum};

use crate::book::Section;
use crate::build::{
    AudiobookBuildOptions, AudiobookBuildReport, build_audiobook, validate_build_options,
};
use crate::input::read_book;
use crate::m4b::{ChapterPolicy, ensure_media_tools};
use crate::model::{default_cache_dir, ensure_model};
use crate::narration::{FootnoteMode, NarrationPolicy, plan_narration};
use crate::phoneme::Pronunciation;
use crate::synthesis::{SegmentCache, SynthesisSettings};
use crate::tts::{
    DEFAULT_MAX_PHONEMES, KokoroProviderReport, KokoroTtsProvider, ensure_supported_platform,
    validate_settings,
};
use crate::voice::{DEFAULT_VOICE, ENGLISH_VOICES, Voice};
use crate::worker::{WorkerLaunch, WorkerLimits, run_mlx_worker};

#[derive(Debug, Parser)]
#[command(
    name = "kokoro-book",
    version,
    about = "Turn an English ebook or document into a navigable M4B audiobook",
    long_about = None
)]
struct Cli {
    /// EPUB, DRM-free AZW3/MOBI, text-based PDF, HTML, Markdown, or TXT input file
    #[arg(value_name = "INPUT")]
    input: Option<PathBuf>,

    /// Output directory; defaults to a folder beside the source
    #[arg(short, long, value_name = "DIR")]
    output: Option<PathBuf>,

    /// Kokoro preset voice
    #[arg(short, long, default_value = DEFAULT_VOICE)]
    voice: String,

    /// Speech speed from 0.5 to 2.0
    #[arg(long, default_value_t = 1.0)]
    speed: f32,

    /// Override one word's pronunciation with IPA; repeat as needed
    #[arg(long, value_name = "WORD=IPA")]
    pronunciation: Vec<Pronunciation>,

    /// Visible M4B navigation depth
    #[arg(long, value_enum, default_value = "chapters")]
    nav: NavOption,

    /// Footnote narration policy
    #[arg(long, value_enum, default_value = "inline")]
    footnotes: FootnoteOption,

    /// Phoneme tokens per synthesis chunk
    #[arg(long, default_value_t = DEFAULT_MAX_PHONEMES, hide = true)]
    chunk_phonemes: usize,

    /// Hide progress output
    #[arg(short, long)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum NavOption {
    Chapters,
    Sections,
    Auto,
}

impl From<NavOption> for ChapterPolicy {
    fn from(value: NavOption) -> Self {
        match value {
            NavOption::Chapters => Self::Chapters,
            NavOption::Sections => Self::Sections,
            NavOption::Auto => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FootnoteOption {
    Inline,
    Skip,
    End,
}

impl From<FootnoteOption> for FootnoteMode {
    fn from(value: FootnoteOption) -> Self {
        match value {
            FootnoteOption::Inline => Self::Inline,
            FootnoteOption::Skip => Self::Skip,
            FootnoteOption::End => Self::End,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect the imported title and semantic structure without synthesis
    Inspect {
        /// EPUB, DRM-free AZW3/MOBI, text-based PDF, HTML, Markdown, or TXT input file
        #[arg(value_name = "INPUT")]
        input: PathBuf,

        /// Show the semantic navigation tree
        #[arg(long)]
        tree: bool,
    },

    /// List English Kokoro preset voices
    Voices,

    /// Internal MLX subprocess
    #[command(name = "__worker", hide = true)]
    Worker {
        #[arg(long)]
        model_dir: PathBuf,
        #[arg(long)]
        voice_file: PathBuf,
        #[arg(long)]
        cache_limit_bytes: u64,
        #[arg(long)]
        cached_threshold_bytes: u64,
        #[arg(long)]
        memory_limit_bytes: u64,
    },
}

/// Parse arguments and run the CLI.
///
/// # Errors
///
/// Returns an error for invalid input, model setup, or synthesis output.
pub fn run() -> Result<()> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            print!("{error}");
            return Ok(());
        }
        Err(error) => {
            let rendered = error.to_string();
            let rendered = rendered.strip_prefix("error: ").unwrap_or(&rendered);
            bail!(terminal_text(rendered));
        }
    };
    run_with(&cli)
}

fn run_with(cli: &Cli) -> Result<()> {
    match &cli.command {
        Some(Command::Inspect { input, tree }) => {
            let book = read_book(input)?;
            print_inspection(&book, *tree);
            return Ok(());
        }
        Some(Command::Voices) => {
            for voice in ENGLISH_VOICES {
                let suffix = if voice.name == DEFAULT_VOICE {
                    " (default)"
                } else {
                    ""
                };
                println!("{}{suffix}", voice.name);
            }
            return Ok(());
        }
        Some(Command::Worker {
            model_dir,
            voice_file,
            cache_limit_bytes,
            cached_threshold_bytes,
            memory_limit_bytes,
        }) => {
            return run_mlx_worker(&WorkerLaunch {
                model_dir: model_dir.clone(),
                voice_file: voice_file.clone(),
                limits: WorkerLimits {
                    cache_limit_bytes: *cache_limit_bytes,
                    cached_threshold_bytes: *cached_threshold_bytes,
                    memory_limit_bytes: *memory_limit_bytes,
                },
            });
        }
        None => {}
    }

    run_conversion(cli)
}

fn run_conversion(cli: &Cli) -> Result<()> {
    let input = cli
        .input
        .as_deref()
        .context("missing INPUT; run `kokoro-book --help`")?;
    let book = read_book(input)?;
    for warning in &book.warnings {
        eprintln!("WARN: {}", terminal_text(warning));
    }
    let voice = Voice::from_str(&cli.voice)?;
    validate_settings(cli.speed, cli.chunk_phonemes)?;
    let options = conversion_options(cli, input)?;
    validate_build_options(&options)?;
    ensure_media_tools()?;

    let plan = plan_narration(&book, options.narration);
    for warning in &plan.warnings {
        eprintln!("WARN: {}", terminal_text(warning));
    }
    if plan.units.is_empty() {
        bail!("input contains no text under the selected narration policy");
    }

    ensure_supported_platform()?;
    let cache_root = default_cache_dir()?;
    let assets = ensure_model(&cache_root, voice)?;
    let segment_cache = SegmentCache::new(cache_root.join("segments"));
    let mut provider = KokoroTtsProvider::launch(
        &assets,
        voice,
        &cli.pronunciation,
        cli.chunk_phonemes,
        WorkerLimits::default(),
    )?;
    let build_result = build_audiobook(&book, &mut provider, &segment_cache, &options);
    let provider_result = provider.finish();
    let report = build_result?;
    let provider_report = provider_result?;

    print_build_report(&report, provider_report, cli.quiet);
    Ok(())
}

fn conversion_options(cli: &Cli, input: &Path) -> Result<AudiobookBuildOptions> {
    Ok(AudiobookBuildOptions {
        output_dir: cli
            .output
            .clone()
            .unwrap_or_else(|| default_output_directory(input)),
        base_name: output_base_name(input)?,
        chapters: cli.nav.into(),
        narration: NarrationPolicy {
            footnotes: cli.footnotes.into(),
        },
        synthesis: SynthesisSettings {
            speed: cli.speed,
            pause_ms: 120,
            max_retries: 2,
        },
        pronunciation_overrides: cli.pronunciation.iter().map(ToString::to_string).collect(),
        build_timestamp_unix_seconds: build_timestamp()?,
    })
}

fn print_build_report(report: &AudiobookBuildReport, provider: KokoroProviderReport, quiet: bool) {
    eprintln!("Created {}", terminal_path(&report.m4b_path));
    if quiet {
        return;
    }
    let audio_seconds = Duration::from_millis(report.m4b.duration_ms).as_secs_f64();
    let rtf = if audio_seconds > 0.0 {
        provider.synthesis_seconds / audio_seconds
    } else {
        0.0
    };
    eprintln!(
        "Audio {:.2}s | synthesis {:.2}s | RTF {:.3} | model load {:.3}s | {} requests | {} restarts | MLX peak {:.2} GiB | {} cache hits | {} new chunks",
        audio_seconds,
        provider.synthesis_seconds,
        rtf,
        provider.model_load_seconds,
        provider.worker_requests,
        provider.worker_restarts,
        bytes_to_gib(provider.memory.peak_bytes),
        report.synthesis.cache_hits,
        report.synthesis.generated_chunks,
    );
    eprintln!("Navigation {}", terminal_path(&report.audionav_path));
    eprintln!("Manifest {}", terminal_path(&report.manifest_path));
    if let Some(cover) = report.cover_path.as_ref() {
        eprintln!("Cover {}", terminal_path(cover));
    }
}

fn print_inspection(book: &crate::book::CanonicalBook, tree: bool) {
    println!(
        "Title: {}",
        terminal_text(book.metadata.title.as_deref().unwrap_or("Untitled"))
    );
    if let Some(version) = book.source.format_version.as_deref() {
        println!("Format: {} {}", book.source.format, terminal_text(version));
    } else {
        println!("Format: {}", book.source.format);
    }
    if !book.metadata.authors.is_empty() {
        let label = if book.metadata.authors.len() == 1 {
            "Author"
        } else {
            "Authors"
        };
        println!(
            "{label}: {}",
            terminal_text(&book.metadata.authors.join("; "))
        );
    }
    if let Some(language) = book.metadata.language.as_deref() {
        println!("Language: {}", terminal_text(language));
    }
    if let Some(cover) = book.metadata.cover.as_ref() {
        println!(
            "Cover: {} ({})",
            terminal_text(&cover.source_id),
            terminal_text(&cover.media_type)
        );
    }
    if !book.pages.is_empty() {
        println!("Pages: {}", book.pages.len());
    }
    for warning in &book.warnings {
        println!("WARN: {}", terminal_text(warning));
    }
    if tree {
        println!();
        print_tree(&book.root, 0);
    } else {
        println!();
        println!("Sections:");
        for (index, section) in book.root.children.iter().enumerate() {
            println!(
                "{index:02}  {}  {} words",
                terminal_text(section.title.as_deref().unwrap_or("Untitled")),
                section.word_count()
            );
        }
    }
    println!();
    println!("Narrated words: {}", book.word_count());
    println!("Warnings: {}", book.warnings.len());
}

fn print_tree(section: &Section, depth: usize) {
    let indent = "  ".repeat(depth);
    println!(
        "{indent}{}",
        terminal_text(section.title.as_deref().unwrap_or("Untitled"))
    );
    for child in &section.children {
        print_tree(child, depth + 1);
    }
}

/// Replace terminal control and bidirectional override characters in untrusted text.
#[must_use]
pub fn terminal_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '\u{061c}'
                        | '\u{200e}'
                        | '\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                )
            {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / 1_073_741_824.0
}

fn terminal_path(path: &Path) -> String {
    terminal_text(&path.display().to_string())
}

fn default_output_directory(input: &Path) -> PathBuf {
    input.with_extension("")
}

fn output_base_name(input: &Path) -> Result<String> {
    let base_name = input
        .file_stem()
        .filter(|value| !value.is_empty())
        .context("input path has no usable file name")?;
    Ok(base_name.to_string_lossy().into_owned())
}

fn build_timestamp() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")
        .map(|duration| duration.as_secs())
}
