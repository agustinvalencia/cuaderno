use cdno_core::hash::{MASK_VERSION, content_hash, mask_references, masked_content_hash};

#[test]
fn hash_is_deterministic_for_same_input() {
    let a = content_hash("hello world");
    let b = content_hash("hello world");
    assert_eq!(a, b);
}

#[test]
fn hash_differs_for_different_inputs() {
    let a = content_hash("hello");
    let b = content_hash("world");
    assert_ne!(a, b);
}

#[test]
fn hash_is_sensitive_to_trailing_whitespace() {
    // mtime alone can't detect a trailing-whitespace-only change, so
    // the hash has to — otherwise reconciliation would silently skip
    // the edit.
    let a = content_hash("text");
    let b = content_hash("text ");
    assert_ne!(a, b);
}

#[test]
fn hash_is_sensitive_to_newline_difference() {
    // CRLF vs LF is a real file change and the hash must surface it.
    let lf = content_hash("line one\nline two");
    let crlf = content_hash("line one\r\nline two");
    assert_ne!(lf, crlf);
}

#[test]
fn hash_is_16_lowercase_hex_chars() {
    let h = content_hash("anything");
    assert_eq!(h.len(), 16);
    assert!(
        h.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    );
}

#[test]
fn hash_of_empty_string_is_stable() {
    // The empty string is a valid input — an empty note should still
    // get a well-defined fingerprint.
    let h = content_hash("");
    assert_eq!(h.len(), 16);
    assert_eq!(h, content_hash(""));
}

#[test]
fn hash_handles_utf8_multibyte() {
    let a = content_hash("café");
    let b = content_hash("cafe");
    assert_ne!(a, b);
}

const KEYS: &[&str] = &["project"];

fn prefix(slug: &str, prose: &str, label: &str) -> String {
    format!(
        "---\ntype: action\nproject: {slug}\n---\n\n{prose} see [[{slug}#Current State|{label}]] and ![[{slug}]]\n"
    )
}

#[test]
fn masked_hash_ignores_addresses_and_listed_fields() {
    let a = prefix("surrogate-model", "Body", "the model");
    let b = prefix("surrogate-v2", "Body", "the model");
    assert_eq!(masked_content_hash(&a, KEYS), masked_content_hash(&b, KEYS));
    assert_ne!(content_hash(&a), content_hash(&b));

    // A quoted wikilink value is masked once, with its label kept.
    let q1 = "---\nproject: \"[[old|x]]\"\n---\n";
    let q2 = "---\nproject: \"[[new|x]]\"\n---\n";
    assert_eq!(masked_content_hash(q1, KEYS), masked_content_hash(q2, KEYS));

    // A label inside a frontmatter wikilink scalar is still hashed.
    assert_ne!(
        masked_content_hash("---\nproject: \"[[old|x]]\"\n---\n", KEYS),
        masked_content_hash("---\nproject: \"[[old|y]]\"\n---\n", KEYS)
    );

    // One byte of prose, a label, an anchor or an unlisted key still count.
    let base = masked_content_hash(&a, KEYS);
    assert_ne!(
        base,
        masked_content_hash(&prefix("surrogate-model", "Bode", "the model"), KEYS)
    );
    assert_ne!(
        base,
        masked_content_hash(&prefix("surrogate-model", "Body", "the mode1"), KEYS)
    );
    assert_ne!(
        base,
        masked_content_hash(&a.replace("#Current State", "#Other"), KEYS)
    );
    assert_ne!(
        base,
        masked_content_hash(&a, &[]),
        "an unlisted key is not masked"
    );
    assert_ne!(
        masked_content_hash("---\nproject: old\n---\n", KEYS),
        masked_content_hash("---\nproject: old\nx: 1\n---\n", KEYS)
    );
}

#[test]
fn masked_hash_without_frontmatter_masks_only_addresses() {
    assert_eq!(
        masked_content_hash("see [[old]]\n", KEYS),
        masked_content_hash("see [[new]]\n", KEYS)
    );
    assert_ne!(
        masked_content_hash("see [[old]]\n", KEYS),
        masked_content_hash("see [[old]]!\n", KEYS)
    );
}

#[test]
fn masking_is_prefix_local() {
    let prefix = prefix("surrogate-model", "Body", "the model");
    let appendices = [
        "\n## Follow-up\nsee [[other#Heading|label]] and ![[embed]]\n",
        "\nproject: not-a-field, written as prose\n[[a]][[b]]\n",
        "\n```\n[[in-a-code-block]]\n```\n`[[code span]]`\n",
    ];
    let masked = mask_references(&prefix, KEYS);
    for appendix in appendices {
        let whole = mask_references(&format!("{prefix}{appendix}"), KEYS);
        assert!(whole.starts_with(&masked), "{appendix:?}");
    }
}

#[test]
fn masked_text_length_survives_a_different_length_rename() {
    let a = mask_references(&prefix("old", "Body", "x"), KEYS);
    let b = mask_references(&prefix("surrogate-model", "Body", "x"), KEYS);
    assert_eq!(a, b);
}

#[test]
fn masked_hash_is_stable_under_mask_version_1() {
    // A change to this literal means the mask rule changed (`wikilink_spans`,
    // the scalar matcher, the token): bump MASK_VERSION and teach the
    // verifier the old rule. Do not just update the string.
    let input = "---\ntype: action\nproject: surrogate-model\nportfolio: \"calibration\"\nrelated: \"[[surrogate-model|the model]]\"\nproject_note: x\n---\n\nSee [[projects/surrogate-model#Current State|state]] and ![[surrogate-model]].\nPlain prose, `[[ignored]]` too.\n";
    assert_eq!(MASK_VERSION, 1);
    assert_eq!(
        masked_content_hash(input, &["project", "portfolio", "related"]),
        "b1a220afef0009c9"
    );
}
