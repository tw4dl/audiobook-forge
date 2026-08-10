//! Command-line interface.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::input::read_book;
use crate::model::{default_cache_dir, ensure_model};
use crate::tts::{SynthesisOptions, synthesize_to_wav, validate_settings};
use crate::voice::{DEFAULT_VOICE, ENGLISH_VOICES, Voice};

#[derive(Debug, Parser)]
#[command(
    name = "kokoro-book",
    version,
    about = "Turn an English EPUB or TXT file into a Kokoro audiobook",
    long_about = None
)]
struct Cli {
    /// EPUB or TXT input file
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

    /// Text characters per synthesis chunk
    #[arg(long, default_value_t = 450, hide = true)]
    chunk_chars: usize,

    /// CPU inference threads
    #[arg(long, default_value_t = 2, hide = true)]
    threads: i32,

    /// Hide progress output
    #[arg(short, long)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List English Kokoro preset voices
    Voices,
}

/// Parse arguments and run the CLI.
///
/// # Errors
///
/// Returns an error for invalid input, model setup, or synthesis output.
pub fn run() -> Result<()> {
    run_with(Cli::parse())
}

fn run_with(cli: Cli) -> Result<()> {
    if matches!(cli.command, Some(Command::Voices)) {
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

    let input = cli
        .input
        .context("missing INPUT; run `kokoro-book --help`")?;
    let book = read_book(&input)?;
    let output = cli.output.unwrap_or_else(|| default_output(&input));
    if output
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("wav"))
    {
        bail!("output must use the .wav extension");
    }
    let voice = Voice::from_str(&cli.voice)?;
    validate_settings(cli.speed, cli.chunk_chars, cli.threads)?;
    let cache = default_cache_dir()?;
    let assets = ensure_model(&cache)?;
    let report = synthesize_to_wav(
        &assets,
        &SynthesisOptions {
            text: &book.text,
            output: &output,
            voice,
            speed: cli.speed,
            chunk_chars: cli.chunk_chars,
            threads: cli.threads,
            quiet: cli.quiet,
        },
    )?;

    eprintln!(
        "Created {} | {:.2}s audio | {:.2}s synthesis | RTF {:.3} | {:.2}s model load | {} chunks",
        report.output.display(),
        report.audio_seconds,
        report.synthesis_seconds,
        report.rtf,
        report.model_load_seconds,
        report.chunks
    );
    Ok(())
}

fn default_output(input: &Path) -> PathBuf {
    input.with_extension("wav")
}
