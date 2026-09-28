//! The bundled note-type registry (RFC 0003 §4.3).
//!
//! A bundled type is installed into vaults the owner has already filled, so a
//! future entry that collides with a built-in would do its damage silently: a
//! template named `project.md` becomes the project type's override, and a
//! folder under `journal/` mixes custom notes into the log. These invariants
//! refuse such an entry at test time rather than in someone's vault.

use cdno_cli::bundled::{BUNDLED, template_in_force};
use cdno_core::paths::{BUNDLED_NOTE_TYPE_NAMES, RESERVED_TOP_LEVEL_FOLDERS};
use cdno_domain::note_type::NoteType;

#[test]
fn no_bundled_name_is_a_builtin_type() {
    for bundled in BUNDLED {
        assert!(
            !NoteType::ALL.iter().any(|t| t.as_str() == bundled.name),
            "bundled `{}` shadows a built-in type",
            bundled.name
        );
    }
}

#[test]
fn no_bundled_template_would_override_a_builtin_template() {
    for bundled in BUNDLED {
        let file = bundled.template_filename;
        for builtin in NoteType::ALL {
            let name = builtin.as_str();
            assert_ne!(
                file,
                format!("{name}.md"),
                "bundled `{}` template would override the `{name}` template",
                bundled.name
            );
            assert!(
                !(file.starts_with(&format!("{name}-")) && file.ends_with(".md")),
                "bundled `{}` template `{file}` would be a `{name}` variant",
                bundled.name
            );
        }
    }
}

#[test]
fn no_bundled_folder_is_reserved() {
    for bundled in BUNDLED {
        let declaration = bundled.declaration().expect("block parses");
        let top = declaration
            .folder
            .split('/')
            .next()
            .expect("a folder has a first segment");
        assert!(
            !RESERVED_TOP_LEVEL_FOLDERS.contains(&top),
            "bundled `{}` folder `{}` is under reserved `{top}`",
            bundled.name,
            declaration.folder
        );
    }
}

#[test]
fn every_block_declares_its_own_name_and_names_its_template() {
    for bundled in BUNDLED {
        let declaration = bundled
            .declaration()
            .unwrap_or_else(|e| panic!("bundled `{}`: {e}", bundled.name));
        assert_eq!(
            template_in_force(bundled.name, &declaration),
            bundled.template_filename,
            "bundled `{}` block names a different template file",
            bundled.name
        );
        // Standalone, the block is a config the vault would open.
        cdno_domain::validate_config_str(bundled.block)
            .unwrap_or_else(|e| panic!("bundled `{}` block: {}", bundled.name, e.message));
    }
}

#[test]
fn core_names_match_the_registry() {
    // `cdno-mcp` reads the names from `cdno-core` so it need not depend on
    // the CLI; the two lists must be the same.
    let registry: Vec<&str> = BUNDLED.iter().map(|t| t.name).collect();
    assert_eq!(registry, BUNDLED_NOTE_TYPE_NAMES);
}
