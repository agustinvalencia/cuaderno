use cdno_core::typst::{TypstLinks, headings, wikilink_spans};
use proptest::prelude::*;

fn one_span(source: &str) -> TypstLinks {
    let links = wikilink_spans(source);
    assert_eq!(links.spans.len(), 1, "{source}: {links:?}");
    assert_eq!(links.addresses.len(), 1);
    assert_eq!(links.targets.len(), 1);
    assert!(links.faults.is_empty(), "{source}: {links:?}");
    links
}

fn nothing(source: &str) {
    let links = wikilink_spans(source);
    assert_eq!(links, TypstLinks::default(), "{source}");
}

fn one_fault(source: &str) -> cdno_core::typst::LinkFault {
    let links = wikilink_spans(source);
    assert!(links.spans.is_empty(), "{source}: {links:?}");
    assert_eq!(links.faults.len(), 1, "{source}: {links:?}");
    links.faults.into_iter().next().unwrap()
}

#[test]
fn headings_use_source_text() {
    assert_eq!(
        headings("= The *bias* term <sec>\n"),
        vec![(1, "The *bias* term".to_string())]
    );
    assert_eq!(headings("== A -- B\n"), vec![(2, "A -- B".to_string())]);
    assert_eq!(headings("= Caf\\u{e9}\n"), vec![(1, "Café".to_string())]);
}

#[test]
fn headings_decode_escapes_collapse_space_and_keep_order() {
    let source =
        "= One \\#   two\n\ntext\n\n=== Three // aside\n\n#block[== Nested]\n\n`= not one`\n";
    assert_eq!(
        headings(source),
        vec![
            (1, "One # two".to_string()),
            (3, "Three".to_string()),
            (2, "Nested".to_string()),
        ]
    );
}

#[test]
fn links_in_a_plain_run() {
    let source = "See [[concepts/bias-variance-split]] here.\n";
    let links = one_span(source);
    assert_eq!(links.addresses[0], "concepts/bias-variance-split");
    assert_eq!(links.targets[0], "concepts/bias-variance-split");
    let span = &links.spans[0];
    assert_eq!(&source[span.address.clone()], links.addresses[0]);
    assert_eq!(span.open, 4);
    assert_eq!(&source[span.inner_end..span.inner_end + 2], "]]");
    assert!(!span.is_embed);
}

#[test]
fn links_split_by_the_lexer_are_whole() {
    for (source, address) in [
        (
            "[[2026-11-08-error-budget--v2]]",
            "2026-11-08-error-budget--v2",
        ),
        ("[[it's-done]]", "it's-done"),
        ("[[a/b.c|label]]", "a/b.c"),
    ] {
        let links = one_span(source);
        assert_eq!(links.addresses[0], address);
        assert_eq!(links.targets[0], address);
        assert_eq!(&source[links.spans[0].address.clone()], address);
    }
}

#[test]
fn an_escaped_hash_is_decoded_in_the_target_and_kept_out_of_the_address() {
    let source = "[[concepts/x\\#Statement]]";
    let links = one_span(source);
    assert_eq!(links.targets[0], "concepts/x#Statement");
    assert_eq!(links.addresses[0], "concepts/x");
    assert_eq!(
        &source[links.spans[0].target.clone()],
        "concepts/x\\#Statement"
    );
    assert_eq!(&source[links.spans[0].address.clone()], "concepts/x");
}

#[test]
fn an_escape_inside_the_address_stays_in_the_raw_span() {
    let source = "[[a\\_b\\#Top|see \\#]]";
    let links = one_span(source);
    assert_eq!(links.addresses[0], "a_b");
    assert_eq!(links.targets[0], "a_b#Top");
    assert_eq!(&source[links.spans[0].address.clone()], "a\\_b");
}

#[test]
fn an_escaped_bar_separates_the_label() {
    let links = one_span("[[surrogate-model\\|the model]]");
    assert_eq!(links.addresses[0], "surrogate-model");
}

#[test]
fn surrounding_space_is_outside_the_ranges() {
    let source = "[[  surrogate-model  \\#Top  |label]]";
    let links = one_span(source);
    assert_eq!(&source[links.spans[0].address.clone()], "surrogate-model");
    assert_eq!(
        &source[links.spans[0].target.clone()],
        "surrogate-model  \\#Top"
    );
    assert_eq!(links.targets[0], "surrogate-model  #Top");
}

#[test]
fn a_same_note_anchor_has_an_empty_address() {
    let source = "[[\\#Top]]";
    let links = one_span(source);
    assert_eq!(links.addresses[0], "");
    assert_eq!(links.targets[0], "#Top");
    assert_eq!(links.spans[0].address, 2..2);
}

#[test]
fn an_embed_is_marked() {
    assert!(one_span("![[surrogate-model]]").spans[0].is_embed);
}

#[test]
fn an_empty_link_is_neither_a_span_nor_a_fault() {
    nothing("[[]]");
    nothing("[[  |label]]");
}

#[test]
fn an_unescaped_hash_is_a_fault_not_a_link() {
    let fault = one_fault("First line.\n\n[[concepts/x#Statement]]\n");
    assert_eq!(fault.leaf, "Hash");
    assert_eq!(fault.line, 3);
    assert_eq!(fault.open, 13);
}

#[test]
fn a_line_comment_swallowing_the_close_is_named() {
    assert_eq!(one_fault("[[a//b]]\n").leaf, "LineComment");
}

#[test]
fn links_inside_a_figure_caption_are_read() {
    let links = one_span("#figure(rect(), caption: [see [[surrogate-model]]])\n");
    assert_eq!(links.addresses[0], "surrogate-model");
}

#[test]
fn links_in_markup_containers_are_read() {
    for source in [
        "*[[x]]*",
        "_[[x]]_",
        "= Heading [[x]]",
        "- item [[x]]",
        "+ item [[x]]",
        "#[[[x]]]",
    ] {
        assert_eq!(one_span(source).addresses[0], "x", "{source}");
    }
}

#[test]
fn links_in_raw_comments_code_strings_and_maths_are_not_read() {
    for source in [
        "`[[x]]`",
        "```\n[[x]]\n```",
        "// [[x]]\n",
        "/* [[x]] */",
        "#let a = \"[[x]]\"\n",
        "$ [[x]] $",
    ] {
        nothing(source);
    }
}

#[test]
fn links_in_the_block_strings_are_read() {
    let source = "#metadata((origin: \"[[projects/surrogate-model]]\", tags: (\"[[a]]\", \"b\"))) <cdno>\n\n#let other = \"[[x]]\"\n";
    let links = wikilink_spans(source);
    assert_eq!(links.addresses, ["projects/surrogate-model", "a"]);
    assert!(links.faults.is_empty());
    assert_eq!(
        &source[links.spans[0].address.clone()],
        "projects/surrogate-model"
    );
    let open = links.spans[0].open;
    assert_eq!(&source[open - 1..open + 2], "\"[[");
}

#[test]
fn block_keys_other_metadata_and_other_calls_are_not_read() {
    nothing("#metadata((\"[[k]]\": \"v\")) <cdno>\n");
    nothing("#metadata((origin: \"[[x]]\")) <other>\n");
    nothing("#metadata((origin: \"[[x]]\"))\n");
    nothing("#note((origin: \"[[x]]\")) <cdno>\n");
    nothing("#block[#metadata((origin: \"[[x]]\")) <cdno>]\n");
}

#[test]
fn the_block_label_may_sit_on_the_next_line() {
    assert_eq!(
        one_span("#metadata((origin: \"[[x]]\"))\n// note\n<cdno>\n").addresses[0],
        "x"
    );
}

#[test]
fn a_block_string_decodes_and_maps_back_to_raw_bytes() {
    let source = "#metadata((origin: \"say \\\"[[a\\u{2d}b#Top]]\\\"\")) <cdno>\n";
    let links = one_span(source);
    assert_eq!(links.addresses[0], "a-b");
    assert_eq!(links.targets[0], "a-b#Top");
    assert_eq!(&source[links.spans[0].address.clone()], "a\\u{2d}b");
    assert_eq!(&source[links.spans[0].target.clone()], "a\\u{2d}b#Top");
    assert_eq!(&source[links.spans[0].open..][..2], "[[");
    assert_eq!(&source[links.spans[0].inner_end..][..2], "]]");
}

#[test]
fn a_block_string_keeps_a_backslash_hash_literally() {
    let links = one_span("#metadata((origin: \"[[x\\#y]]\")) <cdno>\n");
    assert_eq!(links.addresses[0], "x\\");
}

#[test]
fn a_bracket_run_the_lexer_broke_is_still_found_by_lint() {
    assert_eq!(one_fault("[[a *b* c]]").leaf, "Strong");
}

#[test]
fn a_content_block_bracket_is_not_a_link_opening() {
    nothing("#foo[[x]]");
}

#[test]
fn a_link_glued_after_a_hash_expression_is_not_one() {
    nothing("#emph[a][[x]]");
}

#[test]
fn a_space_before_the_opening_makes_it_a_link_again() {
    assert_eq!(one_span("#emph[a] [[x]]").addresses[0], "x");
}

#[test]
fn a_link_spanning_a_line_is_a_fault() {
    let fault = one_fault("[[a\nb]]");
    assert_eq!(fault.leaf, "Space");
    assert_eq!(fault.line, 1);
}

#[test]
fn a_paragraph_break_inside_a_link_is_a_fault() {
    assert_eq!(one_fault("[[a\n\nb]]").leaf, "Parbreak");
}

#[test]
fn a_decoded_line_break_in_a_target_is_a_fault() {
    assert_eq!(one_fault("[[a\\u{a}b]]").leaf, "Escape");
}

#[test]
fn a_stray_opening_on_an_earlier_line_does_not_hide_a_link() {
    let links = wikilink_spans("[[a\nsee [[surrogate-model]]\n");
    assert_eq!(links.addresses, ["surrogate-model"]);
    assert_eq!(links.faults.len(), 1);
    assert_eq!(links.faults[0].line, 1);
}

#[test]
fn an_unclosed_opening_is_a_fault() {
    one_fault("[[x");
}

#[test]
fn an_escaped_bracket_is_not_an_opening_or_a_close() {
    nothing("\\[\\[x\\]\\]");
    assert_eq!(one_fault("[[x\\]\\]").leaf, "end of run");
}

#[test]
fn links_after_a_fault_in_the_same_paragraph_are_read() {
    let links = wikilink_spans("[[a *b* c]] and [[surrogate-model]]");
    assert_eq!(links.addresses, ["surrogate-model"]);
    assert_eq!(links.faults.len(), 1);
}

#[test]
fn offsets_after_multibyte_text_are_byte_offsets() {
    let source = "Bias–variance é [[café\\#Résumé]]";
    let links = one_span(source);
    assert_eq!(links.addresses[0], "café");
    assert_eq!(&source[links.spans[0].address.clone()], "café");
    assert_eq!(links.targets[0], "café#Résumé");
}

#[test]
fn a_pathological_run_of_openings_returns() {
    let source = "[[ ".repeat(60_000);
    assert_eq!(wikilink_spans(&source).faults.len(), 60_000);
    let lines = "[[\n".repeat(60_000);
    assert_eq!(wikilink_spans(&lines).faults.len(), 60_000);
}

proptest! {
    #[test]
    fn a_slug_path_is_one_link(
        path in "[a-z0-9]{1,8}(-[a-z0-9]{1,8}){0,4}(/[a-z0-9]{1,8}(-[a-z0-9]{1,8}){0,4}){0,2}",
    ) {
        let source = format!("Some text [[{path}]] after.\n");
        let links = wikilink_spans(&source);
        prop_assert_eq!(links.spans.len(), 1);
        prop_assert!(links.faults.is_empty());
        prop_assert_eq!(&links.addresses[0], &path);
        prop_assert_eq!(&source[links.spans[0].address.clone()], path.as_str());
    }
}
