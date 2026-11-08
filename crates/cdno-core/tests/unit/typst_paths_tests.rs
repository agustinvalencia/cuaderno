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
fn package_imports_and_unrelated_calls_are_ignored() {
    let src = "#import \"@preview/p:1.0.0\"\n#include \"@preview/q:1.0.0\"\n#text(\"x.png\")\n#json.decode(\"{}\")\n#raw(\"code\")\n#bibliography(\"r.bib\", style: \"ieee\")\n";
    let got: Vec<String> = literals(src).into_iter().map(|l| l.value).collect();
    assert_eq!(got, vec!["r.bib"]);
    assert_eq!(path_args(src).len(), 1);
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

fn values(src: &str) -> Vec<(String, String)> {
    literals(src)
        .into_iter()
        .map(|l| (l.call, l.value))
        .collect()
}

fn pair(c: &str, v: &str) -> (String, String) {
    (c.to_string(), v.to_string())
}

#[test]
fn a_string_with_a_scheme_or_an_at_is_a_local_path_outside_import_and_include() {
    let src = "#read(\"x://../../k\")\n#image(\"@2x.png\")\n#image(\"https://a/b.png\")\n";
    assert_eq!(
        values(src),
        vec![
            pair("read", "x://../../k"),
            pair("image", "@2x.png"),
            pair("image", "https://a/b.png")
        ]
    );
}

#[test]
fn the_callee_may_be_std_with_or_parenthesised() {
    let src = "#std.image(\"a.png\")\n#let f = image.with(\"b.png\")\n#(image)(\"c.png\")\n#std.read.with(\"d.txt\")\n#image.with(width: 1pt)(\"e.png\")\n#image.with(width: 1pt)\n#pdf.attach(\"f.csv\")\n#std.pdf.attach(\"g.csv\")\n";
    assert_eq!(
        values(src),
        vec![
            pair("image", "a.png"),
            pair("image", "b.png"),
            pair("image", "c.png"),
            pair("read", "d.txt"),
            pair("image", "e.png"),
            pair("pdf.attach", "f.csv"),
            pair("pdf.attach", "g.csv"),
        ]
    );
    assert_eq!(
        path_args(src).len(),
        7,
        "a .with that adds no path adds nothing"
    );
}

#[test]
fn an_import_from_a_name_or_field_is_reported_not_literal() {
    let src = "#import mod: f\n#import cfg.path: g\n";
    assert_eq!(
        path_args(src),
        vec![
            PathArg::NotLiteral {
                call: "import".into(),
                line: 1
            },
            PathArg::NotLiteral {
                call: "import".into(),
                line: 2
            },
        ]
    );
}

#[test]
fn the_widened_list_is_found() {
    let src = "#plugin(\"p.wasm\")\n#bibliography(\"r.bib\", style: \"my.csl\")\n#cite(<k>, style: \"other.csl\")\n#cite(<k>, style: \"ieee\")\n#set raw(theme: \"t.tmTheme\", syntaxes: (\"a.sublime-syntax\", \"b.sublime-syntax\"))\n#image(\"a.jpg\", icc: \"p.icc\")\n#raw(\"x\", theme: none)\n";
    assert_eq!(
        values(src),
        vec![
            pair("plugin", "p.wasm"),
            pair("bibliography", "r.bib"),
            pair("bibliography", "my.csl"),
            pair("cite", "other.csl"),
            pair("raw", "t.tmTheme"),
            pair("raw", "a.sublime-syntax"),
            pair("raw", "b.sublime-syntax"),
            pair("image", "a.jpg"),
            pair("image", "p.icc"),
        ]
    );
    assert_eq!(path_args(src).len(), 9);
}

#[test]
fn a_computed_named_path_is_reported_not_literal() {
    let src = "#bibliography(\"r.bib\", style: s)\n";
    assert_eq!(path_args(src).len(), 2);
    assert!(matches!(path_args(src)[1], PathArg::NotLiteral { .. }));
}
