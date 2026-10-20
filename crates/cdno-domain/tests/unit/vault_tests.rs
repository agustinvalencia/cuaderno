use std::sync::Arc;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;

/// A fixed moment used across tests so formatted log lines stay
/// deterministic and assertions don't depend on the wall clock.
fn sample_moment() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 4, 20)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(14, 30, 0).unwrap())
}

fn daily_path() -> VaultPath {
    VaultPath::new("journal/2026/daily/2026-04-20.md").unwrap()
}

fn make_vault() -> (Vault, Arc<dyn VaultStore>, Arc<dyn VaultIndex>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(
        Arc::clone(&store),
        Arc::clone(&index),
        VaultConfig::default(),
    )
    .expect("Vault::new on empty store");
    (vault, store, index)
}

#[test]
fn new_vault_on_empty_store_produces_empty_report() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (_vault, report) =
        Vault::new(store, index, VaultConfig::default()).expect("Vault::new on empty store");

    assert_eq!(report.scanned, 0);
    assert_eq!(report.added, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.removed, 0);
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
}

#[test]
fn new_vault_rejects_a_malformed_ignore_glob() {
    // A bad pattern surfaces at vault-open as DomainError::Config, not a
    // panic or a silently-skipped rule — exercising the real
    // config.ignore_set()? wiring in Vault::new, not just IgnoreSet in
    // isolation.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let config = VaultConfig {
        ignore: vec!["a[".to_string()],
        ..Default::default()
    };
    // `Vault` isn't Debug, so match rather than expect_err.
    match Vault::new(store, index, config) {
        Err(cdno_domain::error::DomainError::Config(_)) => {}
        Err(other) => panic!("expected DomainError::Config, got: {other:?}"),
        Ok(_) => panic!("expected malformed glob to fail Vault::new"),
    }
}

#[test]
fn log_to_daily_note_creates_missing_file_with_logs_section() {
    let (vault, store, _index) = make_vault();

    let path = vault
        .log_to_daily_note(sample_moment(), "first entry of the day")
        .expect("log_to_daily_note on missing file");

    assert_eq!(path, daily_path());

    let content = store.read_file(&path).expect("daily note written");
    assert!(
        content.contains("type: daily"),
        "frontmatter missing type: daily\ncontent:\n{content}",
    );
    assert!(
        content.contains("## Logs\n"),
        "Logs section missing\ncontent:\n{content}",
    );
    assert!(
        content.contains("- **14:30**: first entry of the day\n"),
        "log line missing or wrong format\ncontent:\n{content}",
    );
}

#[test]
fn log_to_daily_note_appends_into_logs_section_of_existing_file() {
    let (vault, store, _index) = make_vault();

    // Two consecutive logs on the same day.
    vault
        .log_to_daily_note(sample_moment(), "first entry")
        .expect("first log succeeds");

    let later = NaiveDate::from_ymd_opt(2026, 4, 20)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(16, 5, 0).unwrap());
    vault
        .log_to_daily_note(later, "second entry")
        .expect("second log succeeds");

    let content = store.read_file(&daily_path()).unwrap();

    // Both lines present, first entry appears before the second, and
    // both live under the Logs heading (not tacked onto the end of
    // the file).
    let first_pos = content
        .find("- **14:30**: first entry")
        .expect("first log line");
    let second_pos = content
        .find("- **16:05**: second entry")
        .expect("second log line");
    let logs_pos = content.find("## Logs\n").expect("Logs heading");

    assert!(logs_pos < first_pos, "first log should follow Logs heading");
    assert!(first_pos < second_pos, "second log should come after first");
}

#[test]
fn log_to_daily_note_errors_when_logs_section_missing() {
    let (vault, store, _index) = make_vault();

    // Pre-seed a daily note without a Logs section so the append path
    // has nowhere to insert into. This is the unhappy case the
    // `MarkdownDocument` contract surfaces explicitly.
    let path = daily_path();
    store
        .write_file(
            &path,
            "---\ndate: 2026-04-20\ntype: daily\n---\n\n# Monday\n\n## Captured\n",
        )
        .unwrap();

    let err = vault
        .log_to_daily_note(sample_moment(), "entry with nowhere to go")
        .expect_err("should fail without a Logs section");

    // The raw error type isn't the focus; what matters is that the
    // failure surfaces rather than corrupting the file.
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("logs") || msg.to_lowercase().contains("section"),
        "error should mention the missing section; got: {msg}",
    );
}

#[test]
fn log_to_daily_note_upserts_index_row_for_the_daily_note() {
    let (vault, _store, index) = make_vault();

    let path = vault
        .log_to_daily_note(sample_moment(), "logged")
        .expect("log succeeds");

    let row = index
        .find_by_path(&path)
        .expect("index lookup")
        .expect("row present");

    assert_eq!(row.path, path);
    assert_eq!(row.note_type, "daily");
    assert!(
        row.size > 0,
        "indexed size should reflect the written content",
    );
    assert!(
        !row.content_hash.is_empty(),
        "content_hash should be populated",
    );
}

// ---------------------------------------------------------------------
// bootstrap::open_vault
// ---------------------------------------------------------------------

#[test]
fn open_vault_rejects_a_directory_without_the_marker() {
    let dir = tempfile::tempdir().unwrap();
    let Err(err) = cdno_domain::bootstrap::open_vault(dir.path()) else {
        panic!("a bare tempdir must not open as a vault");
    };
    assert!(matches!(
        err,
        cdno_domain::bootstrap::BootstrapError::NotAVault { .. }
    ));
}

// ---------------------------------------------------------------------
// The method-version gate in `Vault::transaction()` (RFC 0006 §8.4, T7a)
// ---------------------------------------------------------------------

use std::sync::Mutex;

use cdno_core::config::MethodVersion;
use cdno_core::error::{ConfigError, StoreError};
use cdno_core::file_meta::FileMeta;
use cdno_core::store::VaultWriteLock;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::EnergyLevel;
use cdno_domain::method_version::{Gate, METHOD_VERSION, MethodSkew, method_gate};
use cdno_domain::{Context, MethodStatus};

const CONFIG: &str = ".cuaderno/config.toml";

fn config_path() -> VaultPath {
    VaultPath::new(CONFIG).unwrap()
}

fn project_path() -> VaultPath {
    VaultPath::new("projects/gate-probe.md").unwrap()
}

fn version(major: u16, minor: u16) -> MethodVersion {
    MethodVersion { major, minor }
}

/// The config text of a vault stamped `version`, with nothing else in it.
fn stamped(version: MethodVersion) -> String {
    format!("[vault]\nname = \"Gate\"\nmethod_version = \"{version}\"\n")
}

/// A vault opened at the binary's own version: the config file on disk
/// and the config loaded at open agree, as `cdno init` leaves them. One
/// project, so a write has a map to land on.
fn stamped_vault() -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    store
        .write_file(&config_path(), &stamped(METHOD_VERSION))
        .unwrap();
    let mut config = VaultConfig::default();
    config.vault.method_version = Some(METHOD_VERSION);
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(Arc::clone(&store), index, config).expect("Vault::new");
    vault
        .create_project(sample_moment(), "Gate probe", Context::Work, None)
        .expect("create_project at the binary's version");
    (vault, store)
}

fn add_probe_action(vault: &Vault) -> Result<VaultPath, DomainError> {
    vault.add_action(
        sample_moment(),
        "gate-probe",
        "Write the probe",
        EnergyLevel::Light,
    )
}

#[test]
fn a_write_after_an_external_major_stamp_is_refused_and_writes_nothing() {
    let (vault, store) = stamped_vault();
    let map_before = store.read_file(&project_path()).unwrap();
    let daily_before = store.read_file(&daily_path()).unwrap();

    // Another process (a newer cdno's `cdno migrate`) stamps the vault past
    // this binary's major after this vault was opened.
    let ahead = version(METHOD_VERSION.major + 1, 0);
    store.write_file(&config_path(), &stamped(ahead)).unwrap();

    let err = add_probe_action(&vault).expect_err("a newer major must refuse the write");
    match &err {
        DomainError::MethodVersionAhead { vault, binary } => {
            assert_eq!(*vault, ahead);
            assert_eq!(*binary, METHOD_VERSION);
        }
        other => panic!("expected MethodVersionAhead, got: {other:?}"),
    }
    let message = err.to_string();
    for needle in [
        format!("RLM {ahead}"),
        format!("RLM {METHOD_VERSION}"),
        "upgrade cdno".to_owned(),
        "reads keep working".to_owned(),
    ] {
        assert!(message.contains(&needle), "{needle:?} not in: {message}");
    }

    assert_eq!(store.read_file(&project_path()).unwrap(), map_before);
    assert_eq!(store.read_file(&daily_path()).unwrap(), daily_before);

    // The status query reads the disk the same way.
    assert_eq!(
        vault.method_status().unwrap(),
        MethodStatus {
            vault: Some(ahead),
            binary: METHOD_VERSION,
            skew: MethodSkew::NewerMajor,
        }
    );
}

#[test]
fn a_write_to_a_vault_on_an_older_major_is_refused_and_writes_nothing() {
    let (vault, store) = stamped_vault();
    let map_before = store.read_file(&project_path()).unwrap();
    let daily_before = store.read_file(&daily_path()).unwrap();

    // 0.9 is an older major than the binary's 1.1.
    let behind = version(0, 9);
    assert_eq!(
        METHOD_VERSION,
        version(1, 1),
        "the probe assumes a 1.1 binary"
    );
    store.write_file(&config_path(), &stamped(behind)).unwrap();

    let err = add_probe_action(&vault).expect_err("an older major must refuse the write");
    match &err {
        DomainError::VaultNeedsMigration { vault, binary } => {
            assert_eq!(*vault, Some(behind));
            assert_eq!(*binary, METHOD_VERSION);
        }
        other => panic!("expected VaultNeedsMigration, got: {other:?}"),
    }
    let message = err.to_string();
    for needle in ["RLM 0.9", "RLM 1.1", "cdno migrate --dry-run"] {
        assert!(message.contains(needle), "{needle:?} not in: {message}");
    }

    assert_eq!(store.read_file(&project_path()).unwrap(), map_before);
    assert_eq!(store.read_file(&daily_path()).unwrap(), daily_before);

    // An unstamped vault is never said to follow 1.0 (only reachable with a
    // 2.0 binary, so the message is checked directly).
    let unstamped = DomainError::VaultNeedsMigration {
        vault: None,
        binary: version(2, 0),
    }
    .to_string();
    assert!(
        unstamped.contains("is unstamped (read as 1.0)") && !unstamped.contains("follows"),
        "{unstamped}"
    );
}

#[test]
fn reads_keep_working_on_a_newer_major() {
    let (_vault, store) = stamped_vault();
    let ahead = version(METHOD_VERSION.major + 1, 0);
    store.write_file(&config_path(), &stamped(ahead)).unwrap();

    // Reopen over a fresh index, as a binary started after the stamp would,
    // with the config it loads. Reconciliation rebuilds the index through
    // its own transaction, which the gate does not touch.
    let mut config = VaultConfig::default();
    config.vault.method_version = Some(ahead);
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, report) =
        Vault::new(Arc::clone(&store), index, config).expect("a newer major still opens");
    assert!(
        report.added > 0,
        "reconciliation indexed nothing: {report:?}"
    );

    let orientation = vault
        .orientation_context(sample_moment().date())
        .expect("a newer major still reads");
    assert!(
        orientation.projects.iter().any(|p| p.slug == "gate-probe"),
        "the project is missing from orientation"
    );
    assert!(matches!(
        add_probe_action(&vault),
        Err(DomainError::MethodVersionAhead { .. })
    ));
}

#[test]
fn a_vault_with_no_config_file_writes() {
    let (vault, store, _index) = make_vault();
    assert!(!store.exists(&config_path()).unwrap());

    vault
        .create_project(sample_moment(), "Gate probe", Context::Work, None)
        .expect("an unstamped vault writes");
    add_probe_action(&vault).expect("an unstamped vault writes");
    assert!(
        store
            .read_file(&project_path())
            .unwrap()
            .contains("Write the probe")
    );
    assert_eq!(vault.method_status().unwrap().vault, None);
}

#[test]
fn an_unrelated_config_edit_does_not_block_writes() {
    let (vault, store) = stamped_vault();

    // Edits `Vault::new` would reject (a wrongly typed `[vault]` key, a bad
    // glob, a `[focus]` typo), made while the vault is open: the re-read
    // takes only the version, so writes carry on.
    let edited = format!(
        "{}max_active_projects = \"five\"\nignore = [\"a[\"]\n\n[focus]\ncarry_over_day = 1\n",
        stamped(METHOD_VERSION)
    );
    store.write_file(&config_path(), &edited).unwrap();

    add_probe_action(&vault).expect("an unrelated config problem must not block a write");
    assert!(
        store
            .read_file(&project_path())
            .unwrap()
            .contains("Write the probe")
    );
}

#[test]
fn a_config_that_is_not_toml_on_reread_refuses_the_write() {
    // A file that is no longer TOML and, by the same rule, a malformed
    // version: both refuse with a config error whose own message (all an MCP
    // client sees) names the file, the key and the reason, never a fallback
    // to the version loaded at open.
    for (broken, reason) in [
        ("[vault\nmethod_version = \"1.0\"\n", "TOML parse error"),
        (
            "[vault]\nmethod_version = \"1.0.0\"\n",
            "`1.0.0` is not a method version",
        ),
    ] {
        let (vault, store) = stamped_vault();
        let map_before = store.read_file(&project_path()).unwrap();
        store.write_file(&config_path(), broken).unwrap();

        let err = add_probe_action(&vault).expect_err("an unreadable version must refuse");
        assert!(
            matches!(err, DomainError::Config(ConfigError::MethodVersion { .. })),
            "expected a method-version config error for {broken:?}, got: {err:?}"
        );
        let message = err.to_string();
        for needle in [CONFIG, "`[vault] method_version`", reason] {
            assert!(message.contains(needle), "{needle:?} not in: {message}");
        }
        assert_eq!(store.read_file(&project_path()).unwrap(), map_before);
    }
}

/// A `MemoryVaultStore` that records, in order, each lock acquisition and
/// each file read, for the ordering probe.
struct RecordingStore {
    inner: MemoryVaultStore,
    events: Mutex<Vec<String>>,
}

impl RecordingStore {
    fn record(&self, event: String) {
        self.events.lock().unwrap().push(event);
    }
}

impl VaultStore for RecordingStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.record(format!("read {path}"));
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.append_to_file(path, content)
    }
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.move_file(src, dest)
    }
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        self.inner.delete_file(path)
    }
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        self.inner.exists(path)
    }
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.list_dir(path)
    }
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.walk_dir(path)
    }
    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError> {
        self.inner.metadata(path)
    }
    fn import_external(&self, src: &std::path::Path, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.import_external(src, dest)
    }
    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        self.record("lock".to_owned());
        self.inner.acquire_write_lock()
    }
}

#[test]
fn the_version_is_read_after_the_lock_is_taken() {
    let recording = Arc::new(RecordingStore {
        inner: MemoryVaultStore::new(),
        events: Mutex::new(Vec::new()),
    });
    let store: Arc<dyn VaultStore> = recording.clone();
    store
        .write_file(&config_path(), &stamped(METHOD_VERSION))
        .unwrap();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    vault
        .create_project(sample_moment(), "Gate probe", Context::Work, None)
        .expect("create_project");

    recording.events.lock().unwrap().clear();
    add_probe_action(&vault).expect("add_action");
    let events = recording.events.lock().unwrap().clone();

    let lock = events
        .iter()
        .position(|e| e == "lock")
        .unwrap_or_else(|| panic!("the write took no lock: {events:?}"));
    let read = events
        .iter()
        .position(|e| *e == format!("read {CONFIG}"))
        .unwrap_or_else(|| panic!("the write never read the config: {events:?}"));
    assert!(
        lock < read,
        "the config was read before the lock was taken: {events:?}"
    );
}

#[test]
fn a_newer_minor_writes_and_notices_once() {
    let (vault, store) = stamped_vault();
    assert_eq!(
        METHOD_VERSION,
        version(1, 1),
        "the probe assumes a 1.1 binary"
    );
    // Opened at the binary's own version: nothing to notice.
    assert_eq!(vault.take_method_notice(), None);

    // Another process stamps the vault a newer minor while it is open: the
    // gate allows the write and records the skew from its re-read.
    let newer_minor = version(1, 2);
    store
        .write_file(&config_path(), &stamped(newer_minor))
        .unwrap();
    add_probe_action(&vault).expect("a newer minor writes");
    let expected = MethodStatus {
        vault: Some(newer_minor),
        binary: METHOD_VERSION,
        skew: MethodSkew::NewerMinor,
    };
    assert_eq!(vault.take_method_notice(), Some(expected));

    // A second write records it again; the notice is not yielded twice.
    vault
        .add_action(
            sample_moment(),
            "gate-probe",
            "Write the second probe",
            EnergyLevel::Light,
        )
        .expect("a newer minor writes again");
    assert_eq!(vault.take_method_notice(), None);
    let map = store.read_file(&project_path()).unwrap();
    assert!(
        map.contains("Write the probe") && map.contains("Write the second probe"),
        "{map}"
    );

    // A vault opened on a newer minor records it from the loaded config,
    // before any write.
    let mut config = VaultConfig::default();
    config.vault.method_version = Some(newer_minor);
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (reopened, _report) =
        Vault::new(Arc::clone(&store), index, config).expect("a newer minor opens");
    assert_eq!(reopened.take_method_notice(), Some(expected));
    assert_eq!(reopened.take_method_notice(), None);
}

#[test]
fn method_gate_decides_every_case() {
    let v = version;
    let (binary_1_0, binary_1_1, binary_2_0) = (v(1, 0), v(1, 1), v(2, 0));
    // (binary, vault, gate); `None` is an unstamped vault, read as 1.0.
    let cases = [
        (binary_1_0, Some(v(1, 0)), Gate::Allow, "equal"),
        (binary_1_0, None, Gate::Allow, "unstamped"),
        (binary_1_1, None, Gate::Allow, "unstamped, an older minor"),
        (binary_1_0, Some(v(1, 1)), Gate::Allow, "a newer minor"),
        (binary_1_1, Some(v(1, 0)), Gate::Allow, "an older minor"),
        (binary_2_0, Some(v(2, 1)), Gate::Allow, "a newer minor of 2"),
        (
            binary_1_0,
            Some(v(2, 0)),
            Gate::MethodVersionAhead,
            "a newer major",
        ),
        (
            binary_1_1,
            Some(v(3, 0)),
            Gate::MethodVersionAhead,
            "a newer major, a lower minor",
        ),
        (
            binary_2_0,
            Some(v(1, 9)),
            Gate::VaultNeedsMigration,
            "an older major, a higher minor",
        ),
        (
            binary_2_0,
            None,
            Gate::VaultNeedsMigration,
            "unstamped, an older major",
        ),
    ];
    for (binary, vault, expected, case) in cases {
        assert_eq!(
            method_gate(binary, vault),
            expected,
            "{case}: binary {binary}, vault {vault:?}"
        );
    }
}

#[test]
fn a_vault_declaring_a_project_held_by_schema_field_opens() {
    // The zero-code trial (RFC 0006 §10) declares `held_by` as a project
    // schema field. The key is reserved from `set_frontmatter` only, never
    // hard-reserved, so a vault that ran the trial still opens.
    let raw = "[schemas.project.fields.held_by]\ntype = \"string\"\n";
    let config: VaultConfig = toml::from_str(raw).expect("config parses");
    assert!(
        config
            .schema_for("project")
            .unwrap()
            .fields
            .contains_key("held_by")
    );
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    store
        .write_file(
            &VaultPath::new("projects/harbour.md").unwrap(),
            "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-09-01\n\
             held_by: \"the harbour office\"\n---\n# Harbour\n",
        )
        .unwrap();
    let (vault, _report) = Vault::new(store, index, config).expect("the trial's vault opens");
    let active = vault.active_projects().expect("active_projects");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].1.held_by.as_deref(), Some("the harbour office"));
}
