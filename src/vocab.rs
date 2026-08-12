//! Kokoro v1.0 phoneme vocabulary.
//!
//! The mapping comes from the Apache-2.0 `hexgrad/Kokoro-82M` configuration at
//! revision `f3ff3571791e39611d31c381e3a41a3af07b4987`.

use anyhow::{Result, bail};

// MLX voice packs have 510 frames indexed by phoneme count minus one.
pub const MAX_PHONEMES: usize = 510;
pub(crate) const PHONEME_NORMALIZATION_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PhonemeNormalizationStats {
    pub(crate) automatic_repairs: usize,
    pub(crate) syllabic_consonant: usize,
}

impl PhonemeNormalizationStats {
    pub(crate) fn add_assign(&mut self, other: &Self) {
        self.automatic_repairs += other.automatic_repairs;
        self.syllabic_consonant += other.syllabic_consonant;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedPhonemes {
    pub(crate) phonemes: String,
    pub(crate) stats: PhonemeNormalizationStats,
}

/// Normalize Misaki output to the Kokoro phoneme vocabulary.
///
/// The source text is not involved in this transformation. Only the known
/// syllabic-consonant compatibility marker is repaired; unrelated characters
/// remain in the result so vocabulary validation can report them.
pub(crate) fn normalize_for_kokoro(raw_phonemes: &str) -> NormalizedPhonemes {
    let mut phonemes = String::with_capacity(raw_phonemes.len());
    let mut stats = PhonemeNormalizationStats::default();

    for raw in raw_phonemes.chars() {
        if matches!(raw, '\u{200c}' | '\u{200d}' | '\u{feff}') {
            continue;
        }
        if raw == '\u{0329}' {
            let Some(previous) = phonemes.pop() else {
                phonemes.push(raw);
                continue;
            };
            let replacement = match previous {
                'n' => "ən",
                'l' => "əl",
                'm' => "əm",
                'ɹ' => "əɹ",
                'r' => "ər",
                other => {
                    phonemes.push(other);
                    phonemes.push(raw);
                    continue;
                }
            };
            phonemes.push_str(replacement);
            stats.automatic_repairs += 1;
            stats.syllabic_consonant += 1;
            continue;
        }
        phonemes.push(raw);
    }

    NormalizedPhonemes { phonemes, stats }
}

const VOCAB: &[(char, i64)] = &[
    (';', 1),
    (':', 2),
    (',', 3),
    ('.', 4),
    ('!', 5),
    ('?', 6),
    ('—', 9),
    ('…', 10),
    ('"', 11),
    ('(', 12),
    (')', 13),
    ('“', 14),
    ('”', 15),
    (' ', 16),
    ('\u{0303}', 17),
    ('ʣ', 18),
    ('ʥ', 19),
    ('ʦ', 20),
    ('ʨ', 21),
    ('ᵝ', 22),
    ('ꭧ', 23),
    ('A', 24),
    ('I', 25),
    ('O', 31),
    ('Q', 33),
    ('S', 35),
    ('T', 36),
    ('W', 39),
    ('Y', 41),
    ('ᵊ', 42),
    ('a', 43),
    ('b', 44),
    ('c', 45),
    ('d', 46),
    ('e', 47),
    ('f', 48),
    ('h', 50),
    ('i', 51),
    ('j', 52),
    ('k', 53),
    ('l', 54),
    ('m', 55),
    ('n', 56),
    ('o', 57),
    ('p', 58),
    ('q', 59),
    ('r', 60),
    ('s', 61),
    ('t', 62),
    ('u', 63),
    ('v', 64),
    ('w', 65),
    ('x', 66),
    ('y', 67),
    ('z', 68),
    ('ɑ', 69),
    ('ɐ', 70),
    ('ɒ', 71),
    ('æ', 72),
    ('β', 75),
    ('ɔ', 76),
    ('ɕ', 77),
    ('ç', 78),
    ('ɖ', 80),
    ('ð', 81),
    ('ʤ', 82),
    ('ə', 83),
    ('ɚ', 85),
    ('ɛ', 86),
    ('ɜ', 87),
    ('ɟ', 90),
    ('ɡ', 92),
    ('ɥ', 99),
    ('ɨ', 101),
    ('ɪ', 102),
    ('ʝ', 103),
    ('ɯ', 110),
    ('ɰ', 111),
    ('ŋ', 112),
    ('ɳ', 113),
    ('ɲ', 114),
    ('ɴ', 115),
    ('ø', 116),
    ('ɸ', 118),
    ('θ', 119),
    ('œ', 120),
    ('ɹ', 123),
    ('ɾ', 125),
    ('ɻ', 126),
    ('ʁ', 128),
    ('ɽ', 129),
    ('ʂ', 130),
    ('ʃ', 131),
    ('ʈ', 132),
    ('ʧ', 133),
    ('ʊ', 135),
    ('ʋ', 136),
    ('ʌ', 138),
    ('ɣ', 139),
    ('ɤ', 140),
    ('χ', 142),
    ('ʎ', 143),
    ('ʒ', 147),
    ('ʔ', 148),
    ('ˈ', 156),
    ('ˌ', 157),
    ('ː', 158),
    ('ʰ', 162),
    ('ʲ', 164),
    ('↓', 169),
    ('→', 171),
    ('↗', 172),
    ('↘', 173),
    ('ᵻ', 177),
];

/// Convert IPA text to a padded Kokoro token sequence.
///
/// # Errors
///
/// Returns an error for empty, unsupported, or oversized input.
pub fn token_ids(phonemes: &str) -> Result<Vec<i64>> {
    let mut ids = vec![0];
    let phonemes = normalized_phonemes(phonemes)?;

    for phone in phonemes.chars() {
        let id = VOCAB
            .iter()
            .find(|(candidate, _)| *candidate == phone)
            .map(|(_, id)| *id)
            .expect("normalized phonemes are in the vocabulary");
        ids.push(id);
    }

    let count = ids.len() - 1;
    if count > MAX_PHONEMES {
        bail!(
            "phoneme sequence has {count} tokens; maximum is {MAX_PHONEMES}; reduce --chunk-phonemes"
        );
    }
    ids.push(0);
    Ok(ids)
}

pub(crate) fn normalized_phonemes(phonemes: &str) -> Result<String> {
    let mut normalized = String::with_capacity(phonemes.len());
    let mut unsupported = Vec::new();
    let phonemes = normalize_for_kokoro(phonemes).phonemes;

    for raw in phonemes.chars() {
        let phone = match raw {
            'ɝ' => 'ɚ',
            '`' | '´' => 'ˈ',
            other => other,
        };
        if VOCAB.iter().any(|(candidate, _)| *candidate == phone) {
            normalized.push(phone);
        } else if !unsupported.contains(&phone) {
            unsupported.push(phone);
        }
    }

    if !unsupported.is_empty() {
        let characters = unsupported.iter().collect::<String>();
        bail!("unsupported phoneme characters '{characters}'; use --pronunciation WORD=IPA");
    }
    if normalized.is_empty() {
        bail!("phonemizer returned no speech sounds");
    }
    Ok(normalized)
}

pub fn supports(phonemes: &str) -> bool {
    phonemes.chars().all(|raw| {
        matches!(raw, '\u{200c}' | '\u{200d}' | '\u{feff}')
            || VOCAB.iter().any(|(phone, _)| {
                *phone
                    == match raw {
                        'ɝ' => 'ɚ',
                        '`' | '´' => 'ˈ',
                        other => other,
                    }
            })
    })
}

#[cfg(test)]
mod tests {
    use super::{normalize_for_kokoro, normalized_phonemes, token_ids};

    #[test]
    fn normalizes_syllabic_n_without_touching_source_text() {
        let raw = "ɹˈɪʔn̩";
        let normalized = normalize_for_kokoro(raw);

        assert_eq!(normalized.phonemes, "ɹˈɪʔən");
        assert_eq!(normalized.stats.automatic_repairs, 1);
        assert_eq!(normalized.stats.syllabic_consonant, 1);
        assert_eq!(raw, "ɹˈɪʔn̩");
    }

    #[test]
    fn normalizes_common_syllabic_words() {
        for (raw, expected) in [
            ("sˈɜːʔn̩", "sˈɜːʔən"),
            ("fɹˈaɪʔn̩d", "fɹˈaɪʔənd"),
            ("hˈaɪʔn̩d", "hˈaɪʔənd"),
            ("ɛnlˈaɪʔn̩ɪŋ", "ɛnlˈaɪʔənɪŋ"),
        ] {
            assert_eq!(normalize_for_kokoro(raw).phonemes, expected);
        }
    }

    #[test]
    fn normalizes_all_supported_syllabic_consonants() {
        let normalized = normalize_for_kokoro("n̩ l̩ m̩ ɹ̩ r̩");

        assert_eq!(normalized.phonemes, "ən əl əm əɹ ər");
        assert_eq!(normalized.stats.automatic_repairs, 5);
    }

    #[test]
    fn leaves_unrelated_unsupported_characters_diagnosable() {
        let normalized = normalize_for_kokoro("h🙂");

        assert_eq!(normalized.phonemes, "h🙂");
        assert_eq!(normalized.stats.automatic_repairs, 0);
        let error = normalized_phonemes(&normalized.phonemes).expect_err("unknown phoneme");
        assert!(
            error
                .to_string()
                .contains("unsupported phoneme characters '🙂'")
        );
    }

    #[test]
    fn normalization_is_idempotent() {
        let first = normalize_for_kokoro("ɹˈɪʔn̩ l̩");
        let second = normalize_for_kokoro(&first.phonemes);

        assert_eq!(first.phonemes, second.phonemes);
        assert_eq!(second.stats.automatic_repairs, 0);
    }

    #[test]
    fn pads_and_normalizes_phonemes() {
        assert_eq!(token_ids("hɝ").expect("tokens"), [0, 50, 85, 0]);
        assert_eq!(token_ids("e\u{200d}ɪ").expect("tokens"), [0, 47, 102, 0]);
    }

    #[test]
    fn keeps_the_style_index_within_a_510_frame_voice() {
        assert!(token_ids(&"a".repeat(510)).is_ok());

        let error = token_ids(&"a".repeat(511)).expect_err("style frame 511 must be rejected");

        assert!(error.to_string().contains("maximum is 510"));
    }

    #[test]
    fn rejects_unsupported_phonemes() {
        let error = token_ids("h🙂").expect_err("unknown phoneme must fail");

        assert!(
            error
                .to_string()
                .contains("unsupported phoneme characters '🙂'")
        );
    }

    #[test]
    fn normalizes_aliases_before_worker_synthesis() {
        assert_eq!(
            normalized_phonemes("hɝ`e\u{200d}").expect("phonemes"),
            "hɚˈe"
        );
    }
}
