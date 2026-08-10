use kokoro_book::chunk::chunk_text;

#[test]
fn keeps_chunks_within_the_requested_character_limit() {
    let text = "First short sentence. Second sentence has a few more words. Third sentence.";
    let chunks = chunk_text(text, 32).expect("valid limit");

    assert!(chunks.len() >= 2);
    assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 32));
    assert_eq!(chunks.join(" "), text);
}

#[test]
fn splits_a_single_long_word_without_losing_text() {
    let text = "supercalifragilisticexpialidocious";
    let chunks = chunk_text(text, 10).expect("valid limit");

    assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 10));
    assert_eq!(chunks.concat(), text);
}

#[test]
fn rejects_an_unsafe_zero_limit() {
    let error = chunk_text("hello", 0).expect_err("zero limit must fail");
    assert_eq!(error.to_string(), "chunk size must be greater than zero");
}

#[test]
fn drops_whitespace_only_input() {
    assert!(chunk_text(" \n\t ", 64).expect("valid limit").is_empty());
}
