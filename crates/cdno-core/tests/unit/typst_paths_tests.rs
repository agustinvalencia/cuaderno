use cdno_core::typst::{PathArg, PathLiteral, path_args};

fn literals(src: &str) -> Vec<PathLiteral> {
    path_args(src)
        .into_iter()
        .filter_map(|a| match a {
            PathArg::Literal(l) => Some(l),
            PathArg::NotLiteral { .. } => None,
        })
        .collect()
}

const ONE_OF_EACH: &str = r#"= Error budget

#image("figs/split.png", width: 80%)
#let d = read("data/raw.txt")
#let a = json("data/a.json")
#let b = csv("data/b.csv")
#let c = yaml("data/c.yaml")
#let e = toml("data/e.toml")
#let f = xml("data/f.xml")
#let g = cbor("data/g.cbor")
#bibliography("refs.bib")
#include "chapter.typ"
#import "lib.typ": helper
#import "@preview/x:0.1.0": y
"#;

#[test]
fn path_args_finds_every_listed_call() {
    let found = literals(ONE_OF_EACH);
    let got: Vec<(&str, &str)> = found
        .iter()
        .map(|l| (l.call.as_str(), l.value.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("image", "figs/split.png"),
            ("read", "data/raw.txt"),
            ("json", "data/a.json"),
            ("csv", "data/b.csv"),
            ("yaml", "data/c.yaml"),
            ("toml", "data/e.toml"),
            ("xml", "data/f.xml"),
            ("cbor", "data/g.cbor"),
            ("bibliography", "refs.bib"),
            ("include", "chapter.typ"),
            ("import", "lib.typ"),
        ]
    );
    for l in &found {
        assert_eq!(&ONE_OF_EACH[l.span.clone()], l.value, "{}", l.call);
    }
    assert_eq!(found[0].line, 3);
    assert_eq!(found[10].line, 13);
    assert_eq!(
        path_args(ONE_OF_EACH).len(),
        11,
        "the @preview import is absent"
    );
}

#[test]
fn bibliography_with_an_array_yields_one_literal_per_item() {
    let src = "#bibliography((\"a.bib\", \"b.yml\"))\n#bibliography((\"only.bib\"))\n";
    let got: Vec<String> = literals(src).into_iter().map(|l| l.value).collect();
    assert_eq!(got, vec!["a.bib", "b.yml", "only.bib"]);
    for l in literals(src) {
        assert_eq!(&src[l.span.clone()], l.value);
    }
}

#[test]
fn a_variable_path_is_reported_not_literal() {
    let src = "#let p = \"x.png\"\n#image(p)\n#read(\"a\" + \"b\")\n#include p\n";
    assert_eq!(
        path_args(src),
        vec![
            PathArg::NotLiteral {
                call: "image".into(),
                line: 2
            },
            PathArg::NotLiteral {
                call: "read".into(),
                line: 3
            },
            PathArg::NotLiteral {
                call: "include".into(),
                line: 4
            },
        ]
    );
}

#[test]
fn an_image_inside_a_figure_is_found() {
    let src = "#figure(image(\"figs/a.png\"), caption: [Split])\n\n#figure[\n  #image(\"figs/b.png\")\n]\n";
    let got: Vec<(String, usize)> = literals(src)
        .into_iter()
        .map(|l| (l.value, l.line))
        .collect();
    assert_eq!(
        got,
        vec![("figs/a.png".to_string(), 1), ("figs/b.png".to_string(), 4)]
    );
}

#[test]
fn urls_remote_imports_and_unrelated_calls_are_ignored() {
    let src = "#image(\"https://example.org/a.png\")\n#import \"@preview/p:1.0.0\"\n#import mod: f\n#text(\"x.png\")\n#json.decode(\"{}\")\n";
    assert_eq!(path_args(src), vec![]);
}

#[test]
fn the_span_covers_an_escaped_value_as_written() {
    let src = "#image(\"a\\\"b.png\")\n";
    let l = &literals(src)[0];
    assert_eq!(l.value, "a\"b.png");
    assert_eq!(&src[l.span.clone()], "a\\\"b.png");
}

#[test]
fn a_listed_call_without_a_positional_argument_is_reported_not_literal() {
    let src = "#image(path: \"a.png\")\n#read(..args)\n";
    assert_eq!(
        path_args(src),
        vec![
            PathArg::NotLiteral {
                call: "image".into(),
                line: 1
            },
            PathArg::NotLiteral {
                call: "read".into(),
                line: 2
            },
        ]
    );
}
