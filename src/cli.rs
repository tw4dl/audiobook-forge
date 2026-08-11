//! Command-line interface.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};

use crate::book::Section;
use crate::input::read_book;
use crate::model::{default_cache_dir, ensure_model};
use crate::phoneme::Pronunciation;
use crate::tts::{
    DEFAULT_MAX_PHONEMES, SynthesisOptions, ensure_supported_platform, synthesize_to_wav,
    validate_settings,
};
use crate::voice::{DEFAULT_VOICE, ENGLISH_VOICES, Voice};
use crate::worker::{WorkerLaunch, WorkerLimits, run_mlx_worker};

#[derive(Debug, Parser)]
#[command(
    name = "kokoro-book",
    version,
    about = "Turn an English ebook or document into a Kokoro audiobook",
    long_about = None
)]
struct Cli {
    /// EPUB, HTML, Markdown, or TXT input file
    #[arg(value_name = "INPUT")]
    input: Option<PathBuf>,

    /// Output WAV path
    #[arg(short, long, value_name = "FILE")]
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

    /// Phoneme tokens per synthesis chunk
    #[arg(long, default_value_t = DEFAULT_MAX_PHONEMES, hide = true)]
    chunk_phonemes: usize,

    /// Hide progress output
    #[arg(short, long)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect the imported title and semantic structure without synthesis
    Inspect {
        /// EPUB, HTML, Markdown, or TXT input file
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
    run_with(cli)
}

fn run_with(cli: Cli) -> Result<()> {
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

    let input = cli
        .input
        .context("missing INPUT; run `kokoro-book --help`")?;
    let book = read_book(&input)?;
    for warning in &book.warnings {
        eprintln!("WARN: {}", terminal_text(warning));
    }
    let output = cli.output.unwrap_or_else(|| default_output(&input));
    if output
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("wav"))
    {
        bail!("output must use the .wav extension");
    }
    let voice = Voice::from_str(&cli.voice)?;
    validate_settings(cli.speed, cli.chunk_phonemes)?;
    ensure_supported_platform()?;
    let cache = default_cache_dir()?;
    let assets = ensure_model(&cache, voice)?;
    let report = synthesize_to_wav(
        &assets,
        &SynthesisOptions {
            text: &book.text,
            output: &output,
            voice,
            pronunciations: &cli.pronunciation,
            speed: cli.speed,
            max_phonemes: cli.chunk_phonemes,
            quiet: cli.quiet,
            worker_limits: WorkerLimits::default(),
        },
    )?;

    eprintln!(
        "Created {} | {:.2}s audio | {:.2}s synthesis | RTF {:.3} | {:.3}s model load | {} chunks | {} requests | {} restarts | MLX peak {:.2} GiB | cache {} B | PCM peak {:.3}",
        terminal_text(&report.output.display().to_string()),
        report.audio_seconds,
        report.synthesis_seconds,
        report.rtf,
        report.model_load_seconds,
        report.chunks,
        report.worker_requests,
        report.worker_restarts,
        bytes_to_gib(report.memory.peak_bytes),
        report.memory.cached_bytes,
        report.peak_amplitude,
    );
    Ok(())
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

fn default_output(input: &Path) -> PathBuf {
    input.with_extension("wav")
}
