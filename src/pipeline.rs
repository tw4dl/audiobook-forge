//! Sentence-oriented phoneme packing for bounded Kokoro requests.

use anyhow::{Context, Result, bail};

use crate::chunk::chunk_text;
use crate::phoneme::{Phonemizer, Pronunciation};
use crate::vocab::{self, PhonemeNormalizationStats};
use crate::voice::Voice;

pub(crate) struct PhonemizationResult {
    pub(crate) chunks: Vec<String>,
    pub(crate) normalization: PhonemeNormalizationStats,
}

pub(crate) fn phonemize_book(
    text: &str,
    voice: Voice,
    pronunciations: &[Pronunciation],
    max_phonemes: usize,
) -> Result<PhonemizationResult> {
    let sentences = extract_sentences(text);
    if sentences.is_empty() {
        bail!("input contains no readable text");
    }
    let phonemizer = Phonemizer::new(voice.is_british(), pronunciations);
    let mut normalization = PhonemeNormalizationStats::default();
    let phoneme_sentences = sentences
        .iter()
        .enumerate()
        .map(|(index, sentence)| {
            let phonemes = phonemizer
                .phonemize(sentence)
                .with_context(|| format!("failed to phonemize sentence {}", index + 1))?;
            normalization.add_assign(&phonemes.stats);
            vocab::normalized_phonemes(&phonemes.phonemes)
                .with_context(|| format!("invalid phonemes in sentence {}", index + 1))
        })
        .collect::<Result<Vec<_>>>()?;
    let chunks = pack_phoneme_sentences(&phoneme_sentences, max_phonemes)?;
    for (index, chunk) in chunks.iter().enumerate() {
        vocab::token_ids(chunk).with_context(|| format!("invalid phoneme chunk {}", index + 1))?;
    }
    Ok(PhonemizationResult {
        chunks,
        normalization,
    })
}

/// Extract sentence-sized prose units while keeping terminal punctuation.
pub fn extract_sentences(text: &str) -> Vec<String> {
    let characters = text.char_indices().collect::<Vec<_>>();
    let mut sentences = Vec::new();
    let mut start = 0_usize;
    let mut index = 0_usize;

    while index < characters.len() {
        let (byte_index, character) = characters[index];
        if !matches!(character, '.' | '!' | '?' | '…')
            || (character == '.' && is_abbreviation(text, byte_index))
        {
            index += 1;
            continue;
        }

        let mut after_terminal = index + 1;
        while after_terminal < characters.len()
            && matches!(
                characters[after_terminal].1,
                '"' | '\'' | '”' | '’' | ')' | ']'
            )
        {
            after_terminal += 1;
        }
        if after_terminal < characters.len() && !characters[after_terminal].1.is_whitespace() {
            index += 1;
            continue;
        }

        let end = characters
            .get(after_terminal)
            .map_or(text.len(), |(position, _)| *position);
        push_trimmed(&mut sentences, &text[start..end]);
        while after_terminal < characters.len() && characters[after_terminal].1.is_whitespace() {
            after_terminal += 1;
        }
        start = characters
            .get(after_terminal)
            .map_or(text.len(), |(position, _)| *position);
        index = after_terminal;
    }

    if start < text.len() {
        push_trimmed(&mut sentences, &text[start..]);
    }
    sentences
}

/// Pack phonemized sentences without exceeding `max_phonemes` Unicode tokens.
///
/// Complete sentences stay together when they fit. An oversized sentence falls
/// back to word boundaries, then Unicode scalar boundaries for one long token.
///
/// # Errors
///
/// Returns an error when `max_phonemes` is zero.
pub fn pack_phoneme_sentences(sentences: &[String], max_phonemes: usize) -> Result<Vec<String>> {
    if max_phonemes == 0 {
        bail!("phoneme limit must be greater than zero");
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for sentence in sentences {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }

        let sentence_len = sentence.chars().count();
        if sentence_len > max_phonemes {
            flush(&mut chunks, &mut current);
            chunks.extend(chunk_text(sentence, max_phonemes)?);
            continue;
        }

        let separator_len = usize::from(!current.is_empty());
        if current.chars().count() + separator_len + sentence_len > max_phonemes {
            flush(&mut chunks, &mut current);
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(sentence);
    }

    flush(&mut chunks, &mut current);
    Ok(chunks)
}

fn flush(chunks: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        chunks.push(std::mem::take(current));
    }
}

fn push_trimmed(sentences: &mut Vec<String>, sentence: &str) {
    let sentence = sentence.trim();
    if !sentence.is_empty() {
        sentences.push(sentence.to_owned());
    }
}

fn is_abbreviation(text: &str, period: usize) -> bool {
    let prefix = &text[..period];
    let word = prefix
        .rsplit(|character: char| !character.is_alphabetic())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    word.chars().count() == 1
        || matches!(
            word.as_str(),
            "dr" | "mr" | "mrs" | "ms" | "prof" | "sr" | "jr" | "st" | "vs" | "etc"
        )
}
