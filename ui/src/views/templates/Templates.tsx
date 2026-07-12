// Templates view (#357) — browse every note type's template, read its
// effective content, edit-and-save a custom override, and scaffold a
// starter for a config-defined custom type that has none.
//
// The edit-and-save model: editing a type currently backed by the
// built-in default and saving transparently writes a custom override
// (no separate "eject" step). This is the app's first free-text file
// editor — a plain <textarea> writing the whole file verbatim.
//
// Validation is calm and non-blocking: the editor flags any {{unknown}}
// token not in the type's placeholder set with an ink/attention notice,
// but saving is never blocked. The known set is computed entirely by the
// backend (list_template_placeholders) — including a config custom type's
// declared schema fields — so the frontend never re-derives domain data.
import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { PlaceholderSource } from "../../api/bindings/PlaceholderSource";
import type { TemplatePlaceholder } from "../../api/bindings/TemplatePlaceholder";
import type { TemplateSummary } from "../../api/bindings/TemplateSummary";
import {
  createTemplate,
  errorMessage,
  listTemplatePlaceholders,
  listTemplates,
  openInEditor,
  readTemplate,
  saveTemplate,
} from "../../api/commands";
import { SectionHeading } from "../../components/ui/section-heading";
import { useToast } from "../../shell/Toasts";

/** The distinct `{{placeholder}}` names referenced in `content`, in
 * first-appearance order. Tokenises exactly like the backend's
 * `placeholder_names` (Rust `cdno-core`): names are trimmed, and a `{{`
 * with no closing `}}` is skipped rather than treated as a token — so
 * "what the editor thinks is a placeholder" matches what render will
 * substitute, and the unknown-token check can't disagree with the
 * backend. */
export function templateTokens(content: string): string[] {
  const names: string[] = [];
  let rest = content;
  for (;;) {
    const start = rest.indexOf("{{");
    if (start === -1) break;
    const afterOpen = rest.slice(start + 2);
    const end = afterOpen.indexOf("}}");
    if (end === -1) {
      // No closing `}}` — mirror render and advance past the `{{`.
      rest = afterOpen;
      continue;
    }
    const name = afterOpen.slice(0, end).trim();
    if (name.length > 0 && !names.includes(name)) names.push(name);
    rest = afterOpen.slice(end + 2);
  }
  return names;
}

/** The badge label + tooltip for a template's source. Custom overrides
 * read as "Custom"; a built-in default as "Built-in"; a custom type with
 * no template as "No template". */
function sourceBadge(summary: TemplateSummary): { label: string; title: string } {
  if (summary.source === null) {
    return { label: "No template", title: "This custom type has no template yet" };
  }
  if (summary.source === "custom_base" || summary.source === "custom_variant") {
    return { label: "Custom", title: "A custom override in .cuaderno/templates/" };
  }
  return { label: "Built-in", title: "The built-in default (no override yet)" };
}

/** The display heading for a placeholder group. */
const SOURCE_GROUPS: { kind: PlaceholderSource["kind"]; heading: string; note: string }[] = [
  { kind: "supplied", heading: "Supplied", note: "Filled automatically when a note is created" },
  { kind: "schema", heading: "Schema fields", note: "This type's declared fields, filled from frontmatter" },
  { kind: "config", heading: "Config variables", note: "From [variables] in config.toml" },
  { kind: "prompt", heading: "Prompted", note: "Asked for at creation time" },
];

export default function Templates() {
  const list = useQuery({ queryKey: ["list_templates"], queryFn: listTemplates });

  if (list.isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (list.isError) {
    return (
      <div className="p-8">
        <p className="text-ink">Templates could not be opened.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(list.error)}</p>
      </div>
    );
  }
  return <TemplatesBody templates={list.data} />;
}

function TemplatesBody({ templates }: { templates: TemplateSummary[] }) {
  const [selected, setSelected] = useState<string>(templates[0]?.note_type ?? "");
  const current = templates.find((t) => t.note_type === selected) ?? templates[0];

  return (
    <div className="mx-auto max-w-5xl p-8">
      <h1 className="text-xl font-semibold text-ink">Templates</h1>
      <p className="mt-2 text-sm text-ink-muted">
        Each note type's template. Edit one to save a custom override in{" "}
        <code className="text-ink-faint">.cuaderno/templates/</code>.
      </p>

      <div className="mt-6 grid grid-cols-1 gap-8 md:grid-cols-[16rem_1fr]">
        {/* Left: the note-type list with a source badge each. */}
        <nav aria-label="Note types" className="min-w-0">
          <ul className="space-y-1">
            {templates.map((t) => {
              const badge = sourceBadge(t);
              const isSelected = t.note_type === current?.note_type;
              return (
                <li key={t.note_type}>
                  <button
                    type="button"
                    aria-current={isSelected ? "true" : undefined}
                    onClick={() => setSelected(t.note_type)}
                    className={`flex w-full items-center justify-between gap-2 rounded border px-3 py-2 text-left text-sm ${
                      isSelected
                        ? "border-line bg-bg-sunken font-medium text-ink"
                        : "border-transparent text-ink-muted hover:bg-bg-sunken hover:text-ink"
                    }`}
                  >
                    <span className="min-w-0 truncate">{t.display_name}</span>
                    <span
                      title={badge.title}
                      className="shrink-0 rounded border border-line px-1.5 py-0.5 text-xs text-ink-faint"
                    >
                      {badge.label}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>

        {/* Right: the editor, or the Create affordance for a template-less
            custom type. Keyed by the selection so the editor's draft state
            resets cleanly when the type changes. */}
        <section aria-label="Template" className="min-w-0">
          {current === undefined ? (
            <p className="text-sm text-ink-muted">No note types.</p>
          ) : current.is_custom_type && !current.has_custom_file ? (
            <CreatePanel key={current.note_type} summary={current} />
          ) : (
            <TemplateEditor key={current.note_type} summary={current} />
          )}
        </section>
      </div>
    </div>
  );
}

/** The Create affordance for a config-defined custom type with no
 * template yet: a calm invitation that scaffolds a starter on click. */
function CreatePanel({ summary }: { summary: TemplateSummary }) {
  const client = useQueryClient();
  const { toast } = useToast();

  const create = useMutation({
    mutationFn: () => createTemplate(summary.note_type),
    onSuccess: () => {
      toast("Template created.");
      // The list's source badge flips and the editor's read now has a
      // file — refresh both.
      void client.invalidateQueries({ queryKey: ["list_templates"] });
      void client.invalidateQueries({ queryKey: ["read_template", summary.note_type] });
    },
    onError: (err) => toast(errorMessage(err), "attention"),
  });

  return (
    <div className="rounded-lg border border-line bg-bg-surface p-6">
      <h2 className="text-base font-semibold text-ink">{summary.display_name}</h2>
      <p className="mt-2 text-sm text-ink-muted">
        This custom type has no template yet. Create a starter with its
        declared fields as placeholders, then edit it here.
      </p>
      <button
        type="button"
        onClick={() => create.mutate()}
        disabled={create.isPending}
        className="mt-4 rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken disabled:opacity-50"
      >
        Create template
      </button>
      <p className="mt-3 text-xs text-ink-faint">
        Will write{" "}
        <button
          type="button"
          onClick={() => void openInEditor(summary.path)}
          className="underline decoration-dotted underline-offset-2 hover:text-ink"
        >
          {summary.path}
        </button>
      </p>
    </div>
  );
}

/** The editor: the effective content in a <textarea>, a Save action, a
 * calm unknown-token notice, and a placeholder-reference panel. */
function TemplateEditor({ summary }: { summary: TemplateSummary }) {
  const noteType = summary.note_type;
  const client = useQueryClient();
  const { toast } = useToast();

  const read = useQuery({
    queryKey: ["read_template", noteType],
    queryFn: () => readTemplate(noteType),
  });
  const placeholders = useQuery({
    queryKey: ["list_template_placeholders", noteType],
    queryFn: () => listTemplatePlaceholders(noteType),
  });

  // `baseline` is the last-loaded (or last-saved) content; `draft` is what
  // the textarea holds. Dirty = they differ.
  const [baseline, setBaseline] = useState<string | null>(null);
  const [draft, setDraft] = useState<string>("");

  // Adopt the effective content when it first arrives, and re-adopt an
  // external change — but only while there are no unsaved local edits, so
  // a background refetch never clobbers a mid-edit draft.
  useEffect(() => {
    if (read.data === undefined) return;
    const content = read.data.content;
    const clean = baseline === null || draft === baseline;
    if (content !== baseline && clean) {
      setBaseline(content);
      setDraft(content);
    }
  }, [read.data, baseline, draft]);

  const save = useMutation({
    mutationFn: (content: string) => saveTemplate(noteType, content),
    onSuccess: (_result, content) => {
      // The saved content is the new baseline, so the editor is clean
      // again and a refetch of the same content is a no-op.
      setBaseline(content);
      toast("Saved.");
      void client.invalidateQueries({ queryKey: ["list_templates"] });
      void client.invalidateQueries({ queryKey: ["read_template", noteType] });
    },
    onError: (err) => toast(errorMessage(err), "attention"),
  });

  if (read.isPending) {
    return <p className="text-sm text-ink-muted">Reading…</p>;
  }
  if (read.isError) {
    return <p className="text-sm text-ink-muted">{String(read.error)}</p>;
  }

  const dirty = baseline !== null && draft !== baseline;
  const isBuiltinDefault = summary.source === "builtin_default" || summary.source === "builtin_variant";

  // Unknown tokens: `{{...}}` names not in the backend-supplied set. Held
  // back until the placeholder set has loaded, so the editor never
  // false-warns against an empty known set.
  const known = new Set((placeholders.data ?? []).map((p) => p.name));
  const unknown =
    placeholders.data === undefined
      ? []
      : templateTokens(draft).filter((name) => !known.has(name));

  return (
    <div className="rounded-lg border border-line bg-bg-surface">
      <header className="flex flex-wrap items-center gap-2 border-b border-line px-4 py-3">
        <h2 className="min-w-0 flex-1 truncate text-base font-semibold text-ink">
          {summary.display_name}
        </h2>
        <button
          type="button"
          onClick={() => void openInEditor(summary.path)}
          className="shrink-0 rounded border border-line px-2 py-1 text-xs text-ink hover:bg-bg-sunken"
        >
          Open in editor
        </button>
        <button
          type="button"
          onClick={() => save.mutate(draft)}
          disabled={!dirty || save.isPending}
          className="shrink-0 rounded border border-line px-3 py-1 text-xs text-ink hover:bg-bg-sunken disabled:opacity-50"
        >
          Save
        </button>
      </header>

      <div className="px-4 py-3">
        {isBuiltinDefault && (
          <p className="mb-2 text-xs text-ink-faint">
            Saving creates a custom override of the built-in default.
          </p>
        )}

        <label htmlFor="template-editor" className="sr-only">
          {summary.display_name} template content
        </label>
        <textarea
          id="template-editor"
          value={draft}
          spellCheck={false}
          onChange={(event) => setDraft(event.target.value)}
          className="h-96 w-full resize-y rounded border border-line bg-bg-base p-3 font-mono text-sm text-ink"
        />

        {/* Calm, non-blocking unknown-token notice — an ink/attention
            treatment (no red), saving is never blocked. */}
        {unknown.length > 0 && (
          <p role="status" className="mt-2 text-xs text-attention">
            Unrecognised placeholder{unknown.length > 1 ? "s" : ""}:{" "}
            {unknown.map((name) => `{{${name}}}`).join(", ")}. These will render
            literally — check the reference below. You can still save.
          </p>
        )}

        <PlaceholderPanel placeholders={placeholders.data} />
      </div>
    </div>
  );
}

/** The placeholder-reference panel, grouped by source so the author can
 * see which names a template of this type may use. */
function PlaceholderPanel({ placeholders }: { placeholders: TemplatePlaceholder[] | undefined }) {
  if (placeholders === undefined) return null;

  return (
    <section aria-label="Available placeholders" className="mt-6 border-t border-line pt-4">
      <SectionHeading as="h3">
        Available placeholders
      </SectionHeading>
      <div className="mt-3 space-y-4">
        {SOURCE_GROUPS.map(({ kind, heading, note }) => {
          const group = placeholders.filter((p) => p.source.kind === kind);
          if (group.length === 0) return null;
          return (
            <div key={kind}>
              <p className="text-xs font-medium text-ink-muted">{heading}</p>
              <p className="text-xs text-ink-faint">{note}</p>
              <ul className="mt-1 flex flex-wrap gap-1.5">
                {group.map((p) => (
                  <li
                    key={p.name}
                    title={p.source.kind === "prompt" ? p.source.data.message : undefined}
                    className="rounded border border-line px-1.5 py-0.5 font-mono text-xs text-ink-muted"
                  >
                    {`{{${p.name}}}`}
                  </li>
                ))}
              </ul>
            </div>
          );
        })}
      </div>
    </section>
  );
}
