//! Unit tests for the `--var name=value` parser (#238).

use cdno_cli::prompt::parse_key_val;

#[test]
fn parses_a_simple_pair() {
    assert_eq!(
        parse_key_val("ticket=ABC-1").unwrap(),
        ("ticket".to_owned(), "ABC-1".to_owned())
    );
}

#[test]
fn splits_on_the_first_equals_only() {
    // Values may contain `=`; only the first separates name from value.
    assert_eq!(
        parse_key_val("expr=a=b").unwrap(),
        ("expr".to_owned(), "a=b".to_owned())
    );
}

#[test]
fn allows_an_empty_value() {
    assert_eq!(
        parse_key_val("note=").unwrap(),
        ("note".to_owned(), String::new())
    );
}

#[test]
fn rejects_a_missing_equals() {
    assert!(parse_key_val("ticket").is_err());
}

#[test]
fn rejects_an_empty_name() {
    assert!(parse_key_val("=value").is_err());
}
