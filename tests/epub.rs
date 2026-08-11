use kokoro_book::book::{Block, Provenance, SectionKind, SourcePosition};
use kokoro_book::input::read_book;
use tempfile::tempdir;

#[path = "support/structured_epub.rs"]
mod structured_epub;
#[path = "support/utf16_epub.rs"]
mod utf16_epub;
use structured_epub::{
    COVER_BYTES, chapter_one_page_7_character_offset, chapter_two_page_8_character_offset,
    chapter_two_page_9_character_offset, legacy_page_11_character_offset,
    write_epub3_with_deep_navigation, write_epub3_with_disguised_deep_navigation,
    write_epub3_with_file_only_toc_target, write_epub3_with_headingless_semantic_container,
    write_epub3_with_interleaved_toc_groups, write_epub3_with_invalid_toc_target,
    write_epub3_with_inverted_parent_child_toc, write_epub3_with_malformed_navigation,
    write_epub3_with_malformed_navigation_and_tail, write_epub3_with_prose_before_targeted_heading,
    write_epub3_with_reversed_same_document_toc, write_epub3_with_reversed_toc,
    write_epub3_with_tokenized_navigation, write_epub3_with_unlisted_headingless_tail,
    write_structured_epub2, write_structured_epub3, write_structured_epub3_with_container_target,
    write_structured_epub3_without_page_list,
};
use utf16_epub::{write_utf16_deep_navigation_epub3, write_utf16_epub3};

#[test]
fn imports_epub3_metadata_navigation_pages_cover_and_spine_content() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("structured.epub");
    write_structured_epub3(&path);

    let book = read_book(&path).expect("structured EPUB");
    assert_eq!(book.metadata.title.as_deref(), Some("Navigation by Design"));
    assert_eq!(book.metadata.authors, ["Ada Reader", "Grace Listener"]);
    assert_eq!(book.metadata.language.as_deref(), Some("en-US"));
    let cover = book.metadata.cover.as_ref().expect("cover");
    assert_eq!(cover.media_type, "image/jpeg");
    assert_eq!(cover.bytes, COVER_BYTES);
    assert_eq!(book.source.format_version.as_deref(), Some("3.0"));

    let part = &book.root.children[0];
    assert_eq!(part.id, "toc-part");
    assert_eq!(part.title.as_deref(), Some("Part One"));
    assert_eq!(part.kind, SectionKind::Part);
    assert_eq!(part.provenance, Provenance::Authored);
    assert!(
        matches!(&part.blocks[0], Block::Paragraph(block) if block.text == "The opening of part one.")
    );

    let chapter_one = &part.children[0];
    assert_eq!(chapter_one.id, "toc-chapter-one");
    assert_eq!(chapter_one.kind, SectionKind::Chapter);
    assert!(
        matches!(&chapter_one.blocks[0], Block::Paragraph(block) if block.text == "First chapter paragraph.")
    );
    assert!(
        matches!(&chapter_one.blocks[1], Block::Paragraph(block) if block.text == "Visible CDATA.")
    );
    assert_epub_fragment(chapter_one, "/EPUB/chapter-1.xhtml", "chapter-one");

    let subsection = &chapter_one.children[0];
    assert_eq!(subsection.id, "toc-why");
    assert_eq!(subsection.title.as_deref(), Some("Why It Matters"));
    assert_epub_fragment(subsection, "/EPUB/chapter-1.xhtml", "café");
    assert!(
        matches!(&subsection.blocks[0], Block::Paragraph(block) if block.text == "Navigation preserves meaning.")
    );
    assert!(
        matches!(&subsection.blocks[1], Block::Footnote(block) if block.text == "A concise note.")
    );
    assert_block_epub_fragment(&subsection.blocks[1], "/EPUB/chapter-1.xhtml", "note-1");
    assert!(
        matches!(&subsection.blocks[2], Block::Footnote(block) if block.text == "A closing endnote.")
    );
    assert_block_epub_fragment(&subsection.blocks[2], "/EPUB/chapter-1.xhtml", "note-2");
    assert!(
        matches!(&subsection.blocks[3], Block::Footnote(block) if block.text == "A grouped endnote.")
    );
    assert_block_epub_fragment(&subsection.blocks[3], "/EPUB/chapter-1.xhtml", "note-3");

    assert_eq!(part.children[1].title.as_deref(), Some("Chapter Two"));
    assert!(book.text.find("part one").unwrap() < book.text.find("First chapter").unwrap());
    assert!(book.text.find("First chapter").unwrap() < book.text.find("Second chapter").unwrap());

    assert_eq!(book.pages.len(), 2);
    assert_eq!(book.pages[0].label, "7");
    assert_eq!(
        book.pages[0].position,
        SourcePosition::Epub {
            resource: "/EPUB/chapter-1.xhtml".to_owned(),
            fragment: Some("page-7".to_owned()),
            character_offset: Some(chapter_one_page_7_character_offset()),
        }
    );
}

#[test]
fn imports_epub3_pagebreaks_when_navigation_has_no_page_list() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("pagebreaks.epub");
    write_structured_epub3_without_page_list(&path);

    let book = read_book(&path).expect("EPUB pagebreak fallback");

    assert_eq!(book.pages.len(), 3);
    assert_eq!(book.pages[0].label, "7");
    assert!(matches!(
        &book.pages[0].position,
        SourcePosition::Epub {
            resource,
            fragment: Some(fragment),
            character_offset: Some(_),
        } if resource == "/EPUB/chapter-1.xhtml" && fragment == "page-7"
    ));
    assert_eq!(book.pages[1].label, "8");
    assert!(matches!(
        &book.pages[1].position,
        SourcePosition::Epub {
            character_offset: Some(offset),
            ..
        } if *offset == chapter_two_page_8_character_offset()
    ));
    assert_eq!(book.pages[2].label, "9");
    assert_eq!(
        book.pages[2].position,
        SourcePosition::Epub {
            resource: "/EPUB/chapter-2.xhtml".to_owned(),
            fragment: None,
            character_offset: Some(chapter_two_page_9_character_offset()),
        }
    );
}

#[test]
fn skips_epub_navigation_entries_that_reference_missing_resources() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("invalid-navigation.epub");
    write_epub3_with_invalid_toc_target(&path);

    let book = read_book(&path).expect("recoverable invalid EPUB navigation");

    assert!(
        book.warnings.iter().any(
            |warning| warning.contains("missing resource") && warning.contains("Ghost Chapter")
        )
    );
    assert!(
        !book
            .root
            .children
            .iter()
            .any(|section| section.title.as_deref() == Some("Ghost Chapter"))
    );
    assert!(book.warnings.iter().any(
        |warning| warning.contains("missing fragment") && warning.contains("Missing Fragment")
    ));
    assert!(!book.pages.iter().any(|page| page.label == "99"));
    assert_eq!(book.pages.len(), 3);
    assert!(book.text.contains("First chapter paragraph."));
}

#[test]
fn canonical_order_follows_the_spine_when_toc_order_is_reversed() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("reversed-toc.epub");
    write_epub3_with_reversed_toc(&path);

    let book = read_book(&path).expect("EPUB with reversed TOC");
    let part = find_section(&book.root, "toc-part").expect("part");

    assert_eq!(part.children[0].title.as_deref(), Some("Chapter One"));
    assert_eq!(part.children[1].title.as_deref(), Some("Chapter Two"));
    assert!(book.text.find("First chapter").unwrap() < book.text.find("Second chapter").unwrap());
}

#[test]
fn file_only_toc_targets_merge_the_first_spine_heading_without_duplication() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("file-only-toc.epub");
    write_epub3_with_file_only_toc_target(&path);

    let book = read_book(&path).expect("EPUB with file-only TOC targets");
    let chapter = find_section(&book.root, "toc-chapter-one").expect("chapter one");

    assert_eq!(chapter.title.as_deref(), Some("Chapter One"));
    assert_eq!(
        book.text.matches("Chapter One").count(),
        1,
        "authored and derived headings must merge"
    );
    assert!(
        !chapter
            .children
            .iter()
            .any(|child| child.title == chapter.title)
    );
}

#[test]
fn malformed_navigation_warns_and_falls_back_to_spine_headings() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("malformed-nav.epub");
    write_epub3_with_malformed_navigation(&path);

    let book = read_book(&path).expect("malformed navigation fallback");

    assert!(
        book.warnings
            .iter()
            .any(|warning| warning.contains("navigation could not be parsed"))
    );
    assert!(book.text.contains("First chapter paragraph."));
    assert!(find_section_by_title(&book.root, "Chapter One").is_some());
}

#[test]
fn malformed_navigation_fallback_keeps_a_headingless_tail_at_the_end() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("malformed-nav-tail.epub");
    write_epub3_with_malformed_navigation_and_tail(&path);

    let book = read_book(&path).expect("malformed navigation fallback with tail");

    assert!(book.text.find("Chapter body").unwrap() < book.text.find("Tail body").unwrap());
    assert!(book.root.blocks.is_empty());
}

#[test]
fn excessive_navigation_depth_fails_preflight_without_recursion_failure() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("deep-nav.epub");
    write_epub3_with_deep_navigation(&path);

    let error = read_book(&path).expect_err("deep navigation must be rejected");

    assert!(error.to_string().contains("nesting exceeds 128 levels"));
}

#[test]
fn disguised_navigation_depth_fails_preflight_without_recursion_failure() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("disguised-deep-nav.epub");
    write_epub3_with_disguised_deep_navigation(&path);

    let error = read_book(&path).expect_err("deep disguised navigation must be rejected");

    assert!(error.to_string().contains("nesting exceeds 128 levels"));
    assert!(error.to_string().contains("EPUB/nav"));
}

#[test]
fn utf16_navigation_depth_fails_preflight_before_rbook_recurses() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("utf16-deep-nav.epub");
    write_utf16_deep_navigation_epub3(&path, 12_000);

    let error = read_book(&path).expect_err("deep UTF-16 navigation must be rejected");

    assert!(error.to_string().contains("nesting exceeds 128 levels"));
}

#[test]
fn imports_utf16_epub3_package_navigation_and_spine_xhtml() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("utf16.epub");
    write_utf16_epub3(&path);

    let book = read_book(&path).expect("UTF-16 EPUB 3");

    assert_eq!(book.metadata.title.as_deref(), Some("UTF-16 Book"));
    assert_eq!(book.root.children[0].title.as_deref(), Some("Chapter One"));
    assert!(book.text.contains("Unicode body."));
}

#[test]
fn canonical_order_uses_heading_positions_within_one_spine_resource() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("same-document-order.epub");
    write_epub3_with_reversed_same_document_toc(&path);

    let book = read_book(&path).expect("EPUB with reversed same-document TOC");

    assert_eq!(book.root.children[0].title.as_deref(), Some("Alpha"));
    assert_eq!(book.root.children[1].title.as_deref(), Some("Beta"));
    assert!(book.text.find("Alpha body").unwrap() < book.text.find("Beta body").unwrap());
}

#[test]
fn canonical_order_promotes_a_child_that_precedes_its_toc_parent() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("inverted-parent-child.epub");
    write_epub3_with_inverted_parent_child_toc(&path);

    let book = read_book(&path).expect("EPUB with inverted parent and child targets");

    assert_eq!(book.root.children[0].title.as_deref(), Some("Chapter"));
    assert_eq!(book.root.children[1].title.as_deref(), Some("Part"));
    assert!(book.text.find("Chapter body").unwrap() < book.text.find("Part body").unwrap());
}

#[test]
fn canonical_order_promotes_interleaved_toc_descendants() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("interleaved-groups.epub");
    write_epub3_with_interleaved_toc_groups(&path);

    let book = read_book(&path).expect("EPUB with interleaved TOC groups");

    let first = book.text.find("First body").unwrap();
    let second = book.text.find("Second body").unwrap();
    let third = book.text.find("Third body").unwrap();
    assert!(first < second && second < third);
}

#[test]
fn unlisted_headingless_tail_stays_at_the_end_of_the_spine() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("unlisted-tail.epub");
    write_epub3_with_unlisted_headingless_tail(&path);

    let book = read_book(&path).expect("EPUB with unlisted headingless tail");

    assert!(book.text.find("Chapter body").unwrap() < book.text.find("Tail body").unwrap());
    assert!(book.root.blocks.is_empty());
}

#[test]
fn prose_before_a_fragment_target_stays_before_the_section_title() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("lead-before-heading.epub");
    write_epub3_with_prose_before_targeted_heading(&path);

    let book = read_book(&path).expect("EPUB with prose before targeted heading");

    assert!(book.text.find("Lead prose").unwrap() < book.text.find("Chapter").unwrap());
    assert!(book.text.find("Chapter").unwrap() < book.text.find("Chapter body").unwrap());
}

#[test]
fn toc_container_targets_merge_their_first_descendant_heading() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("container-target.epub");
    write_structured_epub3_with_container_target(&path);

    let book = read_book(&path).expect("EPUB with a containing section target");

    assert_eq!(book.text.matches("The Beginning").count(), 1);
    assert!(book.text.contains("Chapter body."));
    assert_eq!(book.root.children[0].kind, SectionKind::Chapter);
}

#[test]
fn preserves_a_headingless_authored_semantic_container() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("headingless-semantic-container.epub");
    write_epub3_with_headingless_semantic_container(&path);

    let book = read_book(&path).expect("headingless semantic container");
    let chapter = &book.root.children[0];

    assert_eq!(chapter.kind, SectionKind::Chapter);
    assert!(matches!(
        chapter.blocks.as_slice(),
        [Block::Paragraph(block)] if block.text == "Headingless chapter body."
    ));
}

#[test]
fn reads_tokenized_navigation_and_image_alternative_labels() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("tokenized-navigation.epub");
    write_epub3_with_tokenized_navigation(&path);

    let book = read_book(&path).expect("tokenized navigation");

    assert_eq!(book.root.children[0].title.as_deref(), Some("1"));
    assert_eq!(book.root.children[0].kind, SectionKind::Chapter);
    assert_eq!(book.pages.len(), 1);
    assert_eq!(book.pages[0].label, "42");
}

#[test]
fn epub3_structure_matches_the_checked_in_golden_tree() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("structured.epub");
    write_structured_epub3(&path);

    let book = read_book(&path).expect("structured EPUB");
    let actual = render_structure(&book.root);

    assert_eq!(
        actual,
        include_str!("fixtures/structured-epub3.structure.txt")
    );
}

#[test]
fn imports_epub2_ncx_page_list_metadata_cover_and_spine_content() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("structured-epub2.epub");
    write_structured_epub2(&path);

    let book = read_book(&path).expect("structured EPUB 2");

    assert_eq!(book.source.format_version.as_deref(), Some("2.0"));
    assert_eq!(book.metadata.title.as_deref(), Some("Legacy Navigation"));
    assert_eq!(book.metadata.authors, ["Nora Narrator"]);
    assert_eq!(book.metadata.language.as_deref(), Some("en"));
    assert_eq!(
        book.metadata
            .cover
            .as_ref()
            .map(|cover| cover.bytes.as_slice()),
        Some(COVER_BYTES)
    );

    let part = &book.root.children[0];
    assert_eq!(part.id, "ncx-part");
    assert_eq!(part.kind, SectionKind::Part);
    let chapter = &part.children[0];
    assert_eq!(chapter.id, "ncx-chapter");
    assert_eq!(chapter.kind, SectionKind::Chapter);
    assert!(
        matches!(&chapter.blocks[0], Block::Paragraph(block) if block.text == "An EPUB 2 paragraph.")
    );
    assert_epub_fragment(chapter, "/OPS/chapter.xhtml", "legacy-chapter");

    assert_eq!(book.pages.len(), 1);
    assert_eq!(book.pages[0].label, "11");
    assert_eq!(
        book.pages[0].position,
        SourcePosition::Epub {
            resource: "/OPS/chapter.xhtml".to_owned(),
            fragment: Some("page-11".to_owned()),
            character_offset: Some(legacy_page_11_character_offset()),
        }
    );
}

fn assert_epub_fragment(
    section: &kokoro_book::book::Section,
    expected_resource: &str,
    expected_fragment: &str,
) {
    let range = section.source_range.as_ref().expect("EPUB source range");
    assert_eq!(range.source_id, expected_resource);
    assert!(matches!(
        &range.start,
        SourcePosition::Epub {
            resource,
            fragment: Some(fragment),
            ..
        } if resource == expected_resource && fragment == expected_fragment
    ));
}

fn assert_block_epub_fragment(block: &Block, expected_resource: &str, expected_fragment: &str) {
    let range = match block {
        Block::Paragraph(block)
        | Block::Quote(block)
        | Block::Aside(block)
        | Block::Navigation(block)
        | Block::Code(block)
        | Block::Footnote(block) => block.source_range.as_ref(),
        Block::List(block) => block.source_range.as_ref(),
        Block::Figure(block) => block.source_range.as_ref(),
    }
    .expect("EPUB block source range");
    assert_eq!(range.source_id, expected_resource);
    assert!(matches!(
        &range.start,
        SourcePosition::Epub {
            resource,
            fragment: Some(fragment),
            ..
        } if resource == expected_resource && fragment == expected_fragment
    ));
}

fn render_structure(root: &kokoro_book::book::Section) -> String {
    fn render(section: &kokoro_book::book::Section, indent: usize, output: &mut String) {
        use std::fmt::Write as _;

        let source = section.source_range.as_ref().map_or_else(
            || "-".to_owned(),
            |range| match &range.start {
                SourcePosition::Epub {
                    resource, fragment, ..
                } => format!("{}#{}", resource, fragment.as_deref().unwrap_or("")),
                position => format!("{position:?}"),
            },
        );
        writeln!(
            output,
            "{}{}|{:?}|{}|{}|blocks:{}",
            "  ".repeat(indent),
            section.id,
            section.kind,
            section.title.as_deref().unwrap_or(""),
            source,
            section.blocks.len()
        )
        .expect("render structure");
        for child in &section.children {
            render(child, indent + 1, output);
        }
    }

    let mut output = String::new();
    render(root, 0, &mut output);
    output
}

fn find_section<'a>(
    section: &'a kokoro_book::book::Section,
    id: &str,
) -> Option<&'a kokoro_book::book::Section> {
    if section.id == id {
        return Some(section);
    }
    section
        .children
        .iter()
        .find_map(|child| find_section(child, id))
}

fn find_section_by_title<'a>(
    section: &'a kokoro_book::book::Section,
    title: &str,
) -> Option<&'a kokoro_book::book::Section> {
    if section.title.as_deref() == Some(title) {
        return Some(section);
    }
    section
        .children
        .iter()
        .find_map(|child| find_section_by_title(child, title))
}
