use audiobook_forge::pipeline::pack_phoneme_sentences;

#[test]
fn packs_complete_phoneme_sentences_within_the_limit() {
    let sentences = vec![
        "hɛloʊ".to_owned(),
        "wɝld!".to_owned(),
        "ðɪs ɪz ə tɛst.".to_owned(),
    ];

    let chunks = pack_phoneme_sentences(&sentences, 12).expect("valid phoneme limit");

    assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 12));
    assert_eq!(chunks.join(" "), sentences.join(" "));
}

#[test]
fn splits_one_oversized_phoneme_run_without_losing_tokens() {
    let phonemes = "a".repeat(23);

    let chunks =
        pack_phoneme_sentences(std::slice::from_ref(&phonemes), 10).expect("valid phoneme limit");

    assert_eq!(
        chunks.iter().map(String::len).collect::<Vec<_>>(),
        [10, 10, 3]
    );
    assert_eq!(chunks.concat(), phonemes);
}

#[test]
fn rejects_a_zero_phoneme_limit() {
    let error = pack_phoneme_sentences(&["hɛloʊ".to_owned()], 0).expect_err("zero limit must fail");

    assert_eq!(error.to_string(), "phoneme limit must be greater than zero");
}
