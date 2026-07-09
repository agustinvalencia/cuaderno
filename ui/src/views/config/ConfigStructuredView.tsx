// Structured Config view (#365, PR5a) — a READ-ONLY rendering of the
// parsed config: the vault meta, each custom note type as a card, and
// each schema's field declarations as a small table. No edit affordances
// in 5a; PR5b makes the same shape editable on top of the shared
// useConfigDraft model. Calm throughout — cards, chips, muted tables, no
// red token (a config field is data to read, not an error to flag).
import { useQuery } from "@tanstack/react-query";
import type { ConfigModel } from "../../api/bindings/ConfigModel";
import type { CustomNoteType } from "../../api/bindings/CustomNoteType";
import type { FieldSpec } from "../../api/bindings/FieldSpec";
import type { NamedSchema } from "../../api/bindings/NamedSchema";
import { errorMessage, readConfigModel } from "../../api/commands";

export default function ConfigStructuredView() {
  const model = useQuery({ queryKey: ["read_config_model"], queryFn: readConfigModel });

  if (model.isPending) {
    return (
      <p role="status" className="text-sm text-ink-muted">
        Reading the config…
      </p>
    );
  }
  if (model.isError) {
    return (
      <p role="status" className="text-sm text-ink-muted">
        The config could not be read: {errorMessage(model.error)}
      </p>
    );
  }

  return <StructuredBody model={model.data} />;
}

function StructuredBody({ model }: { model: ConfigModel }) {
  return (
    <div className="flex flex-col gap-6">
      <VaultMetaSection
        name={model.vault.name}
        maxActiveProjects={model.vault.max_active_projects}
      />
      <NoteTypesSection model={model} />
      <SchemasSection schemas={model.schemas} />
    </div>
  );
}

/** The `[vault]` section — name and the active-project cap. */
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

/** The `[note_types.*]` table — one card per custom type, or a calm empty
 * state when none are declared. */
function NoteTypesSection({ model }: { model: ConfigModel }) {
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
              <NoteTypeCard name={entry.name} noteType={entry.note_type} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function NoteTypeCard({ name, noteType }: { name: string; noteType: CustomNoteType }) {
  return (
    <article className="rounded-lg border border-line bg-bg-surface p-4">
      <header className="flex flex-wrap items-center gap-2">
        <h4 className="text-base font-semibold text-ink">{name}</h4>
        {noteType.append_only && <Chip>append-only</Chip>}
      </header>

      <dl className="mt-2 text-sm">
        <div className="flex gap-2">
          <dt className="text-ink-muted">Folder</dt>
          <dd className="text-ink">
            <code className="text-ink-faint">{noteType.folder}</code>
          </dd>
        </div>
        {noteType.template !== null && (
          <div className="mt-1 flex gap-2">
            <dt className="text-ink-muted">Template</dt>
            <dd className="text-ink">
              <code className="text-ink-faint">{noteType.template}</code>
            </dd>
          </div>
        )}
      </dl>

      <FieldChips label="Required" names={noteType.required} />
      <FieldChips label="Optional" names={noteType.optional} />
    </article>
  );
}

/** A labelled row of field-name chips, omitted entirely when the list is
 * empty (an absent required/optional set is not worth a row). */
function FieldChips({ label, names }: { label: string; names: string[] }) {
  if (names.length === 0) return null;
  return (
    <div className="mt-2">
      <span className="text-xs text-ink-muted">{label}</span>
      <ul className="mt-1 flex flex-wrap gap-1">
        {names.map((fieldName) => (
          <li key={fieldName}>
            <Chip>{fieldName}</Chip>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** The `[schemas.*]` tables — per note type, its declared fields laid out
 * as a small table, or a calm empty state when none are declared. */
function SchemasSection({ schemas }: { schemas: NamedSchema[] }) {
  // Only schemas carrying at least one typed field are worth a table; a
  // bare `extra_required`-only extension has no field rows to show here.
  const withFields = schemas.filter((s) => Object.keys(s.schema.fields).length > 0);
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
              <SchemaCard name={entry.name} fields={entry.schema.fields} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function SchemaCard({
  name,
  fields,
}: {
  name: string;
  fields: { [key: string]: FieldSpec };
}) {
  // Sort field rows by name so the table order is stable (object key order
  // from a serialised Record is unspecified).
  const rows = Object.entries(fields).sort(([a], [b]) => a.localeCompare(b));
  return (
    <article className="rounded-lg border border-line bg-bg-surface p-4">
      <h4 className="text-base font-semibold text-ink">{name}</h4>
      <div className="mt-2 overflow-x-auto">
        <table className="w-full text-left text-sm">
          <thead>
            <tr className="text-xs text-ink-muted">
              <th className="pr-4 pb-1 font-medium">Field</th>
              <th className="pr-4 pb-1 font-medium">Type</th>
              <th className="pr-4 pb-1 font-medium">Default</th>
              <th className="pr-4 pb-1 font-medium">Required</th>
              <th className="pb-1 font-medium">Values</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(([fieldName, spec]) => (
              <tr key={fieldName} className="border-t border-line">
                <td className="py-1 pr-4 text-ink">{fieldName}</td>
                <td className="py-1 pr-4 text-ink-muted">{spec.type}</td>
                <td className="py-1 pr-4 text-ink-muted">
                  {spec.default === null ? "—" : String(spec.default)}
                </td>
                <td className="py-1 pr-4 text-ink-muted">{spec.required ? "yes" : "no"}</td>
                <td className="py-1 text-ink-muted">
                  {spec.values === null ? "—" : spec.values.join(", ")}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </article>
  );
}

/** A calm, muted chip — a folder/field tag or an append-only marker.
 * Deliberately not the attention tier: config data is read, not flagged. */
function Chip({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded bg-bg-sunken px-2 py-0.5 text-xs text-ink-muted">{children}</span>
  );
}
