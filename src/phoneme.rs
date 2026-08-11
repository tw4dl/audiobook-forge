//! English grapheme-to-phoneme conversion without eSpeak.

use std::fmt::{self, Display};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use misaki_rs::lexicon::PhonemeEntry;
use misaki_rs::{G2P, Language};

use crate::vocab::{self, NormalizedPhonemes};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pronunciation {
    word: String,
    phonemes: String,
}

impl Display for Pronunciation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}={}", self.word, self.phonemes)
    }
}

impl FromStr for Pronunciation {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((word, phonemes)) = value.split_once('=') else {
            bail!("pronunciation must use WORD=IPA");
        };
        let word = word.trim();
        let phonemes = phonemes.trim();
        if word.is_empty() || word.chars().any(char::is_whitespace) || phonemes.is_empty() {
            bail!("pronunciation must use WORD=IPA with one non-empty word");
        }
        let normalized = vocab::normalize_for_kokoro(phonemes);
        if !vocab::supports(&normalized.phonemes) {
            bail!("pronunciation for '{word}' contains unsupported phoneme characters");
        }
        Ok(Self {
            word: word.to_owned(),
            phonemes: phonemes.to_owned(),
        })
    }
}

pub(crate) struct Phonemizer {
    inner: G2P,
}

impl Phonemizer {
    pub(crate) fn new(british: bool, pronunciations: &[Pronunciation]) -> Self {
        let language = if british {
            Language::EnglishGB
        } else {
            Language::EnglishUS
        };
        let mut inner = G2P::new(language);
        for pronunciation in pronunciations {
            let entry = PhonemeEntry::Simple(pronunciation.phonemes.clone());
            for spelling in spellings(&pronunciation.word) {
                inner.lexicon.golds.insert(spelling, entry.clone());
            }
        }
        Self { inner }
    }

    /// Convert English prose to Kokoro-compatible IPA.
    ///
    /// # Errors
    ///
    /// Returns an error when lean Misaki cannot produce usable phonemes.
    pub(crate) fn phonemize(&self, text: &str) -> Result<NormalizedPhonemes> {
        let text = text.replace(['‘', '’'], "'");
        let (phonemes, _) = self
            .inner
            .g2p(&text)
            .context("Misaki failed to phonemize text")?;
        if phonemes.contains('❓') {
            bail!("Misaki could not pronounce part of the text; add --pronunciation WORD=IPA");
        }
        Ok(vocab::normalize_for_kokoro(&phonemes))
    }
}

fn spellings(word: &str) -> Vec<String> {
    let lower = word.to_lowercase();
    let mut forms = vec![word.to_owned()];
    if lower != word {
        forms.push(lower.clone());
    }
    let mut characters = lower.chars();
    if let Some(first) = characters.next() {
        let title = first.to_uppercase().collect::<String>() + characters.as_str();
        if !forms.contains(&title) {
            forms.push(title);
        }
    }
    forms
}

#[cfg(test)]
mod tests {
    use super::{Phonemizer, Pronunciation};

    #[test]
    fn parses_a_book_specific_pronunciation() {
        let pronunciation: Pronunciation = "Cormer=kˈɔɹmɚ".parse().expect("valid override");

        assert_eq!(pronunciation.word, "Cormer");
        assert_eq!(pronunciation.phonemes, "kˈɔɹmɚ");
    }

    #[test]
    fn rejects_incomplete_pronunciations() {
        for value in ["Cormer", "=kˈɔɹmɚ", "Cormer="] {
            let error = value
                .parse::<Pronunciation>()
                .expect_err("invalid override must fail");
            assert!(
                error
                    .to_string()
                    .contains("pronunciation must use WORD=IPA")
            );
        }
    }

    #[test]
    fn applies_an_override_before_phonemizing() {
        let pronunciation: Pronunciation = "Cormer=kˈɔɹmɚ".parse().expect("valid override");
        let phonemizer = Phonemizer::new(false, &[pronunciation]);

        let phonemes = phonemizer.phonemize("Cormer").expect("phonemes");

        assert_eq!(phonemes.phonemes.trim(), "kˈɔɹmɚ");
    }

    #[test]
    fn fixes_a_name_that_lean_misaki_otherwise_spells_out() {
        let baseline = Phonemizer::new(false, &[])
            .phonemize("Elena")
            .expect("baseline phonemes");
        let pronunciation: Pronunciation = "Elena=ɪlˈeɪnə".parse().expect("valid override");
        let corrected = Phonemizer::new(false, &[pronunciation])
            .phonemize("Elena")
            .expect("corrected phonemes");

        assert_ne!(baseline.phonemes.trim(), "ɪlˈeɪnə");
        assert_eq!(corrected.phonemes.trim(), "ɪlˈeɪnə");
    }

    #[test]
    fn handles_curly_quotes_from_an_epub() {
        let text = "‘Why so?’ inquired Elena. ‘One would think you were speaking of some spiteful, disagreeable old woman.’";

        Phonemizer::new(false, &[])
            .phonemize(text)
            .expect("book typography must phonemize");
    }

    #[test]
    fn repairs_common_syllabic_consonants_after_g2p() {
        let phonemizer = Phonemizer::new(false, &[]);

        for word in [
            "written",
            "certain",
            "frightened",
            "heightened",
            "enlightening",
        ] {
            let normalized = phonemizer.phonemize(word).expect("phonemes");
            assert!(!normalized.phonemes.contains('\u{0329}'), "{word}");
            assert!(normalized.stats.automatic_repairs > 0, "{word}");
        }
    }

    #[test]
    fn explicit_pronunciation_is_normalized_before_validation() {
        let pronunciation: Pronunciation = "written=ɹˈɪʔn̩".parse().expect("override");
        let phonemizer = Phonemizer::new(false, &[pronunciation]);
        let normalized = phonemizer.phonemize("written").expect("phonemes");

        assert_eq!(normalized.phonemes.trim(), "ɹˈɪʔən");
        assert_eq!(normalized.stats.automatic_repairs, 1);
    }
}
