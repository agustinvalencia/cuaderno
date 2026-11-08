use cdno_core::typst::{CDNO_LABEL, has_label, parses};

#[test]
fn has_label_is_a_byte_search() {
    assert_eq!(CDNO_LABEL, "<cdno>");
    assert!(has_label("#metadata((type: \"evidence\")) <cdno>\n"));
    assert!(has_label(
        "// the <cdno> label is mentioned here\n= Title\n"
    ));
    assert!(!has_label("= Title\n\nNo label.\n"));
    assert!(!has_label("<cdn>"));
}

#[test]
fn parses_accepts_a_document_with_maths_and_a_show_rule() {
    let src = "#set page(margin: 2cm)\n#show heading: it => [#it.body]\n\n= Error budget\n\nThe bias is $ b = E[hat(f)] - f $ and *strong* text.\n\n#figure(caption: [Split])[#rect()] <fig>\n";
    assert_eq!(parses(src), Ok(()));
}

#[test]
fn parses_reports_the_line_of_an_unclosed_emphasis() {
    let src = "= Title\n\nfine\n\nstill fine\n\n_oops\n\nafter\n";
    let fault = parses(src).unwrap_err();
    assert_eq!(fault.line, 7);
    assert!(!fault.message.is_empty());
}

#[test]
fn parses_reports_an_unclosed_raw_block() {
    let src = "= Title\n\n```rust\nfn main() {}\n";
    let fault = parses(src).unwrap_err();
    assert_eq!(fault.line, 3);
}
