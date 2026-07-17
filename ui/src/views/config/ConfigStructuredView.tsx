// Structured Config view (#365) — the "Form" side of the Config editor.
// PR5a shipped this read-only; PR5b makes it EDITABLE: add/edit/remove
// custom note types and schema fields, add/edit/remove their declarations.
//
// The seam is string-in/string-out and the source of truth for
// persistence is ALWAYS the shared `useConfigDraft` draft STRING plus the
// surgical `config_*` commands — never a client re-serialise (that would
// drop comments/order). Each committed edit calls the matching command
// with the current draft, gets a new draft string back, and `setDraft`s
// it; the existing Save button then runs the exact validate ->
// compare-and-swap -> write -> live-reload gate as the raw editor. The
// form derives what it renders by PARSING the draft (`parseConfigModel`),
// so it always mirrors the live, not-yet-saved buffer.
//
// The backend stays the single authority on validity. The client-side
// pre-checks below (reserved folders, built-in type names) only block
// obviously-bad input for a calmer UX — the server error always renders.
import { useEffect, useRef, useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import type { ConfigModel } from "../../api/bindings/ConfigModel";
import type { CustomNoteType } from "../../api/bindings/CustomNoteType";
import type { FieldSpec } from "../../api/bindings/FieldSpec";
import type { FieldType } from "../../api/bindings/FieldType";
import type { NamedSchema } from "../../api/bindings/NamedSchema";
import type { NamedValue } from "../../api/bindings/NamedValue";
import {
  configRemoveNoteType,
  configRemovePromptVariable,
  configRemoveSchemaField,
  configRemoveVariable,
  configSetNoteType,
  configSetPromptVariable,
  configSetSchemaField,
  configSetVariable,
  errorMessage,
  parseConfigModel,
} from "../../api/commands";
import { useToast } from "../../shell/Toasts";
import type { ConfigDraft } from "./useConfigDraft";

/** Top-level folders the vault reserves; a custom type's `folder` may not
 * collide with one. Mirrors `RESERVED_TOP_LEVEL_FOLDERS` (core/paths.rs)
 * for a friendly pre-check — the server gate stays authoritative. */
const RESERVED_FOLDERS = new Set([
  "journal",
  "projects",
  "portfolios",
  "stewardships",
  "commitments",
  "actions",
  "questions",
  "inbox",
  ".cuaderno",
]);

/** Built-in note type names a custom type may not shadow (case-insensitive).
 * Mirrors the domain `NoteType` set for a friendly pre-check. */
const BUILTIN_NOTE_TYPES = new Set([
  "daily",
  "weekly",
  "monthly",
  "project",
  "action",
  "portfolio",
  "evidence",
  "stewardship",
  "tracking",
  "question",
  "commitment",
  "inbox",
]);

const FIELD_TYPES: FieldType[] = ["bool", "int", "string", "date"];

/** A field spec with only its `type` set — the minimal shape a new field
 * declares; the surgical writer omits every absent key. */
function blankSpec(type: FieldType): FieldSpec {
  return {
    type,
    default: null,
    required: false,
    values: null,
    list: null,
    settable: null,
    log_on_change: null,
  };
}

export default function ConfigStructuredView({ cfg }: { cfg: ConfigDraft }) {
  const { toast } = useToast();

  // Derive the rendered model by parsing the live DRAFT (not the applied
  // config), so multi-edit-before-save always shows what Save would write.
  // Keep the last good model on screen while a re-parse is in flight.
  const model = useQuery({
    queryKey: ["parse_config_model", cfg.draft],
    queryFn: () => parseConfigModel(cfg.draft),
    placeholderData: keepPreviousData,
  });

  // Every committed edit runs through a SERIALISED queue. Each surgical
  // command rewrites the one table it touches on the string it is handed,
  // so edits MUST apply one-at-a-time against the latest result — checkboxes
  // and the type select commit instantly, so two edits can fire inside a
  // single command's IPC round-trip. Reading `cfg.draft` at call time would
  // hand the second edit a stale base (React state lags the in-flight
  // command) and its rewrite would silently drop the first edit's table.
  // Instead the queue threads the true accumulated string through
  // `draftRef`, advanced synchronously as each command resolves, so every
  // edit builds on the previous one. External reseeds — a conflict reload
  // changing the on-disk hash, or a toggle back from Raw — remount this
  // view (the `key={hash}` on ConfigView, and the Raw/Form ternary), giving
  // a fresh `draftRef` seeded from the current draft; so the ref is never
  // stale against the shared draft while the view is mounted.
  const draftRef = useRef(cfg.draft);
  const queueRef = useRef<Promise<void>>(Promise.resolve());
  const applyEdit = (make: (content: string) => Promise<string>) => {
    queueRef.current = queueRef.current.then(async () => {
      try {
        const next = await make(draftRef.current);
        // Advance the ref BEFORE the next queued edit reads it — this is
        // what makes successive rapid edits compose instead of clobber.
        draftRef.current = next;
        cfg.setDraft(next);
      } catch (err) {
        // A failure (e.g. the draft was hand-broken into invalid TOML in
        // Raw) is a calm toast, never a crash. The ref is not advanced, so
        // the next edit retries from the last good draft.
        toast(errorMessage(err), "attention");
      }
    });
  };

  if (model.isPending) {
    return (
      <p role="status" className="text-sm text-ink-muted">
        Reading the config…
      </p>
    );
  }
  if (model.isError) {
    // The draft is not valid TOML — the form cannot render it. Point back
    // to Raw rather than guess; the raw editor's validation names the spot.
    return (
      <p role="status" className="text-sm text-attention">
        This draft is not valid TOML, so the form cannot show it — switch to Raw to
        fix it: {errorMessage(model.error)}
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <VaultMetaSection
        name={model.data.vault.name}
        maxActiveProjects={model.data.vault.max_active_projects}
      />
      <NoteTypesEditor model={model.data} applyEdit={applyEdit} />
      <SchemaFieldsEditor model={model.data} applyEdit={applyEdit} />
      <VariablesEditor model={model.data} applyEdit={applyEdit} />
    </div>
  );
}

/** The `[vault]` section — read-only (editing vault meta is out of scope
 * for the form; use Raw). */
function VaultMetaSection({
  name,
  maxActiveProjects,
}: {
  name: string;
  maxActiveProjects: number;
}) {
  return (
    <section aria-labelledby="config-vault-heading">
      <h3 id="config-vault-heading" className="text-sm font-semibold text-ink">
        Vault
      </h3>
      <dl className="mt-2 rounded-lg border border-line bg-bg-surface p-4 text-sm">
        <div className="flex gap-2">
          <dt className="text-ink-muted">Name</dt>
          <dd className="text-ink">{name}</dd>
        </div>
        <div className="mt-1 flex gap-2">
          <dt className="text-ink-muted">Max active projects</dt>
          <dd className="text-ink">{maxActiveProjects}</dd>
        </div>
      </dl>
    </section>
  );
}

type ApplyEdit = (make: (content: string) => Promise<string>) => void;

// --- Note types ---

function NoteTypesEditor({ model, applyEdit }: { model: ConfigModel; applyEdit: ApplyEdit }) {
  return (
    <section aria-labelledby="config-note-types-heading">
      <h3 id="config-note-types-heading" className="text-sm font-semibold text-ink">
        Note types
      </h3>
      {model.note_types.length === 0 ? (
        <p className="mt-2 text-sm text-ink-muted">No custom note types.</p>
      ) : (
        <ul className="mt-2 flex flex-col gap-3">
          {model.note_types.map((entry) => (
            <li key={entry.name}>
              <NoteTypeCard name={entry.name} noteType={entry.note_type} applyEdit={applyEdit} />
            </li>
          ))}
        </ul>
      )}
      <AddNoteTypeForm existing={model.note_types.map((n) => n.name)} applyEdit={applyEdit} />
    </section>
  );
}

function NoteTypeCard({
  name,
  noteType,
  applyEdit,
}: {
  name: string;
  noteType: CustomNoteType;
  applyEdit: ApplyEdit;
}) {
  // Commit a whole updated note type: the surgical writer rewrites only
  // this `[note_types.<name>]` table with the given shape.
  const set = (next: CustomNoteType) =>
    applyEdit((content) => configSetNoteType(content, name, next));

  const folderError =
    noteType.folder.trim() === ""
      ? "Folder is required."
      : RESERVED_FOLDERS.has(noteType.folder.trim().split("/")[0])
        ? "That folder is reserved by a built-in area."
        : null;

  const fieldNames = [...noteType.required, ...noteType.optional];

  return (
    <article className="rounded-lg border border-line bg-bg-surface p-4">
      <header className="flex flex-wrap items-center gap-2">
        <h4 className="text-base font-semibold text-ink">{name}</h4>
        <button
          type="button"
          onClick={() => applyEdit((content) => configRemoveNoteType(content, name))}
          className="ml-auto shrink-0 rounded border border-line px-2 py-1 text-xs text-ink-muted hover:bg-bg-sunken hover:text-ink"
        >
          Remove type
        </button>
      </header>

      <div className="mt-3 grid gap-3 sm:grid-cols-2">
        <CommitText
          label="Folder"
          ariaLabel={`Folder for ${name}`}
          value={noteType.folder}
          onCommit={(folder) => set({ ...noteType, folder })}
          error={folderError}
        />
        <CommitText
          label="Template"
          ariaLabel={`Template for ${name}`}
          value={noteType.template ?? ""}
          placeholder="(default)"
          onCommit={(template) => set({ ...noteType, template: template.trim() || null })}
        />
      </div>

      <label className="mt-3 flex items-center gap-2 text-sm text-ink">
        <input
          type="checkbox"
          checked={noteType.append_only}
          onChange={(e) => set({ ...noteType, append_only: e.target.checked })}
        />
        Append-only
      </label>

      <div className="mt-3">
        <ChipListEditor
          label="Required fields"
          items={noteType.required}
          onChange={(required) => set({ ...noteType, required })}
        />
      </div>
      <div className="mt-3">
        <ChipListEditor
          label="Optional fields"
          items={noteType.optional}
          onChange={(optional) => set({ ...noteType, optional })}
        />
      </div>

      <div className="mt-3 grid gap-3 sm:grid-cols-2">
        <FieldNameSelect
          label="Title field"
          value={noteType.title_field}
          options={fieldNames}
          onChange={(title_field) => set({ ...noteType, title_field })}
        />
        <FieldNameSelect
          label="Date field"
          value={noteType.date_field}
          options={fieldNames}
          onChange={(date_field) => set({ ...noteType, date_field })}
        />
      </div>
    </article>
  );
}

function AddNoteTypeForm({
  existing,
  applyEdit,
}: {
  existing: string[];
  applyEdit: ApplyEdit;
}) {
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");

  const trimmedName = name.trim();
  const trimmedFolder = folder.trim();
  const error =
    trimmedName === "" || trimmedFolder === ""
      ? null
      : BUILTIN_NOTE_TYPES.has(trimmedName.toLowerCase())
        ? `"${trimmedName}" is a built-in type name.`
        : existing.includes(trimmedName)
          ? `"${trimmedName}" already exists.`
          : RESERVED_FOLDERS.has(trimmedFolder.split("/")[0])
            ? `"${trimmedFolder}" is a reserved folder.`
            : null;
  const canAdd = trimmedName !== "" && trimmedFolder !== "" && error === null;

  function add() {
    if (!canAdd) return;
    const next: CustomNoteType = {
      folder: trimmedFolder,
      required: [],
      optional: [],
      template: null,
      append_only: false,
      title_field: null,
      date_field: null,
    };
    applyEdit((content) => configSetNoteType(content, trimmedName, next));
    setName("");
    setFolder("");
  }

  return (
    <form
      aria-label="Add a note type"
      className="mt-3 rounded-lg border border-dashed border-line p-4"
      onSubmit={(e) => {
        e.preventDefault();
        add();
      }}
    >
      <p className="text-xs font-medium text-ink-muted">Add a note type</p>
      <div className="mt-2 grid gap-3 sm:grid-cols-2">
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-ink-muted">Name</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="rounded border border-line bg-bg-base px-2 py-1 text-ink"
          />
        </label>
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-ink-muted">Folder</span>
          <input
            value={folder}
            onChange={(e) => setFolder(e.target.value)}
            className="rounded border border-line bg-bg-base px-2 py-1 text-ink"
          />
        </label>
      </div>
      {error !== null && (
        <p role="status" className="mt-2 text-sm text-attention">
          {error}
        </p>
      )}
      <button
        type="submit"
        disabled={!canAdd}
        className="mt-3 rounded border border-line px-3 py-1 text-xs text-ink hover:bg-bg-sunken disabled:opacity-50"
      >
        Add note type
      </button>
    </form>
  );
}

// --- Schema fields ---

function SchemaFieldsEditor({ model, applyEdit }: { model: ConfigModel; applyEdit: ApplyEdit }) {
  // Only schemas that carry typed fields get a table; a bare
  // `extra_required`-only extension has no field rows to edit here.
  const withFields = model.schemas.filter((s) => Object.keys(s.schema.fields).length > 0);
  return (
    <section aria-labelledby="config-schemas-heading">
      <h3 id="config-schemas-heading" className="text-sm font-semibold text-ink">
        Schema fields
      </h3>
      {withFields.length === 0 ? (
        <p className="mt-2 text-sm text-ink-muted">No schema field definitions.</p>
      ) : (
        <ul className="mt-2 flex flex-col gap-3">
          {withFields.map((entry) => (
            <li key={entry.name}>
              <SchemaCard entry={entry} applyEdit={applyEdit} />
            </li>
          ))}
        </ul>
      )}
      <AddSchemaFieldForm applyEdit={applyEdit} />
    </section>
  );
}

function SchemaCard({ entry, applyEdit }: { entry: NamedSchema; applyEdit: ApplyEdit }) {
  const rows = Object.entries(entry.schema.fields).sort(([a], [b]) => a.localeCompare(b));
  return (
    <article className="rounded-lg border border-line bg-bg-surface p-4">
      <h4 className="text-base font-semibold text-ink">{entry.name}</h4>
      <ul className="mt-2 flex flex-col gap-3">
        {rows.map(([fieldName, spec]) => (
          <li key={fieldName}>
            <SchemaFieldRow
              type={entry.name}
              field={fieldName}
              spec={spec}
              applyEdit={applyEdit}
            />
          </li>
        ))}
      </ul>
    </article>
  );
}

function SchemaFieldRow({
  type,
  field,
  spec,
  applyEdit,
}: {
  type: string;
  field: string;
  spec: FieldSpec;
  applyEdit: ApplyEdit;
}) {
  const set = (next: FieldSpec) =>
    applyEdit((content) => configSetSchemaField(content, type, field, next));

  // Changing the type away from `string` clears `values` (the server only
  // allows an allowed-value list on a string field).
  function changeType(nextType: FieldType) {
    set({
      ...spec,
      type: nextType,
      default: null,
      values: nextType === "string" ? spec.values : null,
    });
  }

  // Reserved keys the Phase-2 setter honours (#375, #301): `settable` opts a
  // field into `set_frontmatter` (default-deny), and `log_on_change` auto-logs
  // a change to the daily note. `log_on_change` only fires on a settable field,
  // so turning `settable` off disables and clears it — the same coupled clear
  // `changeType` does to `values`. Unchecked maps to `null` so the surgical
  // writer omits the key entirely rather than writing `= false` (an absent and
  // an explicit-false `settable` are both "not settable" to the server).
  function setSettable(on: boolean) {
    set({ ...spec, settable: on ? true : null, log_on_change: on ? spec.log_on_change : null });
  }
  function setLogOnChange(on: boolean) {
    set({ ...spec, log_on_change: on ? true : null });
  }

  return (
    <div className="rounded border border-line bg-bg-base p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-medium text-ink">{field}</span>
        <label className="ml-auto flex items-center gap-1 text-xs text-ink-muted">
          Type
          <select
            aria-label={`Type for ${field}`}
            value={spec.type}
            onChange={(e) => changeType(e.target.value as FieldType)}
            className="rounded border border-line bg-bg-base px-1 py-0.5 text-ink"
          >
            {FIELD_TYPES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          onClick={() => applyEdit((content) => configRemoveSchemaField(content, type, field))}
          className="rounded border border-line px-2 py-0.5 text-xs text-ink-muted hover:bg-bg-sunken hover:text-ink"
        >
          Remove field
        </button>
      </div>

      <div className="mt-2 grid gap-3 sm:grid-cols-2">
        <DefaultInput
          field={field}
          type={spec.type}
          value={spec.default}
          onChange={(value) => set({ ...spec, default: value })}
        />
        <label className="flex items-center gap-2 self-end text-sm text-ink">
          <input
            type="checkbox"
            checked={spec.required}
            onChange={(e) => set({ ...spec, required: e.target.checked })}
          />
          Required
        </label>
      </div>

      {spec.type === "string" && (
        <div className="mt-2">
          <ChipListEditor
            label="Allowed values"
            items={spec.values ?? []}
            onChange={(values) => set({ ...spec, values: values.length > 0 ? values : null })}
          />
        </div>
      )}

      <div className="mt-2 flex flex-wrap items-center gap-4">
        <label className="flex items-center gap-2 text-sm text-ink">
          <input
            type="checkbox"
            checked={spec.settable === true}
            onChange={(e) => setSettable(e.target.checked)}
          />
          Settable
        </label>
        <label
          className={`flex items-center gap-2 text-sm ${
            spec.settable === true ? "text-ink" : "text-ink-faint"
          }`}
        >
          <input
            type="checkbox"
            checked={spec.log_on_change === true}
            disabled={spec.settable !== true}
            title={spec.settable === true ? undefined : "Available once the field is settable"}
            onChange={(e) => setLogOnChange(e.target.checked)}
          />
          Log changes to daily
        </label>
      </div>
    </div>
  );
}

/** The typed default input, driven by the field's `type`: a checkbox-like
 * tri-state select for `bool`, a number for `int`, a date picker for
 * `date`, and text for `string`. An empty/none choice maps to `null` (no
 * default). */
function DefaultInput({
  field,
  type,
  value,
  onChange,
}: {
  field: string;
  type: FieldType;
  value: string | number | boolean | null;
  onChange: (next: string | number | boolean | null) => void;
}) {
  const label = `Default for ${field}`;
  if (type === "bool") {
    const current = value === null ? "" : value ? "true" : "false";
    return (
      <label className="flex flex-col gap-1 text-sm">
        <span className="text-ink-muted">Default</span>
        <select
          aria-label={label}
          value={current}
          onChange={(e) =>
            onChange(e.target.value === "" ? null : e.target.value === "true")
          }
          className="rounded border border-line bg-bg-base px-2 py-1 text-ink"
        >
          <option value="">(no default)</option>
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      </label>
    );
  }
  if (type === "int") {
    return (
      <CommitText
        label="Default"
        inputType="number"
        ariaLabel={label}
        value={value === null ? "" : String(value)}
        placeholder="(no default)"
        onCommit={(raw) => {
          const trimmed = raw.trim();
          if (trimmed === "") return onChange(null);
          const n = Number(trimmed);
          onChange(Number.isInteger(n) ? n : null);
        }}
      />
    );
  }
  return (
    <CommitText
      label="Default"
      inputType={type === "date" ? "date" : "text"}
      ariaLabel={label}
      value={value === null ? "" : String(value)}
      placeholder="(no default)"
      onCommit={(raw) => onChange(raw.trim() === "" ? null : raw)}
    />
  );
}

function AddSchemaFieldForm({ applyEdit }: { applyEdit: ApplyEdit }) {
  const [type, setType] = useState("");
  const [field, setField] = useState("");
  const [fieldType, setFieldType] = useState<FieldType>("string");

  const trimmedType = type.trim();
  const trimmedField = field.trim();
  const canAdd = trimmedType !== "" && trimmedField !== "";

  function add() {
    if (!canAdd) return;
    applyEdit((content) =>
      configSetSchemaField(content, trimmedType, trimmedField, blankSpec(fieldType)),
    );
    setField("");
  }

  return (
    <form
      aria-label="Add a schema field"
      className="mt-3 rounded-lg border border-dashed border-line p-4"
      onSubmit={(e) => {
        e.preventDefault();
        add();
      }}
    >
      <p className="text-xs font-medium text-ink-muted">Add a schema field</p>
      <div className="mt-2 grid gap-3 sm:grid-cols-3">
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-ink-muted">Note type</span>
          <input
            value={type}
            onChange={(e) => setType(e.target.value)}
            className="rounded border border-line bg-bg-base px-2 py-1 text-ink"
          />
        </label>
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-ink-muted">Field name</span>
          <input
            value={field}
            onChange={(e) => setField(e.target.value)}
            className="rounded border border-line bg-bg-base px-2 py-1 text-ink"
          />
        </label>
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-ink-muted">Type</span>
          <select
            aria-label="New field type"
            value={fieldType}
            onChange={(e) => setFieldType(e.target.value as FieldType)}
            className="rounded border border-line bg-bg-base px-2 py-1 text-ink"
          >
            {FIELD_TYPES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </label>
      </div>
      <button
        type="submit"
        disabled={!canAdd}
        className="mt-3 rounded border border-line px-3 py-1 text-xs text-ink hover:bg-bg-sunken disabled:opacity-50"
      >
        Add field
      </button>
    </form>
  );
}

// --- Template variables (#376) ---

/** The `[variables]` block: static variables (available in every template)
 * and prompted variables (asked for interactively). Both are simple
 * name→string maps, so one `VariableList` renders each. */
function VariablesEditor({ model, applyEdit }: { model: ConfigModel; applyEdit: ApplyEdit }) {
  return (
    <section aria-labelledby="config-variables-heading">
      <h3 id="config-variables-heading" className="text-sm font-semibold text-ink">
        Template variables
      </h3>
      <VariableList
        kind="static"
        heading="Static"
        describe="Available in every template."
        valueLabel="Value"
        vars={model.variables.static_vars}
        // `prompt` is the sub-table key, never a static variable name.
        reserved={STATIC_RESERVED_NAMES}
        onSet={(name, value) => applyEdit((content) => configSetVariable(content, name, value))}
        onRemove={(name) => applyEdit((content) => configRemoveVariable(content, name))}
      />
      <VariableList
        kind="prompted"
        heading="Prompted"
        describe="Asked for when a template uses them; the value is the prompt shown."
        valueLabel="Prompt"
        vars={model.variables.prompt}
        reserved={EMPTY_RESERVED_NAMES}
        onSet={(name, message) =>
          applyEdit((content) => configSetPromptVariable(content, name, message))
        }
        onRemove={(name) => applyEdit((content) => configRemovePromptVariable(content, name))}
      />
    </section>
  );
}

const STATIC_RESERVED_NAMES = new Set(["prompt"]);
const EMPTY_RESERVED_NAMES: Set<string> = new Set();

/** One name→string variable map, rendered as editable rows plus an add form.
 * The value commits on blur/Enter (via `CommitText`), so a per-keystroke
 * surgical write never fires. */
function VariableList({
  kind,
  heading,
  describe,
  valueLabel,
  vars,
  reserved,
  onSet,
  onRemove,
}: {
  kind: "static" | "prompted";
  heading: string;
  describe: string;
  valueLabel: string;
  vars: NamedValue[];
  reserved: Set<string>;
  onSet: (name: string, value: string) => void;
  onRemove: (name: string) => void;
}) {
  return (
    <div className="mt-3">
      <h4 className="text-xs font-medium text-ink-muted">
        {heading} <span className="font-normal">— {describe}</span>
      </h4>
      {vars.length === 0 ? (
        <p className="mt-1 text-sm text-ink-muted">None.</p>
      ) : (
        <ul className="mt-1 flex flex-col gap-2">
          {vars.map((v) => (
            <li key={v.name} className="flex items-end gap-2">
              <div className="flex-1">
                <CommitText
                  label={v.name}
                  ariaLabel={`${valueLabel} for ${v.name}`}
                  value={v.value}
                  onCommit={(value) => onSet(v.name, value)}
                />
              </div>
              <button
                type="button"
                // Scope the name by kind: a variable of the same name can exist
                // in both the static and prompted lists, so a bare
                // `Remove ${name}` would be an ambiguous accessible name.
                aria-label={`Remove ${kind} variable ${v.name}`}
                onClick={() => onRemove(v.name)}
                className="rounded border border-line px-2 py-1 text-xs text-ink-muted hover:bg-bg-sunken hover:text-ink"
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
      <AddVariableForm
        kind={kind}
        valueLabel={valueLabel}
        existing={vars.map((v) => v.name)}
        reserved={reserved}
        onAdd={onSet}
      />
    </div>
  );
}

function AddVariableForm({
  kind,
  valueLabel,
  existing,
  reserved,
  onAdd,
}: {
  kind: "static" | "prompted";
  valueLabel: string;
  existing: string[];
  reserved: Set<string>;
  onAdd: (name: string, value: string) => void;
}) {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");

  const trimmedName = name.trim();
  const error =
    trimmedName === ""
      ? null
      : reserved.has(trimmedName)
        ? `"${trimmedName}" is a reserved name.`
        : existing.includes(trimmedName)
          ? `"${trimmedName}" already exists.`
          : null;
  const canAdd = trimmedName !== "" && error === null;

  function add() {
    if (!canAdd) return;
    onAdd(trimmedName, value);
    setName("");
    setValue("");
  }

  return (
    <form
      aria-label={`Add a ${kind} variable`}
      className="mt-2 rounded-lg border border-dashed border-line p-3"
      onSubmit={(e) => {
        e.preventDefault();
        add();
      }}
    >
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-ink-muted">Name</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="rounded border border-line bg-bg-base px-2 py-1 text-ink"
          />
        </label>
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-ink-muted">{valueLabel}</span>
          <input
            value={value}
            onChange={(e) => setValue(e.target.value)}
            className="rounded border border-line bg-bg-base px-2 py-1 text-ink"
          />
        </label>
      </div>
      {error !== null && (
        <p role="status" className="mt-2 text-sm text-attention">
          {error}
        </p>
      )}
      <button
        type="submit"
        disabled={!canAdd}
        className="mt-3 rounded border border-line px-3 py-1 text-xs text-ink hover:bg-bg-sunken disabled:opacity-50"
      >
        Add variable
      </button>
    </form>
  );
}

// --- Shared inputs ---

/** A text/number/date input that holds its own draft while the user types
 * and commits on blur or Enter — so a per-keystroke surgical write never
 * fires, only a settled value does. */
function CommitText({
  label,
  ariaLabel,
  value,
  placeholder,
  inputType = "text",
  error,
  onCommit,
}: {
  label: string;
  ariaLabel?: string;
  value: string;
  placeholder?: string;
  inputType?: "text" | "number" | "date";
  error?: string | null;
  onCommit: (value: string) => void;
}) {
  const [local, setLocal] = useState(value);
  // Re-seed when the committed value changes underneath (another edit
  // rewrote the draft, so the parsed model handed us a new value). Keyed
  // on `value` alone, so a keystroke — which only moves `local` — never
  // resets what the user is typing.
  useEffect(() => setLocal(value), [value]);

  function commit() {
    if (local !== value) onCommit(local);
  }

  return (
    <label className="flex flex-col gap-1 text-sm">
      <span className="text-ink-muted">{label}</span>
      <input
        type={inputType}
        aria-label={ariaLabel ?? label}
        value={local}
        placeholder={placeholder}
        onChange={(e) => setLocal(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            commit();
          }
        }}
        className="rounded border border-line bg-bg-base px-2 py-1 text-ink"
      />
      {error != null && (
        <span role="status" className="text-xs text-attention">
          {error}
        </span>
      )}
    </label>
  );
}

/** A tag-list editor: existing items as removable chips plus an input to
 * add one. Commits the whole new array on every add/remove. */
function ChipListEditor({
  label,
  items,
  onChange,
}: {
  label: string;
  items: string[];
  onChange: (items: string[]) => void;
}) {
  const [draft, setDraft] = useState("");

  function add() {
    const value = draft.trim();
    if (value === "" || items.includes(value)) {
      setDraft("");
      return;
    }
    onChange([...items, value]);
    setDraft("");
  }

  return (
    <div>
      <span className="text-xs text-ink-muted">{label}</span>
      <ul className="mt-1 flex flex-wrap items-center gap-1">
        {items.map((item) => (
          <li key={item}>
            <span className="inline-flex items-center gap-1 rounded bg-bg-sunken px-2 py-0.5 text-xs text-ink-muted">
              {item}
              <button
                type="button"
                aria-label={`Remove ${item} from ${label}`}
                onClick={() => onChange(items.filter((i) => i !== item))}
                className="text-ink-faint hover:text-ink"
              >
                ×
              </button>
            </span>
          </li>
        ))}
        <li>
          <input
            aria-label={`Add to ${label}`}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={add}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                add();
              }
            }}
            className="w-24 rounded border border-line bg-bg-base px-2 py-0.5 text-xs text-ink"
            placeholder="add…"
          />
        </li>
      </ul>
    </div>
  );
}

/** A select over declared field names (required ∪ optional) for
 * `title_field`/`date_field`, plus a "(none)" option that maps to `null`.
 * Disabled when the type declares no fields yet. */
function FieldNameSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string | null;
  options: string[];
  onChange: (value: string | null) => void;
}) {
  return (
    <label className="flex flex-col gap-1 text-sm">
      <span className="text-ink-muted">{label}</span>
      <select
        aria-label={label}
        value={value ?? ""}
        disabled={options.length === 0}
        onChange={(e) => onChange(e.target.value === "" ? null : e.target.value)}
        className="rounded border border-line bg-bg-base px-2 py-1 text-ink disabled:opacity-50"
      >
        <option value="">(none)</option>
        {options.map((name) => (
          <option key={name} value={name}>
            {name}
          </option>
        ))}
      </select>
    </label>
  );
}
