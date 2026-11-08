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

#[test]
fn parses_does_not_panic_on_multibyte_line_breaks() {
    let src = "``` \u{85}```\u{2028}]";
    let _ = parses(src);
}

#[test]
fn parses_counts_every_line_break_typst_knows() {
    assert_eq!(parses("one\rtwo\r_oops\r").unwrap_err().line, 3);
    assert_eq!(parses("one\u{2028}_oops\n").unwrap_err().line, 2);
}

#[test]
fn parses_survives_a_tree_as_deep_as_the_input() {
    let src = format!("#a{}", ".b".repeat(200_000));
    let outcome = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || parses(&src).is_ok())
        .unwrap()
        .join();
    assert!(outcome.is_ok());
}

#[test]
fn parses_reports_the_first_error_not_a_later_one() {
    let fault = parses("a\n_x\n\n#let y = \n").unwrap_err();
    assert_eq!(fault.line, 2);
    assert!(
        fault.message.contains("unclosed delimiter"),
        "{}",
        fault.message
    );
}

#[test]
fn parses_survives_a_deep_chain_that_carries_errors() {
    let src = format!("$#(1 +){}$", "!".repeat(50_000));
    let outcome = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || parses(&src).is_ok())
        .unwrap()
        .join();
    assert!(outcome.is_ok());
}
