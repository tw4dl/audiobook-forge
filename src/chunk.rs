//! Text chunking that keeps Kokoro requests bounded.

use anyhow::{Result, bail};

/// Split normalized text into chunks no longer than `max_chars` Unicode characters.
///
/// # Errors
///
/// Returns an error when `max_chars` is zero.
pub fn chunk_text(text: &str, max_chars: usize) -> Result<Vec<String>> {
    if max_chars == 0 {
        bail!("chunk size must be greater than zero");
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        let separator_len = usize::from(!current.is_empty());

        if current.chars().count() + separator_len + word_len <= max_chars {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            continue;
        }

        if !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }

        if word_len <= max_chars {
            current.push_str(word);
            continue;
        }

        let mut part = String::new();
        for character in word.chars() {
            part.push(character);
            if part.chars().count() == max_chars {
                chunks.push(std::mem::take(&mut part));
            }
        }
        current = part;
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    Ok(chunks)
}
