//! The Stewardship view seams against the Memory doubles — the
//! composed detail view-model, the list, and the template-field
//! discovery, no Tauri runtime involved.

use std::collections::HashMap;
use std::sync::Arc;

use cdno_core::config::{Variables, VaultConfig};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::vault::StewardshipVariant;
use cdno_tauri::commands::stewardships::{
    get_stewardship_detail_impl, get_tracking_template_fields_impl, list_stewardships_impl,
};
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with_config(notes: &[(&str, &str)], config: VaultConfig) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, config).expect("Vault::new");
    vault
}

fn vault_with(notes: &[(&str, &str)]) -> Vault {
    vault_with_config(notes, VaultConfig::default())
}

// An expanded stewardship (folder + `_index.md`) with two tracking
// notes, each carrying a body table — the fixture the detail view and
// its trend charts render.
const HEALTH_INDEX: &str = "---\ntype: stewardship\ncontext: personal\n---\n\n# Health\n\n## Current Status\nConsistent.\n";

const GYM_1: &str = "---\ntype: tracking\nstewardship: health\nactivity: gym\ndate: 2026-07-01\nduration_min: 60\nroutine: null\n---\n\n# Gym \u{2014} 1 July 2026\n\n| Sets | Reps |\n|------|------|\n| 3 | 5 |\n| 3 | 8 |\n\n## Notes\nSolid session.\n";

const GYM_2: &str = "---\ntype: tracking\nstewardship: health\nactivity: gym\ndate: 2026-07-05\nduration_min: 55\nroutine: null\n---\n\n# Gym \u{2014} 5 July 2026\n\n| Sets | Reps |\n|------|------|\n| 4 | 6 |\n\n## Notes\nFelt strong.\n";

// A flat stewardship: single file, no `tracking/` subdir.
const FINANCES: &str = "---\ntype: stewardship\ncontext: household\n---\n\n# Finances\n\n## Current Status\nOn top of it.\n";

#[test]
fn list_stewardships_returns_both_variants() {
    let vault = vault_with(&[
        ("stewardships/health/_index.md", HEALTH_INDEX),
        ("stewardships/health/tracking/2026-07-01-gym.md", GYM_1),
        ("stewardships/finances.md", FINANCES),
    ]);

    let rows = list_stewardships_impl(&vault, ymd(2026, 7, 8)).unwrap();
    let health = rows.iter().find(|s| s.slug == "health").unwrap();
    assert_eq!(health.variant, StewardshipVariant::Expanded);
    assert_eq!(health.tracking_count, 1);
    let finances = rows.iter().find(|s| s.slug == "finances").unwrap();
    assert_eq!(finances.variant, StewardshipVariant::Flat);
    assert_eq!(finances.tracking_count, 0);
}

#[test]
fn detail_composes_series_recent_and_count_for_an_expanded_stewardship() {
    let vault = vault_with(&[
        ("stewardships/health/_index.md", HEALTH_INDEX),
        ("stewardships/health/tracking/2026-07-01-gym.md", GYM_1),
        ("stewardships/health/tracking/2026-07-05-gym.md", GYM_2),
    ]);

    let detail = get_stewardship_detail_impl(&vault, "health").unwrap();

    assert_eq!(detail.slug, "health");
    assert_eq!(detail.name, "Health");
    assert_eq!(detail.variant, StewardshipVariant::Expanded);
    assert!(detail.body_markdown.contains("## Current Status"));

    // Two numeric columns (Sets, Reps) × the "gym" activity → two
    // series, each summed per note.
    assert_eq!(detail.series.len(), 2);
    let sets = detail
        .series
        .iter()
        .find(|s| s.name.contains("Sets"))
        .expect("a Sets series");
    // GYM_1 sums 3+3=6; GYM_2 is a single row of 4. Points are
    // date-sorted.
    assert_eq!(sets.points.len(), 2);
    assert_eq!(sets.points[0].value, 6.0);
    assert_eq!(sets.points[1].value, 4.0);

    // Recent is newest-first; the count is the full total.
    assert_eq!(detail.tracking_count, 2);
    assert_eq!(detail.recent.len(), 2);
    assert_eq!(detail.recent[0].date, ymd(2026, 7, 5));
    assert_eq!(detail.recent[0].activity, "gym");
    assert_eq!(
        detail.recent[0].path,
        "stewardships/health/tracking/2026-07-05-gym.md"
    );
    // The excerpt is the first non-blank body line after the H1 — the
    // table header here; we only assert it carries something to preview.
    assert!(!detail.recent[0].body_excerpt.is_empty());
}

#[test]
fn detail_of_a_flat_stewardship_has_no_series_and_no_tracking() {
    let vault = vault_with(&[("stewardships/finances.md", FINANCES)]);

    let detail = get_stewardship_detail_impl(&vault, "finances").unwrap();

    assert_eq!(detail.variant, StewardshipVariant::Flat);
    assert!(detail.series.is_empty(), "flat stewardships have no charts");
    assert!(detail.recent.is_empty());
    assert_eq!(detail.tracking_count, 0);
}

#[test]
fn template_fields_are_empty_for_the_generic_tracking_template() {
    let vault = vault_with(&[("stewardships/health/_index.md", HEALTH_INDEX)]);
    // No custom template and no prompt vars → nothing to gather; the
    // form shows only its static content/routine inputs.
    let fields = get_tracking_template_fields_impl(&vault, "gym").unwrap();
    assert!(fields.is_empty());
}

#[test]
fn template_fields_surface_a_custom_templates_prompt_vars() {
    // A vault whose config declares a prompted variable and whose
    // custom `tracking-gym` template references it — the fields the log
    // form must gather for the "gym" activity.
    let mut prompt = HashMap::new();
    prompt.insert("mood".to_owned(), "How did it feel?".to_owned());
    let config = VaultConfig {
        variables: Variables {
            static_vars: HashMap::new(),
            prompt,
        },
        ..VaultConfig::default()
    };
    let template = "---\ntype: tracking\nstewardship: {{stewardship}}\nactivity: gym\ndate: {{date}}\nmood: {{mood}}\n---\n\n# Gym\n\n{{content}}\n";
    let vault = vault_with_config(
        &[
            ("stewardships/health/_index.md", HEALTH_INDEX),
            (".cuaderno/templates/tracking-gym.md", template),
        ],
        config,
    );

    let fields = get_tracking_template_fields_impl(&vault, "gym").unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "mood");
    assert_eq!(fields[0].prompt, "How did it feel?");

    // A different activity with no custom template falls back to the
    // generic (no prompts).
    let none = get_tracking_template_fields_impl(&vault, "swim").unwrap();
    assert!(none.is_empty());
}
