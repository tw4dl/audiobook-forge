//! Zero-audio validation and prepared narration persistence.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::book::{CanonicalBook, SourceRange};
use crate::chunk::chunk_text;
use crate::narration::{NarrationPlan, NarrationUnit, TextNormalizationReport};
use crate::phoneme::{Phonemizer, Pronunciation};
use crate::pipeline::{extract_sentences, pack_phoneme_sentences};
use crate::synthesis::PhonemeNormalizationReport;
use crate::vocab::{self, PHONEME_NORMALIZATION_VERSION};
use crate::voice::Voice;

pub const PREFLIGHT_SCHEMA_VERSION: u32 = 1;
pub const PREPARED_NARRATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct PreflightOptions {
    pub(crate) voice: Voice,
    pub(crate) pronunciations: Vec<Pronunciation>,
    pub(crate) max_phonemes: usize,
    pub(crate) max_characters: usize,
}

impl PreflightOptions {
    pub(crate) fn profile(&self) -> String {
        let language = if self.voice.is_british() {
            "en-GB"
        } else {
            "en-US"
        };
        format!("{language}-kokoro-v1")
    }

    pub(crate) fn configuration_hash(&self) -> String {
        let overrides = self
            .pronunciations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        crate::tts::configuration_hash(self.voice, &overrides, self.max_phonemes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightReport {
    pub schema_version: u32,
    pub source_sha256: String,
    pub profile: String,
    pub normalization_version: u32,
    pub scanned_units: usize,
    pub scanned_sentences: usize,
    pub automatic_repairs: usize,
    pub text_repairs: usize,
    pub best_effort_pronunciations: usize,
    pub unresolved: usize,
    pub issues_by_kind: BTreeMap<String, usize>,
    pub issues: Vec<PreflightIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightIssue {
    pub signature: String,
    pub kind: PreflightIssueKind,
    pub token: Option<String>,
    pub unit_id: String,
    pub sentence_index: usize,
    pub source_range: Option<SourceRange>,
    pub text: String,
    pub occurrences: usize,
    pub suggestion: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreflightIssueKind {
    AutomaticRepair,
    UnknownG2pToken,
    UnsupportedPhoneme,
    OversizedChunk,
    EmptyNarrationUnit,
    ExcludedByPolicy,
    SourceParseWarning,
}

impl PreflightIssueKind {
    fn is_unresolved(&self) -> bool {
        matches!(
            self,
            Self::UnknownG2pToken | Self::UnsupportedPhoneme | Self::OversizedChunk
        )
    }

    pub(crate) fn key(&self) -> &'static str {
        match self {
            Self::AutomaticRepair => "automatic_repair",
            Self::UnknownG2pToken => "unknown_g2p_token",
            Self::UnsupportedPhoneme => "unsupported_phoneme",
            Self::OversizedChunk => "oversized_chunk",
            Self::EmptyNarrationUnit => "empty_narration_unit",
            Self::ExcludedByPolicy => "excluded_by_policy",
            Self::SourceParseWarning => "source_parse_warning",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedNarration {
    pub schema_version: u32,
    pub complete: bool,
    pub source_sha256: String,
    pub profile: String,
    pub provider_configuration_hash: String,
    pub normalization_version: u32,
    pub max_phonemes: usize,
    pub max_characters: usize,
    pub units: Vec<PreparedNarrationUnit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedNarrationUnit {
    #[serde(rename = "id", alias = "unit_id")]
    pub unit_id: String,
    pub section_id: Option<String>,
    #[serde(default = "default_prepared_unit_status")]
    pub status: String,
    pub original_text: String,
    pub tts_text: String,
    pub sentences: Vec<PreparedSentence>,
    pub phoneme_chunks: Vec<String>,
    pub source_range: Option<SourceRange>,
    pub text_normalization: TextNormalizationReport,
    pub phoneme_normalization: PhonemeNormalizationReport,
    pub repairs: Vec<PreparedRepair>,
}

fn default_prepared_unit_status() -> String {
    "ready".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedRepair {
    pub rule: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedSentence {
    pub index: usize,
    pub text: String,
    pub phonemes: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PreflightOutcome {
    pub(crate) report: PreflightReport,
    pub(crate) prepared: PreparedNarration,
}

/// Validate every planned narration unit without starting a TTS worker.
pub(crate) fn preflight_book(
    book: &CanonicalBook,
    plan: &NarrationPlan,
    options: &PreflightOptions,
) -> Result<PreflightOutcome> {
    if options.max_characters == 0 {
        bail!("provider character limit must be greater than zero");
    }
    if options.max_phonemes == 0 {
        bail!("phoneme limit must be greater than zero");
    }

    let source_sha256 = sha256_file(&book.source.path)?;
    let mut collector = IssueCollector::default();
    for warning in &book.warnings {
        collector.warning(warning, true);
    }
    for warning in &plan.warnings {
        collector.warning(warning, false);
    }

    let phonemizer = Phonemizer::new(options.voice.is_british(), &options.pronunciations);
    let mut prepared_units = Vec::with_capacity(plan.units.len());
    let mut scanned_sentences = 0_usize;
    let mut automatic_repairs = 0_usize;
    let mut text_repairs = 0_usize;
    let mut best_effort_pronunciations = 0_usize;

    for unit in &plan.units {
        text_repairs += unit.normalization.count;
        let Some(prepared) = prepare_unit(
            unit,
            &phonemizer,
            options.max_characters,
            options.max_phonemes,
            &mut scanned_sentences,
            &mut automatic_repairs,
            &mut best_effort_pronunciations,
            &mut collector,
        ) else {
            continue;
        };
        prepared_units.push(prepared);
    }

    if plan.units.is_empty() {
        collector.add(PreflightIssue {
            signature: "empty_narration_unit:book".to_owned(),
            kind: PreflightIssueKind::EmptyNarrationUnit,
            token: None,
            unit_id: "book".to_owned(),
            sentence_index: 0,
            source_range: None,
            text: String::new(),
            occurrences: 1,
            suggestion: None,
            detail: "narration plan contains no spoken units".to_owned(),
        });
    }

    let unresolved = collector.unresolved_count();
    let mut issues_by_kind = collector.counts;
    if automatic_repairs > 0 {
        issues_by_kind.insert(
            PreflightIssueKind::AutomaticRepair.key().to_owned(),
            automatic_repairs,
        );
    }
    let report = PreflightReport {
        schema_version: PREFLIGHT_SCHEMA_VERSION,
        source_sha256: source_sha256.clone(),
        profile: options.profile(),
        normalization_version: PHONEME_NORMALIZATION_VERSION,
        scanned_units: plan.units.len(),
        scanned_sentences,
        automatic_repairs,
        text_repairs,
        best_effort_pronunciations,
        unresolved,
        issues_by_kind,
        issues: collector.issues,
    };
    let prepared = PreparedNarration {
        schema_version: PREPARED_NARRATION_SCHEMA_VERSION,
        complete: unresolved == 0,
        source_sha256,
        profile: options.profile(),
        provider_configuration_hash: options.configuration_hash(),
        normalization_version: PHONEME_NORMALIZATION_VERSION,
        max_phonemes: options.max_phonemes,
        max_characters: options.max_characters,
        units: prepared_units,
    };
    Ok(PreflightOutcome { report, prepared })
}

pub(crate) fn validate_prepared_artifact(
    book: &CanonicalBook,
    plan: &NarrationPlan,
    prepared: &PreparedNarration,
    options: &PreflightOptions,
) -> Result<()> {
    if !prepared.complete {
        bail!("prepared narration contains unresolved blocking issues");
    }
    if prepared.source_sha256 != sha256_file(&book.source.path)? {
        bail!("prepared narration source hash does not match the input");
    }
    if prepared.profile != options.profile()
        || prepared.provider_configuration_hash != options.configuration_hash()
        || prepared.normalization_version != PHONEME_NORMALIZATION_VERSION
        || prepared.max_phonemes != options.max_phonemes
        || prepared.max_characters != options.max_characters
    {
        bail!("prepared narration profile does not match this build");
    }
    if prepared.units.len() != plan.units.len()
        || prepared
            .units
            .iter()
            .zip(&plan.units)
            .any(|(prepared, unit)| {
                prepared.status != "ready"
                    || prepared.unit_id != unit.id
                    || prepared.tts_text != unit.text
                    || prepared.original_text != unit.original_text
            })
    {
        bail!("prepared narration does not match the current narration plan");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn prepare_unit(
    unit: &NarrationUnit,
    phonemizer: &Phonemizer,
    max_characters: usize,
    max_phonemes: usize,
    scanned_sentences: &mut usize,
    automatic_repairs: &mut usize,
    best_effort_pronunciations: &mut usize,
    collector: &mut IssueCollector,
) -> Option<PreparedNarrationUnit> {
    if unit.text.trim().is_empty() {
        collector.add(PreflightIssue {
            signature: format!("empty_narration_unit:{}", unit.id),
            kind: PreflightIssueKind::EmptyNarrationUnit,
            token: None,
            unit_id: unit.id.clone(),
            sentence_index: 0,
            source_range: unit.source_range.clone(),
            text: unit.text.clone(),
            occurrences: 1,
            suggestion: None,
            detail: "narration unit is empty after text normalization".to_owned(),
        });
        return None;
    }

    let text_chunks = match chunk_text(&unit.text, max_characters) {
        Ok(chunks) => chunks,
        Err(error) => {
            collector.add(issue_from_error(
                PreflightIssueKind::OversizedChunk,
                unit,
                0,
                error.to_string(),
            ));
            return None;
        }
    };
    let mut sentences = Vec::new();
    let mut normalized_sentences = Vec::new();
    let mut phoneme_chunks = Vec::new();
    let mut unit_repairs = PhonemeNormalizationReport::default();
    let mut best_effort_repairs = Vec::new();
    for text_chunk in text_chunks {
        let sentence_start = sentences.len();
        let normalized_start = normalized_sentences.len();
        for sentence in extract_sentences(&text_chunk) {
            *scanned_sentences += 1;
            let sentence_index = sentences.len() + 1;
            let mut best_effort_repair = None;
            let result = match validate_sentence(phonemizer, &sentence) {
                Ok(result) => Ok(result),
                Err(detail) => {
                    let token = if detail.contains("Misaki could not pronounce") {
                        find_unknown_token(phonemizer, &sentence)
                    } else {
                        None
                    };
                    if let Some(token) = token.as_deref()
                        && let Some((candidate, result)) =
                            try_best_effort_candidates(phonemizer, token)
                    {
                        best_effort_repair = Some(PreparedRepair {
                            rule: "best_effort_pronunciation".to_owned(),
                            from: token.to_owned(),
                            to: candidate,
                        });
                        Ok(result)
                    } else {
                        Err((detail, token))
                    }
                }
            };
            match result {
                Ok((phonemes, stats)) => {
                    unit_repairs.automatic_repairs += stats.automatic_repairs;
                    unit_repairs.syllabic_consonant += stats.syllabic_consonant;
                    *automatic_repairs += stats.automatic_repairs;
                    if best_effort_repair.is_some() {
                        *best_effort_pronunciations += 1;
                    }
                    normalized_sentences.push(phonemes.clone());
                    if let Some(repair) = best_effort_repair {
                        best_effort_repairs.push(repair);
                    }
                    sentences.push(PreparedSentence {
                        index: sentence_index,
                        text: sentence,
                        phonemes: Some(phonemes),
                    });
                }
                Err((detail, token)) => {
                    let kind = if token.is_some() {
                        PreflightIssueKind::UnknownG2pToken
                    } else if detail.contains("unsupported phoneme") {
                        PreflightIssueKind::UnsupportedPhoneme
                    } else {
                        PreflightIssueKind::UnknownG2pToken
                    };
                    collector.add(PreflightIssue {
                        signature: format!(
                            "{}:{}",
                            kind.key(),
                            token.as_deref().unwrap_or(&detail)
                        ),
                        kind,
                        token: token.clone(),
                        unit_id: unit.id.clone(),
                        sentence_index,
                        source_range: unit.source_range.clone(),
                        text: sentence.clone(),
                        occurrences: 1,
                        suggestion: token.clone(),
                        detail,
                    });
                    sentences.push(PreparedSentence {
                        index: sentence_index,
                        text: sentence,
                        phonemes: None,
                    });
                }
            }
        }
        let sentence_count = sentences.len() - sentence_start;
        let normalized_count = normalized_sentences.len() - normalized_start;
        if sentence_count == normalized_count {
            match pack_phoneme_sentences(&normalized_sentences[normalized_start..], max_phonemes) {
                Ok(chunks) => phoneme_chunks.extend(chunks),
                Err(error) => collector.add(issue_from_error(
                    PreflightIssueKind::OversizedChunk,
                    unit,
                    0,
                    error.to_string(),
                )),
            }
        }
    }

    let mut repairs = unit
        .normalization
        .repairs
        .iter()
        .map(|repair| PreparedRepair {
            rule: repair.rule.clone(),
            from: repair.from.clone(),
            to: repair.to.clone(),
        })
        .collect::<Vec<_>>();
    repairs.extend(best_effort_repairs);
    if unit_repairs.syllabic_consonant > 0 {
        repairs.push(PreparedRepair {
            rule: "syllabic_consonant".to_owned(),
            from: "U+0329".to_owned(),
            to: "Kokoro-compatible schwa consonant".to_owned(),
        });
    }

    let status = if sentences.iter().any(|sentence| sentence.phonemes.is_none()) {
        "blocked"
    } else {
        "ready"
    };
    Some(PreparedNarrationUnit {
        unit_id: unit.id.clone(),
        section_id: unit.section_id.clone(),
        status: status.to_owned(),
        original_text: unit.original_text.clone(),
        tts_text: unit.text.clone(),
        sentences,
        phoneme_chunks,
        source_range: unit.source_range.clone(),
        text_normalization: unit.normalization.clone(),
        phoneme_normalization: unit_repairs,
        repairs,
    })
}

fn issue_from_error(
    kind: PreflightIssueKind,
    unit: &NarrationUnit,
    sentence_index: usize,
    detail: String,
) -> PreflightIssue {
    PreflightIssue {
        signature: format!("{}:{}", kind.key(), detail),
        kind,
        token: None,
        unit_id: unit.id.clone(),
        sentence_index,
        source_range: unit.source_range.clone(),
        text: unit.text.clone(),
        occurrences: 1,
        suggestion: None,
        detail,
    }
}

fn find_unknown_token(phonemizer: &Phonemizer, sentence: &str) -> Option<String> {
    sentence
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|character: char| !character.is_alphanumeric() && character != '\'')
        })
        .filter(|token| !token.is_empty())
        .find(|token| phonemizer.phonemize(token).is_err())
        .map(str::to_owned)
}

fn validate_sentence(
    phonemizer: &Phonemizer,
    sentence: &str,
) -> std::result::Result<(String, PhonemeNormalizationReport), String> {
    let normalized = phonemizer
        .phonemize(sentence)
        .map_err(|error| error.to_string())?;
    let phonemes =
        vocab::normalized_phonemes(&normalized.phonemes).map_err(|error| error.to_string())?;
    Ok((
        phonemes,
        PhonemeNormalizationReport {
            automatic_repairs: normalized.stats.automatic_repairs,
            syllabic_consonant: normalized.stats.syllabic_consonant,
        },
    ))
}

fn try_best_effort_candidates(
    phonemizer: &Phonemizer,
    token: &str,
) -> Option<(String, (String, PhonemeNormalizationReport))> {
    let mut candidates = Vec::new();
    let stripped = token
        .trim_matches(|character: char| !character.is_alphanumeric() && character != '\'')
        .to_owned();
    if !stripped.is_empty() {
        candidates.push(stripped.clone());
        candidates.push(stripped.to_lowercase());
        let ascii = ascii_fold(&stripped);
        if ascii != stripped {
            candidates.push(ascii);
        }
        if stripped
            .chars()
            .all(|character| character.is_ascii_alphabetic())
        {
            candidates.push(
                stripped
                    .chars()
                    .map(|character| character.to_ascii_uppercase().to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
    }
    candidates.dedup();
    candidates.into_iter().find_map(|candidate| {
        validate_sentence(phonemizer, &candidate)
            .ok()
            .map(|result| (candidate, result))
    })
}

fn ascii_fold(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'ē' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'ī' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'ū' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            'ý' | 'ÿ' => 'y',
            'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ā' => 'A',
            'É' | 'È' | 'Ê' | 'Ë' | 'Ē' => 'E',
            'Í' | 'Ì' | 'Î' | 'Ï' | 'Ī' => 'I',
            'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ō' => 'O',
            'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ū' => 'U',
            'Ñ' => 'N',
            'Ç' => 'C',
            'Ý' => 'Y',
            other => other,
        })
        .collect()
}

#[derive(Default)]
struct IssueCollector {
    issues: Vec<PreflightIssue>,
    counts: BTreeMap<String, usize>,
}

impl IssueCollector {
    fn add(&mut self, mut issue: PreflightIssue) {
        let key = issue.kind.key().to_owned();
        *self.counts.entry(key).or_default() += issue.occurrences;
        if let Some(existing) = self
            .issues
            .iter_mut()
            .find(|existing| existing.signature == issue.signature)
        {
            existing.occurrences += issue.occurrences;
            return;
        }
        issue.occurrences = issue.occurrences.max(1);
        self.issues.push(issue);
    }

    fn warning(&mut self, warning: &str, source: bool) {
        let kind = if source {
            PreflightIssueKind::SourceParseWarning
        } else {
            PreflightIssueKind::ExcludedByPolicy
        };
        self.add(PreflightIssue {
            signature: format!("{}:{warning}", kind.key()),
            kind,
            token: None,
            unit_id: "book".to_owned(),
            sentence_index: 0,
            source_range: None,
            text: String::new(),
            occurrences: 1,
            suggestion: None,
            detail: warning.to_owned(),
        });
    }

    fn unresolved_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.kind.is_unresolved())
            .map(|issue| issue.occurrences)
            .sum()
    }
}

/// Write a JSON preflight report atomically.
pub(crate) fn write_report(path: &Path, report: &PreflightReport) -> Result<()> {
    let bytes =
        serde_json::to_vec_pretty(report).context("failed to serialize preflight report")?;
    write_atomic(path, &bytes)
}

/// Write prepared narration as a metadata header followed by JSONL units.
pub(crate) fn write_prepared(path: &Path, prepared: &PreparedNarration) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create prepared narration directory {}",
            parent.display()
        )
    })?;
    let temporary = NamedTempFile::new_in(parent).with_context(|| {
        format!(
            "failed to create prepared narration in {}",
            parent.display()
        )
    })?;
    let (file, temporary) = temporary.into_parts();
    let mut writer = BufWriter::new(file);
    let header = PreparedHeader::from_prepared(prepared);
    serde_json::to_writer(&mut writer, &header).context("failed to write prepared header")?;
    writer
        .write_all(b"\n")
        .context("failed to write prepared newline")?;
    for unit in &prepared.units {
        serde_json::to_writer(&mut writer, unit).context("failed to write prepared unit")?;
        writer
            .write_all(b"\n")
            .context("failed to write prepared newline")?;
    }
    writer
        .flush()
        .context("failed to flush prepared narration")?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to persist prepared narration {}", path.display()))?;
    Ok(())
}

/// Write one suggestion per unresolved lexical token.
pub(crate) fn write_suggestions(path: &Path, report: &PreflightReport) -> Result<()> {
    let mut suggestions = report
        .issues
        .iter()
        .filter_map(|issue| {
            (issue.kind == PreflightIssueKind::UnknownG2pToken)
                .then(|| issue.token.clone())
                .flatten()
        })
        .collect::<Vec<_>>();
    suggestions.sort();
    suggestions.dedup();
    let mut text = String::from("# Generated by audiobook-forge preflight. Review before use.\n");
    for token in suggestions {
        text.push_str(&token);
        text.push_str("=\n");
    }
    write_atomic(path, text.as_bytes())
}

/// Write one TTS-ready text file per contiguous narration section.
pub(crate) fn write_chapter_texts(directory: &Path, prepared: &PreparedNarration) -> Result<()> {
    fs::create_dir_all(directory).with_context(|| {
        format!(
            "failed to create chapter text directory {}",
            directory.display()
        )
    })?;
    let mut chapter_index = 0_usize;
    let mut current_section: Option<String> = None;
    let mut current_text = String::new();
    for unit in &prepared.units {
        if current_section.as_ref() != unit.section_id.as_ref() && !current_text.is_empty() {
            chapter_index += 1;
            let name = chapter_filename(chapter_index, current_section.as_deref());
            write_atomic(&directory.join(name), current_text.trim_end().as_bytes())?;
            current_text.clear();
        }
        current_section.clone_from(&unit.section_id);
        if !current_text.is_empty() {
            current_text.push('\n');
        }
        current_text.push_str(&unit.tts_text);
    }
    if !current_text.is_empty() {
        chapter_index += 1;
        let name = chapter_filename(chapter_index, current_section.as_deref());
        write_atomic(&directory.join(name), current_text.trim_end().as_bytes())?;
    }
    Ok(())
}

fn chapter_filename(index: usize, section: Option<&str>) -> String {
    let label = section
        .unwrap_or("narration")
        .split(':')
        .next_back()
        .unwrap_or("narration")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("{index:03}-{label}.txt")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create preflight directory {}", parent.display()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create preflight file in {}", parent.display()))?;
    temporary
        .write_all(bytes)
        .context("failed to write preflight output")?;
    temporary
        .flush()
        .context("failed to flush preflight output")?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to persist preflight output {}", path.display()))?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| {
        format!(
            "failed to open source for preflight hashing {}",
            path.display()
        )
    })?;
    let mut reader = std::io::BufReader::new(file);
    let mut digest = Sha256::new();
    std::io::copy(&mut reader, &mut digest).context("failed to hash source for preflight")?;
    Ok(format!("{:x}", digest.finalize()))
}

#[derive(Debug, Serialize)]
struct PreparedHeader {
    kind: &'static str,
    schema_version: u32,
    complete: bool,
    source_sha256: String,
    profile: String,
    provider_configuration_hash: String,
    normalization_version: u32,
    max_phonemes: usize,
    max_characters: usize,
}

impl PreparedHeader {
    fn from_prepared(prepared: &PreparedNarration) -> Self {
        Self {
            kind: "metadata",
            schema_version: prepared.schema_version,
            complete: prepared.complete,
            source_sha256: prepared.source_sha256.clone(),
            profile: prepared.profile.clone(),
            provider_configuration_hash: prepared.provider_configuration_hash.clone(),
            normalization_version: prepared.normalization_version,
            max_phonemes: prepared.max_phonemes,
            max_characters: prepared.max_characters,
        }
    }
}

/// Read a prepared JSONL artifact and validate its structure.
///
/// # Errors
///
/// Returns an error when the artifact cannot be read or contains invalid JSONL.
pub fn read_prepared(path: &Path) -> Result<PreparedNarration> {
    let file = File::open(path)
        .with_context(|| format!("failed to open prepared narration {}", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .transpose()
        .context("failed to read prepared header")?
        .context("prepared narration is empty")?;
    let header: PreparedHeaderOwned =
        serde_json::from_str(&header).context("invalid prepared narration header")?;
    if header.kind != "metadata" {
        bail!("prepared narration header has invalid kind");
    }
    let mut units = Vec::new();
    for line in lines {
        let line = line.context("failed to read prepared narration unit")?;
        if line.trim().is_empty() {
            continue;
        }
        units.push(serde_json::from_str(&line).context("invalid prepared narration unit")?);
    }
    Ok(PreparedNarration {
        schema_version: header.schema_version,
        complete: header.complete,
        source_sha256: header.source_sha256,
        profile: header.profile,
        provider_configuration_hash: header.provider_configuration_hash,
        normalization_version: header.normalization_version,
        max_phonemes: header.max_phonemes,
        max_characters: header.max_characters,
        units,
    })
}

#[derive(Debug, Deserialize)]
struct PreparedHeaderOwned {
    kind: String,
    schema_version: u32,
    complete: bool,
    source_sha256: String,
    profile: String,
    provider_configuration_hash: String,
    normalization_version: u32,
    max_phonemes: usize,
    max_characters: usize,
}

#[cfg(test)]
mod tests {
    use super::{PreflightOptions, preflight_book};
    use crate::book::{
        Block, BookMetadata, CanonicalBook, Provenance, Section, SectionKind, SourceDocument,
        SourceFormat, TextBlock,
    };
    use crate::narration::{NarrationPolicy, plan_narration};
    use crate::voice::Voice;

    fn book(text: &str) -> CanonicalBook {
        let mut root = Section::new(
            "book",
            SectionKind::Book,
            Some("Book".to_owned()),
            0,
            Provenance::Authored,
        );
        let mut chapter = Section::new(
            "chapter",
            SectionKind::Chapter,
            Some("Chapter".to_owned()),
            1,
            Provenance::Authored,
        );
        chapter.blocks.push(Block::Paragraph(TextBlock {
            text: text.to_owned(),
            source_range: None,
        }));
        root.children.push(chapter);
        CanonicalBook {
            metadata: BookMetadata {
                title: Some("Book".to_owned()),
                ..BookMetadata::default()
            },
            root,
            source: SourceDocument {
                path: std::env::temp_dir().join("audiobook-forge-preflight-test.txt"),
                format: SourceFormat::Text,
                format_version: None,
            },
            text: text.to_owned(),
            pages: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn batch_reports_all_unknown_tokens_and_deduplicates() {
        let path = std::env::temp_dir().join("audiobook-forge-preflight-test.txt");
        std::fs::write(&path, "First.").expect("source");
        let book = book("❓. ❓.");
        let plan = plan_narration(&book, NarrationPolicy::default());
        let voice = "af_heart".parse::<Voice>().expect("voice");
        let outcome = preflight_book(
            &book,
            &plan,
            &PreflightOptions {
                voice,
                pronunciations: Vec::new(),
                max_phonemes: 200,
                max_characters: 400,
            },
        )
        .expect("preflight");

        assert!(outcome.report.scanned_units >= 2);
        assert!(outcome.report.unresolved >= 2);
        assert!(
            outcome
                .report
                .issues
                .iter()
                .any(|issue| { issue.kind == super::PreflightIssueKind::UnknownG2pToken })
        );
        assert!(
            outcome
                .report
                .issues
                .iter()
                .any(|issue| issue.occurrences >= 2)
        );
    }

    #[test]
    fn known_syllabic_repairs_are_clean_and_prepared() {
        let path = std::env::temp_dir().join("audiobook-forge-preflight-test.txt");
        std::fs::write(&path, "Written and certain.").expect("source");
        let book = book("Written and certain.");
        let plan = plan_narration(&book, NarrationPolicy::default());
        let voice = "af_heart".parse::<Voice>().expect("voice");
        let outcome = preflight_book(
            &book,
            &plan,
            &PreflightOptions {
                voice,
                pronunciations: Vec::new(),
                max_phonemes: 200,
                max_characters: 400,
            },
        )
        .expect("preflight");

        assert_eq!(outcome.report.unresolved, 0);
        assert!(outcome.report.automatic_repairs >= 2);
        assert!(outcome.prepared.complete);
        assert!(!outcome.prepared.units[0].phoneme_chunks.is_empty());
    }
}
