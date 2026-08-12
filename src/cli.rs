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
use crate::preflight::{
    PreflightOptions, PreflightReport, preflight_book, read_prepared, validate_prepared_artifact,
    write_chapter_texts, write_prepared, write_report, write_suggestions,
};
use crate::qwen::{
    DEFAULT_QWEN_VOICE, QwenProviderReport, QwenTtsProvider, default_python,
    validate_speed as validate_qwen_speed,
};
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

    /// Speech provider
    #[arg(long, value_enum, default_value = "kokoro")]
    provider: ProviderOption,

    /// Provider voice; defaults to `af_heart` for Kokoro and Aiden for Qwen
    #[arg(short, long)]
    voice: Option<String>,

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

    /// Use a matching prepared narration directory or JSONL artifact
    #[arg(long, value_name = "PATH")]
    prepared: Option<PathBuf>,

    /// Fail on best-effort preflight repairs as well as blockers
    #[arg(long)]
    strict_preflight: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ProviderOption {
    Kokoro,
    Qwen,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PreflightFormat {
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PreflightFailOn {
    Unresolved,
    None,
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

    /// Compile narration text and validate every unit without starting TTS
    Preflight {
        /// EPUB, DRM-free AZW3/MOBI, text-based PDF, HTML, Markdown, or TXT input file
        #[arg(value_name = "INPUT")]
        input: PathBuf,

        /// Prepared output directory; writes report, narration JSONL, suggestions, and chapter text
        #[arg(short, long, value_name = "DIR")]
        output: Option<PathBuf>,

        /// Explicit report JSON path
        #[arg(long, value_name = "PATH")]
        report: Option<PathBuf>,

        /// Explicit prepared narration JSONL path
        #[arg(long, value_name = "PATH")]
        prepared: Option<PathBuf>,

        /// Explicit pronunciation suggestions path
        #[arg(long, value_name = "PATH")]
        suggestions: Option<PathBuf>,

        /// Kokoro preset voice
        #[arg(short, long, default_value = DEFAULT_VOICE)]
        voice: String,

        /// Override one word's pronunciation with IPA; repeat as needed
        #[arg(long, value_name = "WORD=IPA")]
        pronunciation: Vec<Pronunciation>,

        /// Phoneme tokens per synthesis chunk
        #[arg(long, default_value_t = DEFAULT_MAX_PHONEMES)]
        chunk_phonemes: usize,

        /// Narration footnote policy
        #[arg(long, value_enum, default_value = "inline")]
        footnotes: FootnoteOption,

        /// Visible chapter policy to record for a later build
        #[arg(long, value_enum, default_value = "chapters")]
        nav: NavOption,

        /// Render text or JSON summary to stdout
        #[arg(long, value_enum, default_value = "json")]
        format: PreflightFormat,

        /// Exit nonzero on unresolved issues, or always return the report
        #[arg(long, value_enum, default_value = "unresolved")]
        fail_on: PreflightFailOn,
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
        Some(Command::Preflight {
            input,
            output,
            report,
            prepared,
            suggestions,
            voice,
            pronunciation,
            chunk_phonemes,
            footnotes,
            nav,
            format,
            fail_on,
        }) => {
            run_preflight_command(
                input,
                output.as_deref(),
                report.as_deref(),
                prepared.as_deref(),
                suggestions.as_deref(),
                voice,
                pronunciation,
                *chunk_phonemes,
                *footnotes,
                *nav,
                *format,
                *fail_on,
            )?;
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

#[allow(clippy::too_many_arguments)]
fn run_preflight_command(
    input: &Path,
    output: Option<&Path>,
    report_path: Option<&Path>,
    prepared_path: Option<&Path>,
    suggestions_path: Option<&Path>,
    voice_name: &str,
    pronunciations: &[Pronunciation],
    max_phonemes: usize,
    footnotes: FootnoteOption,
    _nav: NavOption,
    format: PreflightFormat,
    fail_on: PreflightFailOn,
) -> Result<()> {
    let book = read_book(input)?;
    let plan = plan_narration(
        &book,
        NarrationPolicy {
            footnotes: footnotes.into(),
        },
    );
    let voice = Voice::from_str(voice_name)?;
    validate_settings(1.0, max_phonemes)?;
    let outcome = preflight_book(
        &book,
        &plan,
        &PreflightOptions {
            voice,
            pronunciations: pronunciations.to_vec(),
            max_phonemes,
            max_characters: crate::tts::DEFAULT_PROVIDER_MAX_CHARACTERS,
        },
    )?;
    let prepared_dir = output.map_or_else(|| input.with_extension("prepared"), PathBuf::from);
    let base = output_base_name(input)?;
    let report_path = report_path.map_or_else(
        || prepared_dir.join(format!("{base}.preflight.json")),
        PathBuf::from,
    );
    let prepared_path = prepared_path.map_or_else(
        || prepared_dir.join(format!("{base}.narration.jsonl")),
        PathBuf::from,
    );
    let suggestions_path = suggestions_path.map_or_else(
        || prepared_dir.join(format!("{base}.pronunciations.txt")),
        PathBuf::from,
    );
    write_report(&report_path, &outcome.report)?;
    write_prepared(&prepared_path, &outcome.prepared)?;
    write_suggestions(&suggestions_path, &outcome.report)?;
    write_chapter_texts(&prepared_dir.join("chapters"), &outcome.prepared)?;
    print_preflight_summary(&outcome.report, format);
    eprintln!("Report {}", terminal_path(&report_path));
    eprintln!("Prepared {}", terminal_path(&prepared_path));
    eprintln!("Suggestions {}", terminal_path(&suggestions_path));
    if matches!(fail_on, PreflightFailOn::Unresolved) && outcome.report.unresolved > 0 {
        bail!(
            "preflight found {} blocking issue(s); see {}",
            outcome.report.unresolved,
            report_path.display()
        );
    }
    Ok(())
}

fn print_preflight_summary(report: &PreflightReport, format: PreflightFormat) {
    match format {
        PreflightFormat::Json => {
            if let Ok(json) = serde_json::to_string_pretty(report) {
                println!("{json}");
            }
        }
        PreflightFormat::Text => {
            println!("Preflight complete");
            println!("  Narration units: {}", report.scanned_units);
            println!("  Automatic repairs: {}", report.automatic_repairs);
            println!("  Text repairs: {}", report.text_repairs);
            println!(
                "  Best-effort pronunciations: {}",
                report.best_effort_pronunciations
            );
            println!("  Blocking errors: {}", report.unresolved);
            for issue in &report.issues {
                println!(
                    "  {} × {}: {}",
                    issue.kind.key(),
                    issue.occurrences,
                    issue.detail
                );
            }
        }
    }
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
    validate_settings(cli.speed, cli.chunk_phonemes)?;
    if cli.provider == ProviderOption::Qwen {
        validate_qwen_options(cli)?;
    }
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

    match cli.provider {
        ProviderOption::Kokoro => run_kokoro_conversion(cli, input, &book, &plan, options),
        ProviderOption::Qwen => run_qwen_conversion(cli, &book, &plan, &options),
    }
}

fn validate_qwen_options(cli: &Cli) -> Result<()> {
    validate_qwen_speed(cli.speed)?;
    if !cli.pronunciation.is_empty() {
        bail!("--pronunciation is supported only by the Kokoro provider");
    }
    if cli.prepared.is_some() {
        bail!("--prepared is not yet supported by the Qwen provider");
    }
    if cli.strict_preflight {
        bail!("--strict-preflight is supported only by the Kokoro provider");
    }
    Ok(())
}

fn run_kokoro_conversion(
    cli: &Cli,
    input: &Path,
    book: &crate::book::CanonicalBook,
    plan: &crate::narration::NarrationPlan,
    mut options: AudiobookBuildOptions,
) -> Result<()> {
    let voice_name = cli.voice.as_deref().unwrap_or(DEFAULT_VOICE);
    let voice = Voice::from_str(voice_name)?;

    let preflight_options = PreflightOptions {
        voice,
        pronunciations: cli.pronunciation.clone(),
        max_phonemes: cli.chunk_phonemes,
        max_characters: crate::tts::DEFAULT_PROVIDER_MAX_CHARACTERS,
    };
    let prepared = if let Some(path) = cli.prepared.as_deref() {
        let prepared_path = resolve_prepared_path(path, input)?;
        let prepared = read_prepared(&prepared_path)?;
        validate_prepared_artifact(book, plan, &prepared, &preflight_options)?;
        if cli.strict_preflight
            && prepared.units.iter().any(|unit| {
                unit.repairs
                    .iter()
                    .any(|repair| repair.rule == "best_effort_pronunciation")
            })
        {
            bail!("strict preflight rejects best-effort pronunciation repairs");
        }
        prepared
    } else {
        let outcome = preflight_book(book, plan, &preflight_options)?;
        let prepared_dir = options.output_dir.join("prepared");
        let base = output_base_name(input)?;
        let report_path = prepared_dir.join(format!("{base}.preflight.json"));
        let narration_path = prepared_dir.join(format!("{base}.narration.jsonl"));
        let suggestions_path = prepared_dir.join(format!("{base}.pronunciations.txt"));
        write_report(&report_path, &outcome.report)?;
        write_prepared(&narration_path, &outcome.prepared)?;
        write_suggestions(&suggestions_path, &outcome.report)?;
        write_chapter_texts(&prepared_dir.join("chapters"), &outcome.prepared)?;
        eprintln!(
            "Preflight complete | {} units | {} text repairs | {} phoneme repairs | {} best-effort | {} blocking errors",
            outcome.report.scanned_units,
            outcome.report.text_repairs,
            outcome.report.automatic_repairs,
            outcome.report.best_effort_pronunciations,
            outcome.report.unresolved
        );
        if outcome.report.unresolved > 0 {
            bail!("preflight blocked the build; see {}", report_path.display());
        }
        if cli.strict_preflight && outcome.report.best_effort_pronunciations > 0 {
            bail!("strict preflight rejects best-effort pronunciation repairs");
        }
        outcome.prepared
    };
    options.synthesis.prepared = Some(prepared);

    ensure_supported_platform()?;
    let cache_root = default_cache_dir()?;
    let assets = ensure_model(&cache_root, voice)?;
    let segment_cache = segment_cache(&cache_root);
    let mut provider = KokoroTtsProvider::launch(
        &assets,
        voice,
        &cli.pronunciation,
        cli.chunk_phonemes,
        WorkerLimits::default(),
    )?;
    let build_result = build_audiobook(book, &mut provider, &segment_cache, &options);
    let provider_result = provider.finish();
    let report = build_result?;
    let provider_report = ProviderRunReport::from(provider_result?);

    print_build_report(&report, &provider_report, cli.quiet);
    Ok(())
}

fn run_qwen_conversion(
    cli: &Cli,
    book: &crate::book::CanonicalBook,
    _plan: &crate::narration::NarrationPlan,
    options: &AudiobookBuildOptions,
) -> Result<()> {
    let voice = cli.voice.as_deref().unwrap_or(DEFAULT_QWEN_VOICE);
    let cache_root = default_cache_dir()?;
    let segment_cache = segment_cache(&cache_root);
    let mut provider = QwenTtsProvider::launch(default_python(&cache_root), voice)?;
    let build_result = build_audiobook(book, &mut provider, &segment_cache, options);
    let provider_result = provider.finish();
    let report = build_result?;
    let provider_report = ProviderRunReport::from(provider_result?);

    print_build_report(&report, &provider_report, cli.quiet);
    Ok(())
}

fn resolve_prepared_path(path: &Path, input: &Path) -> Result<PathBuf> {
    if path.is_dir() || path.extension().is_none() {
        return Ok(path.join(format!("{}.narration.jsonl", output_base_name(input)?)));
    }
    Ok(path.to_path_buf())
}

fn segment_cache(cache_root: &Path) -> SegmentCache {
    SegmentCache::new(
        std::env::var_os("KOKORO_BOOK_SEGMENT_CACHE_DIR")
            .map_or_else(|| cache_root.join("segments"), PathBuf::from),
    )
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
            prepared: None,
        },
        pronunciation_overrides: cli.pronunciation.iter().map(ToString::to_string).collect(),
        build_timestamp_unix_seconds: build_timestamp()?,
    })
}

#[derive(Debug, Clone, Copy)]
struct ProviderRunReport {
    worker_requests: usize,
    worker_restarts: usize,
    generation_retries: usize,
    model_load_seconds: f64,
    synthesis_seconds: f64,
    peak_memory_bytes: u64,
}

impl From<KokoroProviderReport> for ProviderRunReport {
    fn from(value: KokoroProviderReport) -> Self {
        Self {
            worker_requests: value.worker_requests,
            worker_restarts: value.worker_restarts,
            generation_retries: 0,
            model_load_seconds: value.model_load_seconds,
            synthesis_seconds: value.synthesis_seconds,
            peak_memory_bytes: value.memory.peak_bytes,
        }
    }
}

impl From<QwenProviderReport> for ProviderRunReport {
    fn from(value: QwenProviderReport) -> Self {
        Self {
            worker_requests: value.worker_requests,
            worker_restarts: value.worker_restarts,
            generation_retries: value.generation_retries,
            model_load_seconds: value.model_load_seconds,
            synthesis_seconds: value.synthesis_seconds,
            peak_memory_bytes: value.peak_memory_bytes,
        }
    }
}

fn print_build_report(report: &AudiobookBuildReport, provider: &ProviderRunReport, quiet: bool) {
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
        "Audio {:.2}s | synthesis {:.2}s | RTF {:.3} | model load {:.3}s | {} requests | {} restarts | {} generation retries | MLX peak {:.2} GiB | {} cache hits | {} new chunks",
        audio_seconds,
        provider.synthesis_seconds,
        rtf,
        provider.model_load_seconds,
        provider.worker_requests,
        provider.worker_restarts,
        provider.generation_retries,
        bytes_to_gib(provider.peak_memory_bytes),
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
