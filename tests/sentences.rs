use audiobook_forge::pipeline::extract_sentences;

#[test]
fn extracts_sentences_and_keeps_closing_quotes() {
    let text = "First sentence. “Why now?” she asked!\n\nFinal paragraph";

    assert_eq!(
        extract_sentences(text),
        [
            "First sentence.",
            "“Why now?”",
            "she asked!",
            "Final paragraph"
        ]
    );
}

#[test]
fn does_not_split_common_abbreviations() {
    assert_eq!(
        extract_sentences("Dr. Smith met Mrs. Jones. They left."),
        ["Dr. Smith met Mrs. Jones.", "They left."]
    );
}
