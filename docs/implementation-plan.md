# Cuaderno — Implementation Plan

## Detailed architecture, patterns, and build sequence

-----

## 1. Introduction

This document is the companion to the Cuaderno Design Document. Where the design document specifies *what* the system does — its note types, folder structure, CLI commands, MCP tools, and UI views — this document specifies *how* it is built. It covers the internal architecture of each crate, the traits and abstractions that define the system’s extension points, the patterns that govern data flow and error handling, and the phased sequence in which the pieces should be assembled.

The intended reader is someone (including a future version of the author, or an AI assistant) who is about to write the code. It assumes familiarity with Rust, but when a design pattern or architectural concept is introduced, a brief refresher is provided so the document is self-contained.

-----

## 2. Guiding Architectural Principles

Before diving into crate internals, a few principles that should inform every implementation decision.

**Depend on abstractions, not concretions.** This is the Dependency Inversion Principle — the “D” in SOLID. In Rust, it means that the higher-level crates (`cdno-domain`, `cdno-cli`, `cdno-mcp`) should depend on traits defined in `cdno-core`, not on specific implementations. If the domain layer needs to read a file, it calls a method on a `VaultStore` trait, not a function that directly opens a file descriptor. This allows testing with in-memory fakes, and it allows swapping implementations (for example, a local filesystem store versus a future remote store) without changing the domain logic.

**Make illegal states unrepresentable.** Rust’s type system is powerful enough to encode many of the RLM’s constraints at compile time. A `ProjectStatus` enum with variants `Active`, `Parked`, and `Completed` prevents a project from being in an undefined state. A function that takes `ActiveProject` rather than `Project` guarantees at the type level that you are not accidentally operating on a parked project. Not every constraint can be encoded this way (the 5-project cap requires runtime checking), but the ones that can, should be.

**Errors are values, not exceptions.** Every function that can fail returns `Result<T, E>`. The error types are specific and informative — not `anyhow::Error` everywhere, but domain-specific errors that tell the caller exactly what went wrong and what they can do about it. The CLI and MCP layers translate these domain errors into user-facing messages or JSON-RPC error responses.

**Parse, don’t validate.** When reading a markdown file from disk, do not read it as raw text and then validate it in a separate step. Instead, parse it directly into a typed struct that can only represent valid states. If the frontmatter is missing a required field, the parse fails immediately with a clear error. If it succeeds, the resulting value is guaranteed to be well-formed. This is a pattern from the Haskell community that works beautifully in Rust.

-----

## 3. The Trait Landscape

This section defines the key traits that form the contract between crates. In Rust, a trait is an interface — a set of method signatures that a type must implement. Traits enable polymorphism: the domain layer can call methods on a `VaultStore` without knowing whether it is backed by a real filesystem or an in-memory map used in tests.

### 3.1 Storage Abstraction (`cdno-core`)

The core crate defines traits for interacting with the vault’s storage.

```rust
/// Read and write operations on the vault's file system.
/// The primary implementation is `FsVaultStore` (real files).
/// Tests use `MemoryVaultStore` (in-memory HashMap).
pub trait VaultStore: Send + Sync {
    /// Read the raw content of a file at the given vault-relative path.
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError>;

    /// Write content to a file, creating it if it does not exist.
    /// Returns the previous content if the file existed (for rollback).
    fn write_file(&self, path: &VaultPath, content: &str)
        -> Result<Option<String>, StoreError>;

    /// Append content to an existing file.
    fn append_to_file(&self, path: &VaultPath, content: &str)
        -> Result<(), StoreError>;

    /// Move a file from one path to another.
    fn move_file(&self, from: &VaultPath, to: &VaultPath)
        -> Result<(), StoreError>;

    /// Check whether a file exists.
    fn exists(&self, path: &VaultPath) -> bool;

    /// List all files in a directory (non-recursive).
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError>;

    /// List all files in a directory recursively.
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError>;

    /// Get file metadata (mtime, size).
    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError>;
}
```

The `VaultPath` type is a newtype over a relative path (relative to the vault root). It prevents accidentally using an absolute path or a path outside the vault. This is a small but important safety measure.

```rust
/// A path relative to the vault root. Cannot be constructed
/// from an absolute path or a path containing `..`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VaultPath(PathBuf);

impl VaultPath {
    pub fn new(relative: impl AsRef<Path>) -> Result<Self, PathError> {
        let path = relative.as_ref();
        if path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
            return Err(PathError::InvalidVaultPath(path.to_owned()));
        }
        Ok(Self(path.to_owned()))
    }
}
```

### 3.2 Index Abstraction (`cdno-core`)

The index is the query layer over the vault’s files. It caches parsed metadata so that queries do not require reading every file.

```rust
/// Query and update operations on the vault index.
/// The primary implementation is `SqliteIndex`.
/// Tests can use `MemoryIndex`.
pub trait VaultIndex: Send + Sync {
    /// Retrieve metadata for a single note.
    fn get_note(&self, path: &VaultPath) -> Result<Option<IndexedNote>, IndexError>;

    /// Query notes by type.
    fn notes_by_type(&self, note_type: NoteType) -> Result<Vec<IndexedNote>, IndexError>;

    /// Query notes by type and status.
    fn notes_by_status(&self, note_type: NoteType, status: &str)
        -> Result<Vec<IndexedNote>, IndexError>;

    /// Query notes within a directory (for portfolio contents).
    fn notes_in_dir(&self, dir: &VaultPath) -> Result<Vec<IndexedNote>, IndexError>;

    /// Full-text search across note content.
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, IndexError>;

    /// Index or re-index a single note.
    fn upsert_note(&mut self, path: &VaultPath, note: &IndexedNote) -> Result<(), IndexError>;

    /// Remove a note from the index.
    fn remove_note(&mut self, path: &VaultPath) -> Result<(), IndexError>;

    /// Get all deadline entries (for commitments aggregation).
    fn deadlines(&self, lookahead: Duration) -> Result<Vec<Deadline>, IndexError>;

    /// Get portfolio-level metadata (note count, last updated).
    fn portfolio_health(&self) -> Result<Vec<PortfolioSummary>, IndexError>;

    /// Begin a transaction (for atomic multi-note updates).
    fn begin_transaction(&mut self) -> Result<Transaction<'_>, IndexError>;
}
```

The `IndexedNote` struct contains everything extracted from a note’s frontmatter plus computed metadata:

```rust
pub struct IndexedNote {
    pub path: VaultPath,
    pub note_type: NoteType,
    pub title: String,
    pub frontmatter: Frontmatter,    // parsed YAML as typed fields
    pub content_hash: u64,           // xxhash of file content
    pub mtime: SystemTime,
    pub deadlines: Vec<Deadline>,    // extracted from milestones/periodic commitments
    pub links: Vec<VaultPath>,       // outgoing wikilinks
    pub tags: Vec<String>,
}
```

### 3.3 Markdown Manipulation (`cdno-core`)

Notes are not just blobs of text — they have structure (YAML frontmatter, headings, sections). The core crate provides tools for surgical manipulation.

```rust
/// Parse and manipulate markdown documents with YAML frontmatter.
pub trait MarkdownDocument {
    /// Parse a raw markdown string into a structured document.
    fn parse(raw: &str) -> Result<Self, ParseError> where Self: Sized;

    /// Get the parsed frontmatter as a typed struct.
    fn frontmatter(&self) -> &Frontmatter;

    /// Get the content of a specific section by heading.
    /// Returns None if the section does not exist.
    fn section(&self, heading: &str) -> Option<&str>;

    /// Replace the content of a specific section.
    /// The section is identified by its heading text.
    fn replace_section(&mut self, heading: &str, new_content: &str)
        -> Result<(), ManipulationError>;

    /// Append content to a specific section.
    fn append_to_section(&mut self, heading: &str, content: &str)
        -> Result<(), ManipulationError>;

    /// Render back to a raw markdown string.
    fn render(&self) -> String;
}
```

This trait is what makes project map mutations possible. When the domain layer calls `replace_section("Current State", new_text)`, the core crate handles finding the right heading, preserving everything else, and rendering the result. The domain layer never does string surgery on raw markdown.

### 3.4 File Watching (`cdno-core`)

```rust
/// Watch the vault for filesystem changes and emit events.
pub trait FileWatcher: Send {
    /// Start watching. Events are sent to the provided channel.
    fn watch(&self, sender: Sender<FileEvent>) -> Result<(), WatchError>;

    /// Stop watching.
    fn stop(&self);
}

pub enum FileEvent {
    Created(VaultPath),
    Modified(VaultPath),
    Deleted(VaultPath),
    Moved { from: VaultPath, to: VaultPath },
}
```

### 3.5 Template Engine (`cdno-core`)

```rust
/// Resolve variables in a template string.
pub trait TemplateEngine {
    /// Load a template for the given note type and optional variant.
    /// Checks custom templates first, falls back to built-in defaults.
    fn load_template(
        &self,
        note_type: NoteType,
        variant: Option<&str>,
    ) -> Result<Template, TemplateError>;

    /// Resolve all variables in a template given a context of values.
    fn render(
        &self,
        template: &Template,
        variables: &VariableContext,
    ) -> Result<RenderedNote, TemplateError>;
}

/// Layered variable resolution following the four-tier precedence.
pub struct VariableContext {
    builtins: HashMap<String, String>,        // tier 1: date, time, etc.
    contextual: HashMap<String, String>,      // tier 2: title, slug, etc.
    vault_level: HashMap<String, String>,     // tier 3: from config.toml
    prompted: HashMap<String, PromptedVar>,   // tier 4: ask user if missing
}
```

-----

## 4. The Domain Layer (`cdno-domain`)

This is the heart of cuaderno. It defines what the system *means* — the note types, the rules, the queries, and the state transitions. Everything in this crate is pure logic: no file I/O, no networking, no UI. It receives its dependencies (the `VaultStore`, the `VaultIndex`) via constructor injection.

### 4.1 Constructor Injection

*A brief refresher.* Dependency injection is the practice of providing a component’s dependencies from the outside rather than having it construct them internally. In Rust, the most common form is constructor injection: the struct’s `new()` function takes trait objects or generic parameters for its dependencies. This makes the component testable (pass in fakes) and flexible (swap implementations without changing logic).

The domain layer’s central type is the `Vault`, which holds references to all the infrastructure it needs:

```rust
/// The central domain object. Holds references to storage,
/// index, and configuration. All domain operations go through this.
pub struct Vault<S: VaultStore, I: VaultIndex> {
    store: S,
    index: I,
    config: VaultConfig,
    templates: TemplateEngine,
}

impl<S: VaultStore, I: VaultIndex> Vault<S, I> {
    pub fn new(store: S, index: I, config: VaultConfig) -> Self {
        let templates = TemplateEngine::new(&config, &store);
        Self { store, index, config, templates }
    }
}
```

**Generics vs trait objects — decide during Week 1.** The original design uses generics with trait bounds for static dispatch. However, since there are exactly two concrete instantiations (production and test), the ergonomic cost of generic parameter proliferation (every function touching `Vault` must be generic over `S` and `I`, infecting call sites up the stack) may outweigh the negligible runtime benefit. Run a spike in Week 1: implement `Vault` both ways and compare ergonomics. `Box<dyn VaultStore>` with dynamic dispatch may prove cleaner for a personal tool with no hot-path performance concerns. The concrete types are specified at the top level (in `main()` or in the test harness) and propagated down either way.

In production:

```rust
let store = FsVaultStore::new(vault_root);
let index = SqliteIndex::open(db_path)?;
let vault = Vault::new(store, index, config);
```

In tests:

```rust
let store = MemoryVaultStore::new();
let index = MemoryIndex::new();
let vault = Vault::new(store, index, test_config());
```

### 4.2 Note Type System

The domain crate defines the full taxonomy of note types as an enum. Rust enums are algebraic data types — each variant can carry different data, and the compiler enforces exhaustive matching, so you can never forget to handle a case.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoteType {
    Daily,
    Weekly,
    Project,
    Action,        // action-as-investigation manifest (see design §5.11)
    Portfolio,
    Evidence,
    Stewardship,
    Tracking,
    Question,
    Commitment,
    Inbox,
}
```

Each note type has an associated frontmatter schema that defines which fields are required, optional, and what types they must be. This is where the “parse, don’t validate” principle materialises.

```rust
/// Parsed and validated frontmatter for a project map.
pub struct ProjectFrontmatter {
    pub context: Context,
    pub status: ProjectStatus,
    pub created: NaiveDate,
    pub core_question: Option<VaultPath>,  // link to question note
    // ... extra fields from config schema
}

/// Parsed and validated frontmatter for an evidence note.
pub struct EvidenceFrontmatter {
    pub created: NaiveDate,
    pub source: String,
    pub portfolio: String,               // parent portfolio slug
    pub origin: VaultPath,               // mandatory from Phase 3 — see design §5.5
}

/// Parsed and validated frontmatter for an action note (§5.11).
pub struct ActionFrontmatter {
    pub status: ActionStatus,            // Active, Completed, Blocked
    pub project: String,                 // parent project slug
    pub energy: Energy,                  // Deep, Medium, Light
    pub milestone: Option<VaultPath>,    // wikilink to a project milestone
    pub due: Option<NaiveDate>,          // self-imposed deadline (only when standalone)
    pub created: NaiveDate,
    pub completed: Option<NaiveDate>,
    pub blocker: Option<String>,
    pub criteria: Option<String>,        // free text, manifest of "done"
    pub tags: Vec<String>,
}

// ... one struct per note type
```

Each frontmatter struct has a `TryFrom<Frontmatter>` implementation that validates all required fields and returns a specific error if anything is missing or malformed. Once you have a `ProjectFrontmatter`, it is guaranteed to be valid.

```rust
impl TryFrom<Frontmatter> for ProjectFrontmatter {
    type Error = ValidationError;

    fn try_from(fm: Frontmatter) -> Result<Self, Self::Error> {
        Ok(Self {
            context: fm.require_field::<Context>("context")?,
            status: fm.require_field::<ProjectStatus>("status")?,
            created: fm.require_field::<NaiveDate>("created")?,
            core_question: fm.optional_field::<VaultPath>("core_question")?,
        })
    }
}
```

### 4.3 Domain Operations

Domain operations are methods on the `Vault` struct. Each operation encapsulates a complete workflow — reading files, validating state, applying changes, updating the index, and logging to the daily entry. The key design choice: **every domain operation returns a `VaultTransaction` that must be explicitly committed.** This gives the caller (CLI, MCP, Tauri) control over when the transaction is finalised, and ensures that all changes within an operation are atomic.

*A brief refresher on the Command pattern.* The Command pattern encapsulates a request as an object, allowing you to parameterise operations, queue them, and support undo. In cuaderno, the `VaultTransaction` is the command object: it captures all the file writes and index updates that an operation requires, and the `commit()` method executes them atomically.

```rust
impl<S: VaultStore, I: VaultIndex> Vault<S, I> {
    /// Update a project's current state.
    /// Reads the old state, prepares a log entry, and writes the new state.
    /// Returns a transaction that must be committed.
    pub fn update_project_state(
        &self,
        project: &VaultPath,
        new_state: &str,
    ) -> Result<VaultTransaction, DomainError> {
        // 1. Read the project file.
        let raw = self.store.read_file(project)?;
        let mut doc = MarkdownDoc::parse(&raw)?;
        let fm = ProjectFrontmatter::try_from(doc.frontmatter().clone())?;

        // 2. Ensure the project is active.
        if fm.status != ProjectStatus::Active {
            return Err(DomainError::ProjectNotActive(project.clone()));
        }

        // 3. Extract the old state.
        let old_state = doc.section("Current State")
            .ok_or(DomainError::MissingSection("Current State"))?
            .to_owned();

        // 4. Build the log entry.
        let log_line = format!(
            "- [project:{}] State updated:\n  was: \"{}\"\n  now: \"{}\"",
            project.stem(), old_state.trim(), new_state.trim()
        );

        // 5. Replace the section.
        doc.replace_section("Current State", new_state)?;

        // 6. Build the transaction.
        let today = self.today_log_path();
        let mut tx = VaultTransaction::new();
        tx.write_file(project.clone(), doc.render());
        tx.append_to_file(today, log_line);
        // Index updates are derived from the file changes
        // when the transaction is committed.

        Ok(tx)
    }
}
```

This pattern is repeated for every domain operation. The caller — whether CLI, MCP handler, or Tauri command — receives the transaction and commits it:

```rust
// In the CLI layer:
let tx = vault.update_project_state(&project_path, &new_state)?;
tx.commit(&mut vault)?;
println!("Project state updated.");
```

### 4.4 The VaultTransaction

The transaction is the central mechanism for atomicity and history preservation. It collects file operations and executes them as a batch.

**Crash safety caveat.** The rollback logic only runs if the process is still alive. If the process crashes between writing file A and file B in a multi-file operation, startup reconciliation will detect the index-vs-filesystem inconsistency and re-index the changed files — but it has no concept of "this multi-file operation was only partially applied." The *logical* atomicity of the operation is lost across crashes. In practice, most operations touch 1-2 files, so the window is small and the consequence is minor (e.g., a project state updated without the corresponding daily log entry). The guarantee is: **file-level consistency** (every file that exists is correctly indexed), not **operation-level atomicity across crashes**. This is acceptable — the daily log is append-only, so the worst case is a missing log line, not data corruption.

```rust
pub struct VaultTransaction {
    writes: Vec<(VaultPath, String)>,
    appends: Vec<(VaultPath, String)>,
    moves: Vec<(VaultPath, VaultPath)>,
    rollback_snapshots: Vec<(VaultPath, Option<String>)>,
}

impl VaultTransaction {
    pub fn new() -> Self { ... }

    pub fn write_file(&mut self, path: VaultPath, content: String) { ... }
    pub fn append_to_file(&mut self, path: VaultPath, content: String) { ... }
    pub fn move_file(&mut self, from: VaultPath, to: VaultPath) { ... }

    /// Execute all operations atomically.
    pub fn commit<S: VaultStore, I: VaultIndex>(
        self,
        vault: &mut Vault<S, I>,
    ) -> Result<(), TransactionError> {
        // 1. Snapshot current content of all affected files.
        for (path, _) in &self.writes {
            let existing = vault.store.read_file(path).ok();
            self.rollback_snapshots.push((path.clone(), existing));
        }

        // 2. Begin index transaction.
        let mut idx_tx = vault.index.begin_transaction()?;

        // 3. Execute file operations.
        for (path, content) in &self.writes {
            if let Err(e) = vault.store.write_file(path, content) {
                self.rollback(&vault.store);
                return Err(e.into());
            }
        }
        for (path, content) in &self.appends {
            if let Err(e) = vault.store.append_to_file(path, content) {
                self.rollback(&vault.store);
                return Err(e.into());
            }
        }
        for (from, to) in &self.moves {
            if let Err(e) = vault.store.move_file(from, to) {
                self.rollback(&vault.store);
                return Err(e.into());
            }
        }

        // 4. Re-parse and re-index all affected files.
        for path in self.affected_paths() {
            let content = vault.store.read_file(&path)?;
            let note = parse_and_index(&content, &path)?;
            idx_tx.upsert_note(&path, &note)?;
        }

        // 5. Commit the index transaction.
        idx_tx.commit()?;

        Ok(())
    }

    fn rollback<S: VaultStore>(&self, store: &S) {
        for (path, snapshot) in &self.rollback_snapshots {
            match snapshot {
                Some(content) => { let _ = store.write_file(path, content); }
                None => { /* file did not exist, delete it */ }
            }
        }
    }
}
```

### 4.5 Business Rules

Some RLM rules require runtime enforcement rather than type-level encoding.

**The 5-project cap.** Before creating or activating a project, the domain layer queries the index for active projects and checks the count.

```rust
impl<S: VaultStore, I: VaultIndex> Vault<S, I> {
    pub fn create_project(
        &self,
        title: &str,
        context: Context,
        core_question: Option<VaultPath>,
    ) -> Result<VaultTransaction, DomainError> {
        let active = self.index.notes_by_status(NoteType::Project, "active")?;
        if active.len() >= self.config.max_active_projects {
            return Err(DomainError::ProjectCapReached {
                current: active.len(),
                max: self.config.max_active_projects,
                active_projects: active.iter().map(|n| n.title.clone()).collect(),
            });
        }
        // ... proceed with creation
    }
}
```

The error carries enough information for the CLI or skill to suggest which project to park.

**Commitments aggregation.** This is a query, not a write operation. It scans four sources (project milestones via the `milestones` index table, stewardship periodic commitments, standalone commitment notes, action notes with a `due:` field that aren't pinned to a milestone) and merges them. Action notes that reference a milestone via the `milestone:` field are *not* duplicated here — the milestone is the source of truth for the date. Pseudocode below shows the original three-source shape; the action `due:` source is added in the same pattern (read `notes_by_type(Action)`, filter to those with `due` set and no `milestone`).

```rust
impl<S: VaultStore, I: VaultIndex> Vault<S, I> {
    pub fn commitments(
        &self,
        lookahead: Duration,
    ) -> Result<Vec<CommitmentEntry>, DomainError> {
        let mut entries = Vec::new();
        let cutoff = Utc::now() + lookahead;

        // Source 1: Project milestones with hard deadlines.
        let projects = self.index.notes_by_type(NoteType::Project)?;
        for project in &projects {
            for deadline in &project.deadlines {
                if deadline.is_hard && deadline.date <= cutoff {
                    entries.push(CommitmentEntry {
                        date: deadline.date,
                        title: deadline.title.clone(),
                        source: CommitmentSource::Project(project.title.clone()),
                        is_overdue: deadline.date < Utc::now(),
                    });
                }
            }
        }

        // Source 2: Stewardship periodic commitments.
        let stewardships = self.index.notes_by_type(NoteType::Stewardship)?;
        for stewardship in &stewardships {
            for deadline in &stewardship.deadlines {
                if deadline.date <= cutoff {
                    entries.push(CommitmentEntry {
                        date: deadline.date,
                        title: deadline.title.clone(),
                        source: CommitmentSource::Stewardship(
                            stewardship.title.clone()
                        ),
                        is_overdue: deadline.date < Utc::now(),
                    });
                }
            }
        }

        // Source 3: Standalone commitment notes.
        let commitments = self.index.notes_by_type(NoteType::Commitment)?;
        for commitment in &commitments {
            for deadline in &commitment.deadlines {
                if deadline.date <= cutoff {
                    entries.push(CommitmentEntry {
                        date: deadline.date,
                        title: commitment.title.clone(),
                        source: CommitmentSource::Standalone,
                        is_overdue: deadline.date < Utc::now(),
                    });
                }
            }
        }

        // Sort by date.
        entries.sort_by_key(|e| e.date);
        Ok(entries)
    }
}
```

### 4.6 Reconciliation

The startup reconciliation logic lives in the domain layer because it needs to understand note types and frontmatter schemas to re-parse changed files. But it delegates file operations and index updates to the core traits.

```rust
impl<S: VaultStore, I: VaultIndex> Vault<S, I> {
    /// Reconcile the index with the current filesystem state.
    /// Called on startup and periodically for long-running processes.
    pub fn reconcile(&mut self) -> Result<ReconcileReport, DomainError> {
        let mut report = ReconcileReport::default();

        // Walk all files in the vault.
        let all_files = self.store.walk_dir(&VaultPath::root())?;

        for file_path in &all_files {
            if !is_markdown(&file_path) { continue; }

            let fs_meta = self.store.metadata(file_path)?;
            match self.index.get_note(file_path)? {
                Some(indexed) => {
                    // File exists in index. Check if changed.
                    if fs_meta.mtime != indexed.mtime {
                        let content = self.store.read_file(file_path)?;
                        let hash = xxhash(&content);
                        if hash != indexed.content_hash {
                            // Content changed. Re-parse and re-index.
                            let note = parse_and_index(&content, file_path)?;
                            self.index.upsert_note(file_path, &note)?;
                            report.updated += 1;
                        } else {
                            // Mtime changed but content identical.
                            // Just update mtime in index.
                            self.index.update_mtime(file_path, fs_meta.mtime)?;
                        }
                    }
                }
                None => {
                    // File not in index. New file created externally.
                    let content = self.store.read_file(file_path)?;
                    let note = parse_and_index(&content, file_path)?;
                    self.index.upsert_note(file_path, &note)?;
                    report.added += 1;
                }
            }
        }

        // Check for deleted files (in index but not on disk).
        let indexed_paths = self.index.all_paths()?;
        for indexed_path in &indexed_paths {
            if !self.store.exists(indexed_path) {
                self.index.remove_note(indexed_path)?;
                report.removed += 1;
            }
        }

        Ok(report)
    }
}
```

-----

## 5. The Interface Layers

The CLI, MCP, and Tauri crates are intentionally thin. They translate between their respective protocols and the domain layer. The domain layer does all the thinking.

### 5.1 CLI (`cdno-cli`)

The CLI uses `clap` for argument parsing, constructing the `Vault` with production implementations and delegating to domain methods.

*A brief refresher on the Facade pattern.* A facade provides a simplified interface to a complex subsystem. The CLI is a facade over the domain layer: each CLI command maps to one domain method call. The CLI’s job is to parse arguments, call the method, handle the result (print output or error), and exit.

**Interactive prompts (`cdno-cli::prompt` module).** Mutating commands declare their required inputs as clap-optional. The handler checks which required fields are missing and, if stdout is a TTY, prompts for them via `inquire` (fuzzy-search selectors for project / milestone / energy / status, calendar widget for dates, plain text for free-form fields). When at least one field was prompted, a preview is rendered and the user confirms before the `VaultTransaction` is committed. When the user supplied every required field via flags, the command runs straight through with no confirmation step — the same shape agentic clients use. Non-TTY sessions (piped, CI, redirected) error rather than hang. The prompt module reads from the existing index for selectors (`Vault::list_active_projects`, the `milestones` table) — no new domain surface is required. Domain stays sync, pure, I/O-free; prompting is purely a CLI concern.

```rust
fn main() -> Result<()> {
    let cli = Cli::parse();

    // Construct production dependencies.
    let config = VaultConfig::load(&cli.vault_path)?;
    let store = FsVaultStore::new(&cli.vault_path);
    let mut index = SqliteIndex::open(&cli.vault_path.join(".cuaderno/index.db"))?;
    let mut vault = Vault::new(store, index, config);

    // Reconcile on startup.
    let report = vault.reconcile()?;
    if report.has_changes() {
        eprintln!("Index reconciled: {} updated, {} added, {} removed.",
            report.updated, report.added, report.removed);
    }

    // Dispatch to command handler.
    match cli.command {
        Command::Log { message } => {
            let tx = vault.append_to_daily_log(&message)?;
            tx.commit(&mut vault)?;
        }
        Command::Project(ProjectCommand::State { project, new_state }) => {
            let tx = vault.update_project_state(&project, &new_state)?;
            tx.commit(&mut vault)?;
            println!("State updated for {}.", project);
        }
        Command::Commitments { weeks } => {
            let entries = vault.commitments(Duration::weeks(weeks))?;
            print_commitments_table(&entries);
        }
        // ... other commands
    }

    Ok(())
}
```

### 5.2 MCP Server (`cdno-mcp`)

The MCP crate has three internal layers: tool definitions, handlers, and transports.

**Tool definitions** are derived from the domain types. Each MCP tool has a name, description, and input/output JSON schema. In Rust, these schemas can be generated from the domain types using `schemars`:

```rust
/// Input for the get_orientation tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct OrientationInput {
    /// Lookahead window for commitments (default: "48h").
    #[serde(default = "default_lookahead")]
    pub lookahead: String,
}

/// Output for the get_orientation tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OrientationOutput {
    pub commitments: Vec<CommitmentEntry>,
    pub active_projects: Vec<ProjectSummary>,
    pub active_question_count: usize,
    pub lapsed_habits: Vec<LapsedHabit>,
}
```

**Handlers** are functions that take a typed input, call domain methods, and return a typed output. They are transport-agnostic.

```rust
pub fn handle_get_orientation<S: VaultStore, I: VaultIndex>(
    vault: &Vault<S, I>,
    input: OrientationInput,
) -> Result<OrientationOutput, HandlerError> {
    let lookahead = parse_duration(&input.lookahead)?;
    let commitments = vault.commitments(lookahead)?;
    let projects = vault.active_project_summaries()?;
    let questions = vault.active_questions()?.len();
    let habits = vault.lapsed_habits()?;

    Ok(OrientationOutput {
        commitments,
        active_projects: projects,
        active_question_count: questions,
        lapsed_habits: habits,
    })
}
```

**Transports** implement the MCP protocol over a specific medium.

*A brief refresher on the Adapter pattern.* An adapter converts the interface of one system into the interface expected by another. The stdio transport adapts between line-based JSON-RPC on stdin/stdout and the typed handler functions. The HTTP transport adapts between HTTP requests/responses (with SSE for streaming) and the same handler functions.

```rust
/// The transport trait. Each transport reads MCP requests,
/// dispatches to handlers, and sends responses.
pub trait McpTransport {
    /// Run the transport's event loop.
    fn serve<S: VaultStore, I: VaultIndex>(
        &self,
        vault: Arc<Mutex<Vault<S, I>>>,
        handlers: &HandlerRegistry,
    ) -> Result<(), TransportError>;
}

pub struct StdioTransport;
pub struct HttpTransport {
    port: u16,
    auth_token: Option<String>,
}

impl McpTransport for StdioTransport { ... }
impl McpTransport for HttpTransport { ... }
```

The `HandlerRegistry` is a map from tool names to handler functions, built at startup from the handler definitions. This is the Strategy pattern: the registry selects the right handler based on the incoming tool name.

### 5.3 Tauri Backend (`cdno-tauri`)

The Tauri crate follows the same pattern as the MCP crate, but translates between Tauri IPC (`invoke`) and domain methods instead of JSON-RPC and domain methods.

Tauri manages state through its built-in state management. The `Vault` is wrapped in a `Mutex` and registered as Tauri managed state:

```rust
fn main() {
    let config = VaultConfig::load(&vault_path).unwrap();
    let store = FsVaultStore::new(&vault_path);
    let index = SqliteIndex::open(&db_path).unwrap();
    let vault = Vault::new(store, index, config);

    // Start file watcher for live index updates.
    let watcher = FsFileWatcher::new(&vault_path);
    let (tx, rx) = channel();
    watcher.watch(tx).unwrap();
    // Spawn a thread to process file events and update the index.
    spawn_watcher_thread(rx, vault.clone());

    tauri::Builder::default()
        .manage(Mutex::new(vault))
        .invoke_handler(tauri::generate_handler![
            get_orientation,
            get_weekly_context,
            update_project_state,
            // ... all commands
        ])
        .run(tauri::generate_context!())
        .unwrap();
}

#[tauri::command]
fn get_orientation(
    vault: tauri::State<'_, Mutex<Vault<FsVaultStore, SqliteIndex>>>,
) -> Result<OrientationOutput, String> {
    let vault = vault.lock().map_err(|e| e.to_string())?;
    let input = OrientationInput::default();
    handle_get_orientation(&vault, input).map_err(|e| e.to_string())
}
```

**Shared layer clarification.** The Tauri commands should call domain methods directly (`vault.commitments()`, `vault.update_project_state()`), not the MCP handler wrappers. MCP handlers deal with JSON-serializable input/output types and add serialisation overhead that Tauri doesn't need — Tauri can work with richer Rust types directly via Tauri's own serde bridge. The right shared layer is the domain methods themselves, not the MCP handlers. Both MCP and Tauri are thin translation layers over the same domain API, but they translate differently.

-----

## 6. Error Handling Strategy

Cuaderno uses a layered error strategy. Each layer defines its own error type, and errors are translated at layer boundaries.

*A brief refresher on the `thiserror` pattern.* In Rust, the `thiserror` crate lets you derive `Error` implementations declaratively, with automatic `From` conversions for wrapping lower-level errors. This creates a clean error hierarchy where each layer has specific, informative errors.

```rust
// cdno-core errors
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("File not found: {0}")]
    NotFound(VaultPath),
    #[error("Permission denied: {0}")]
    PermissionDenied(VaultPath),
    #[error("I/O error on {path}: {source}")]
    Io { path: VaultPath, source: std::io::Error },
}

// cdno-domain errors
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("Project cap reached ({current}/{max}). Active: {active_projects:?}")]
    ProjectCapReached {
        current: usize,
        max: usize,
        active_projects: Vec<String>,
    },
    #[error("Project is not active: {0}")]
    ProjectNotActive(VaultPath),
    #[error("Missing section '{0}' in note")]
    MissingSection(&'static str),
    #[error("Validation failed: {0}")]
    Validation(#[from] ValidationError),
    #[error("Storage error: {0}")]
    Store(#[from] StoreError),
    #[error("Index error: {0}")]
    Index(#[from] IndexError),
}
```

The CLI translates `DomainError` into user-facing messages. The MCP layer translates it into JSON-RPC error responses. The Tauri layer translates it into a string returned to the React frontend. Each translation is a simple `match` on the error variants, formatting appropriately for the medium.

-----

## 7. Testing Strategy

Each crate has a different testing profile.

**cdno-core**: unit tests with real files in a temporary directory (using `tempfile`). Tests the markdown parser, the section manipulator, the template engine, and the SQLite index against actual filesystem behaviour. This is the layer where integration with the OS matters, so fakes are not appropriate.

**cdno-domain**: unit tests with `MemoryVaultStore` and `MemoryIndex`. This is where the bulk of the test suite lives. Every domain operation, every business rule, every query is tested against in-memory fakes. Tests are fast (no disk I/O) and deterministic (no filesystem race conditions). Example:

```rust
#[test]
fn project_cap_is_enforced() {
    let mut vault = test_vault();  // MemoryVaultStore + MemoryIndex

    // Create 5 projects.
    for i in 0..5 {
        let tx = vault.create_project(
            &format!("Project {i}"), Context::Work, None
        ).unwrap();
        tx.commit(&mut vault).unwrap();
    }

    // Sixth project should fail.
    let result = vault.create_project("Project 5", Context::Work, None);
    assert!(matches!(result, Err(DomainError::ProjectCapReached { .. })));
}

#[test]
fn project_state_update_logs_history() {
    let mut vault = test_vault();
    create_test_project(&mut vault, "test-project");

    let tx = vault.update_project_state(
        &vpath("projects/test-project.md"),
        "New state text",
    ).unwrap();
    tx.commit(&mut vault).unwrap();

    // Check that the daily log contains the state change.
    let log = vault.store.read_file(&vault.today_log_path()).unwrap();
    assert!(log.contains("[project:test-project] State updated:"));
    assert!(log.contains("was:"));
    assert!(log.contains("now: \"New state text\""));
}
```

**cdno-cli**: integration tests that run the CLI binary as a subprocess against a temporary vault directory. These test the full pipeline from command-line arguments through domain operations to file output. Sparse — only testing that the wiring is correct, not re-testing domain logic.

**cdno-mcp**: integration tests that send JSON-RPC messages to the stdio transport and verify responses. Tests the serialisation/deserialisation and the handler dispatch, not the domain logic.

**cdno-tauri**: tested via the React frontend’s integration tests (Playwright or similar), not directly.

-----

## 8. Build Sequence

This section maps onto the design document’s build phases but adds implementation detail and sequencing within each phase.

### Phase 1: Foundation (estimated: 3-4 weeks)

The goal is a working vault with the correct folder structure, basic logging, and a proven transactional core.

**Week 1: Workspace and core primitives.**

Set up the cargo workspace with `cdno-core` and `cdno-domain`. Implement `VaultPath`, `FileMeta`, and `NoteType`. Implement the `VaultStore` trait and `FsVaultStore`. Implement the `MarkdownDocument` trait with frontmatter parsing (using `serde_yaml`) and section manipulation. Write thorough unit tests for the markdown parser — this is the foundation everything else depends on.

**Week 2: Index and reconciliation.**

Implement the `VaultIndex` trait in `cdno-core` and the `SqliteIndex` implementation. **Note on crate placement**: the `VaultIndex` trait belongs in core (it's the abstraction), but `SqliteIndex` is domain-shaped — its schema (notes with `note_type`, deadlines table, links table) reflects RLM concepts. If the "core has no domain knowledge" boundary feels violated, consider placing `SqliteIndex` in `cdno-domain` or a dedicated `cdno-index` crate. Pragmatically, keeping it in core is fine for now — revisit if the schema grows more domain-specific. Design the schema: a `notes` table with columns for path, type, title, content hash, mtime, and a JSON blob for variable frontmatter fields; a `deadlines` table for the commitments aggregation; a `links` table for wikilink tracking. Configure WAL mode. Implement `VaultTransaction` with atomic writes and rollback. Implement startup reconciliation. Write tests for every failure mode: crash between file write and index commit, external file creation, external deletion, content change without mtime change (rare but possible).

**Week 3: Templates and configuration.**

Implement `VaultConfig` loading from `.cuaderno/config.toml`. Implement the `TemplateEngine` with four-tier variable resolution. Ship built-in default templates compiled into the binary (using `include_str!`). Implement template selection logic (custom > built-in). Write the `cdno init` command that creates the vault folder structure and default configuration.

**Week 4: Domain scaffolding and basic CLI.**

Implement the `Vault` struct with constructor injection. Implement `append_to_daily_log` (the simplest domain operation — scaffolds today’s log if needed, appends a line). Implement `cdno log`, `cdno capture`, and `cdno lint` commands. Write `MemoryVaultStore` and `MemoryIndex` for testing. Write the first domain tests.

**Phase 1 deliverable**: `cdno init` creates a vault. `cdno log "text"` appends to today’s daily log. `cdno lint` validates all notes. The transactional core and reconciliation are proven.

### Phase 2: Daily Loop (estimated: 5-6 weeks)

> **Rationale for reordering.** The original plan put the knowledge layer (portfolios, questions) before the operations layer (projects, orient). This delays the point at which the tool becomes daily-usable to ~week 10. By pulling project maps, commitments, and the orient command into Phase 2, you can eat your own dogfood by ~week 7. Portfolios and questions are valuable but less critical for the daily loop — they move to Phase 3.
>
> **2026-05-03 update.** The action layer (design §5.11), the `milestones` and `tags` index tables, and the `cdno-cli::prompt` ergonomics module are pulled into this phase. The action layer was originally scoped for Phase 6 pending dogfooding (see vault note `decision-task-notes-thin-layer.md`). The call was reversed on adoption grounds: the mdvault → RLM transition needs the scaffold, and shipping unused costs less than discovering its absence mid-transition. The thin-ness rules in design §5.11 keep "later we could not use it" a real option. Sequence within the phase: action layer first (its schema is the most uncertain piece), then commitments aggregation builds on top, then orient surfaces both. Estimate grows from 3-4 to 5-6 weeks accordingly.

**Project maps.** The most complex domain object. Implement creation (with template, 5-cap enforcement, question linking), state update (with history logging), waiting-on tracking, milestone management (now reframed as Gantt-style event markers — see design §5.3), parking and activation. Each operation produces a `VaultTransaction`. Write extensive tests for the cap enforcement, the history logging, and the state update atomicity.

**Bullet actions** (the default form). Implement `add_action`, `complete_action` operating on the project's `## Next Actions` body section. Energy tags. Substring-based completion query with disambiguation errors when ambiguous. Daily-log entry on every mutation.

**Action notes** (the heavier form, design §5.11). Implement the `action` note type — frontmatter struct + template + lifecycle. `cdno action add ... --note` creates a bullet **and** an action note linked from it. `cdno action promote` finds an existing bullet and attaches a note (rewriting the bullet to wikilink the new note). `cdno action complete` is two writes in one transaction: bullet removed (logged to daily) **and**, if a note is attached, frontmatter status updated and the file moved to `actions/_done/<year>/`. Append-only-after-completion is enforced via lint, not file-system locks. Schema enforcement primitives (soft line-cap warning, "this looks like evidence" hint) live in `cdno-domain::lint` as a generic mechanism reusable across note types.

**Milestones index table.** Extend reconciliation to populate a `milestones(project_id, name, date, hard_soft, status)` table from project bodies. Queries become O(1) without promoting milestones to a note type. The deadlines source for commitments aggregation reads from this table.

**Tags index table.** Extend reconciliation to populate a `tags(note_id, tag)` table from `#tag` mentions across notes (especially `#action/<slug>` in daily logs). The Faraday-style query "all entries tagged X" becomes a join, not a file scan. Generic infrastructure — useful for evidence cross-tag queries in Phase 3 too.

**Commitment notes.** Implement creation and completion (move to `_done/<year>/`). Straightforward.

**Commitments aggregation.** Implement the four-source query (project milestones via the `milestones` index table, action `due:` for standalone-deadline action notes, standalone commitments, stewardship periodics — the last absent until Phase 3, handled gracefully).

**CLI ergonomics module.** `cdno-cli::prompt` using `inquire`. Optional flags + TTY-detected prompts for missing required fields + confirm-on-prompt before commit. Apply to every new mutating command in this phase as it lands; retrofit existing project/commitment commands when they're touched.

**The `cdno orient` command.** Daily orientation CLI flow: query commitments (48h), active project summaries (now including any attached action notes with status), prompt for energy level, suggest a starting point. First command that composes multiple domain queries into a user-facing workflow.

**CLI commands**: `cdno project`, `cdno action`, `cdno commit`, `cdno commitments`, `cdno orient`.

**Phase 2 deliverable**: the tool is daily-usable. Projects with 5-cap, actions (default bullet, optional note form via `--note`), commitments, milestones as event markers in their own index table, tag-indexed daily logs, and daily orientation from the terminal — with interactive prompting and confirm-on-prompt for the exploratory path. The minimum viable practice of the RLM, with the adoption scaffold for users migrating from mdvault.

### Phase 3: Knowledge & Stewardship Layer (estimated: 3-4 weeks)

**Portfolio operations.** Implement portfolio creation (folder + `_index.md`), evidence filing (create note inside portfolio folder), portfolio listing (note counts, last updated from index), and portfolio content browsing. The key indexing work is treating folders as first-class entities — the `portfolio_health()` query aggregates per-folder.

**`origin:` mandatory on evidence.** From day one of this phase, evidence frontmatter requires an `origin:` wikilink (see design §5.5). It points to whatever produced the evidence — a project, an action note, or a stewardship. The forward-link gives provenance and the backlink falls out of the index for free, so actions and projects can list their evidence without duplicating any structural data. Cheap to bake in now, expensive to migrate later.

**Question notes.** Implement creation, status transitions (active/parked/answered/retired), and the `active_questions()` query. These are simple notes with status fields — less complex than projects.

**Stewardships.** Implement flat and expanded variants. Flat stewardships are single files with simple CRUD. Expanded stewardships create a folder with `_index.md`, `tracking/`, and optionally `routines/`. Implement periodic commitment management (add, update, deadline computation). Implement tracking note scaffolding for different activities (gym, body, swim — using activity-specific templates). Once stewardships exist, the commitments aggregation query from Phase 2 automatically picks up their periodic commitments — no changes needed.

**CLI commands**: `cdno portfolio create/list/show`, `cdno file`, `cdno questions`, `cdno stewardship`, `cdno track`.

### Phase 4: MCP Server (estimated: 3-4 weeks)

**Handler layer.** Implement all MCP tool handlers as functions calling domain methods. Derive JSON schemas from input/output types using `schemars`. Build the `HandlerRegistry`.

**Stdio transport.** Implement the JSON-RPC-over-stdio event loop. Handle the MCP initialisation handshake, tool listing, and tool invocation. Test with Claude Desktop.

**Skill adaptation.** Update all Claude skill markdown files to use the new MCP tool names and response shapes. Test each skill end-to-end: daily-orientation, weekly-review, monthly-review, file-to-portfolio, create-project (with cap enforcement), triage.

**File watcher integration.** For MCP sessions, start a file watcher thread that updates the index on external changes. This matters because the user might edit a project map in Obsidian during a Claude conversation, and the MCP server needs to see the change.

### Phase 5: Cuaderno UI (estimated: 4-6 weeks)

**Tauri setup.** Create the `cdno-tauri` crate. Register `Vault` as managed state. Implement Tauri commands that call the shared handler functions. Set up the React frontend with Vite and Tremor.

**Home / Daily Orientation view.** The most important UI view. Commitments strip, project cards with state and top action, energy selector. Implement file watching for live updates.

**Weekly Review view.** Guided flow: wins composer (pre-populated from domain queries), project state editor (inline, calls `update_project_state` on blur), stewardship scan, commitments lookahead.

**Commitments Timeline.** Date-sorted, colour-coded by context, filterable. The simplest view to implement but high daily value.

### Phase 6: Extended UI and HTTP (estimated: 4-6 weeks)

**Strategic / Monthly view.** Questions grid, portfolio health table, project slot allocator, stewardship overview.

**Portfolio Browser.** Evidence timeline with inline preview, quick-add.

**Stewardship Detail.** Tracking charts (using Tremor’s charting components or Recharts), recent entries.

**HTTP transport.** Implement the Axum server with SSE. Add token-based authentication. Add the periodic reconciliation background task. Deploy and test remote access.

### Phase 7: Migration (estimated: 1-2 weeks)

**Migration tool.** `cdno migrate --from-mdv <vault_path>`. Reads the old vault, maps note types (project → project map, task → inline action or work-item, zettel → evidence or standalone note), and writes the new structure. Interactive: for each ambiguous note, asks the user where it should go. Designed to be run incrementally — you can migrate a few notes at a time during monthly reviews.

-----

## 9. Dependency Summary

External crates the project will depend on:

|Crate                                |Purpose                          |Used in             |
|-------------------------------------|---------------------------------|--------------------|
|`serde` + `serde_yaml` + `serde_json`|Serialisation                    |core, domain, mcp   |
|`rusqlite`                           |SQLite index                     |core                |
|`clap`                               |CLI argument parsing             |cli                 |
|`inquire`                            |Interactive prompts (fuzzy selectors, date widget) for missing required fields. Confirm-on-prompt before commit. CLI only — agentic clients always supply full args.|cli                 |
|`notify`                             |Filesystem watching              |core                |
|`thiserror`                          |Error type derivation            |all                 |
|`chrono`                             |Date/time handling               |core, domain        |
|`xxhash-rust`                        |Fast content hashing             |core                |
|`schemars`                           |JSON Schema generation from types|mcp                 |
|`axum` + `tokio`                     |HTTP server and async runtime    |mcp (HTTP transport)|
|`tauri`                              |Desktop app framework            |tauri               |
|`tempfile`                           |Temporary directories for tests  |core (dev)          |

-----

## 10. Open Questions

A few decisions deferred to implementation time:

**Async or sync in the domain layer?** The domain layer’s operations are fundamentally synchronous (read file, compute, write file). Making them async adds complexity without obvious benefit for the CLI. However, the MCP HTTP transport and the Tauri app benefit from async I/O. The pragmatic answer is: keep the domain layer synchronous and use `tokio::task::spawn_blocking` in the async contexts (MCP HTTP, Tauri) to call domain methods without blocking the event loop.

**How deep should deadline parsing go?** Parsing “hard: 2026-05-22” from a milestone line is straightforward regex. Parsing “every 6 months — next: 2026-04” from a stewardship periodic commitment requires understanding recurrence patterns. How much of this should be in the indexer (fast but limited) versus computed on-the-fly by the domain layer (flexible but slower)? The recommendation is: index the next occurrence date only, and let the domain layer compute future occurrences when needed.

**Should the UI embed Claude, or should it delegate to Claude Desktop?** The design document specifies that the Tauri app can call the Anthropic API directly for agentic workflows. This is powerful but adds API key management, cost control, and latency to the UI. An alternative is to have the UI open a deep link to Claude Desktop with pre-filled context. Decide during Phase 5 based on what the Claude API ecosystem looks like at that point.

**Template hot-reloading.** Should the tool detect changes to templates in `.cuaderno/templates/` and reload them without restarting? This is a nice-to-have for iterating on templates but adds complexity. Initially, require a restart; add hot-reloading later if it proves annoying.

**Concurrency model for the HTTP transport.** The current design wraps `Vault` in `Arc<Mutex<...>>`, which serialises all requests. For a personal tool this is fine, but it becomes a bottleneck if the HTTP transport ever serves multiple users (the "multi-vault" future extension). Consider `RwLock` instead of `Mutex` to allow concurrent reads — most MCP tools are read-heavy (context gathering). Write operations still serialise, but reads (which dominate) can proceed in parallel. Decide during Phase 6 based on actual usage patterns.

-----

*This implementation plan is a living document. As each phase is built, discoveries will feed back into the design. The architectural constraints (trait-based abstraction, transactional writes, parse-don’t-validate) should be treated as invariants. The specific implementations and sequencing are guidance, not commitments.*