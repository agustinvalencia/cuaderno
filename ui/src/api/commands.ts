// Typed wrappers over Tauri invoke — one function per backend
// command. Components never call invoke() directly; this module is
// the single seam the tests mock.
import { invoke } from "@tauri-apps/api/core";
import type { CmdError } from "./bindings/CmdError";
import type { CommitmentsView } from "./bindings/CommitmentsView";
import type { ConfigDocument } from "./bindings/ConfigDocument";
import type { ConfigModel } from "./bindings/ConfigModel";
import type { ConfigSaveError } from "./bindings/ConfigSaveError";
import type { CustomNoteType } from "./bindings/CustomNoteType";
import type { FieldSpec } from "./bindings/FieldSpec";
import type { ConfigValidationError } from "./bindings/ConfigValidationError";
import type { DailyView } from "./bindings/DailyView";
import type { EnergyLevel } from "./bindings/EnergyLevel";
import type { MonthlyView } from "./bindings/MonthlyView";
import type { IndexExclusions } from "./bindings/IndexExclusions";
import type { InboxItem } from "./bindings/InboxItem";
import type { NoteView } from "./bindings/NoteView";
import type { NowView } from "./bindings/NowView";
import type { OrientationView } from "./bindings/OrientationView";
import type { PortfolioDetail } from "./bindings/PortfolioDetail";
import type { PortfolioSummary } from "./bindings/PortfolioSummary";
import type { QuestionStatus } from "./bindings/QuestionStatus";
import type { QuestionStrategicRow } from "./bindings/QuestionStrategicRow";
import type { ProjectActions } from "./bindings/ProjectActions";
import type { ProjectDetail } from "./bindings/ProjectDetail";
import type { ResolvedLink } from "./bindings/ResolvedLink";
import type { SearchResultEntry } from "./bindings/SearchResultEntry";
import type { StewardshipDetail } from "./bindings/StewardshipDetail";
import type { StewardshipSummary } from "./bindings/StewardshipSummary";
import type { StrategicBundle } from "./bindings/StrategicBundle";
import type { TemplateContent } from "./bindings/TemplateContent";
import type { TemplateField } from "./bindings/TemplateField";
import type { TemplatePlaceholder } from "./bindings/TemplatePlaceholder";
import type { TemplateSummary } from "./bindings/TemplateSummary";
import type { WeeklyBundle } from "./bindings/WeeklyBundle";
import type { WeeklyView } from "./bindings/WeeklyView";

export class CuadernoError extends Error {
  readonly payload: CmdError;

  constructor(payload: CmdError) {
    super(typeof payload.data === "string" ? payload.data : payload.kind);
    this.name = "CuadernoError";
    this.payload = payload;
  }
}

/** Every question with its backlinks, whatever its status (#443). */
export function listQuestions(): Promise<QuestionStrategicRow[]> {
  return invoke<QuestionStrategicRow[]>("list_questions");
}

/** Move a question between active / parked / answered / retired (#443). */
export function setQuestionStatus(slug: string, status: QuestionStatus): Promise<void> {
  return invoke<void>("set_question_status", { slug, status });
}

/** What you are in the middle of, per today's log (#442). `null` when
 * nothing is open. */
export function getNow(): Promise<NowView | null> {
  return invoke<NowView | null>("get_now");
}

/** What the startup reconciliation left out of the index (#440). A file
 * absent from the index is absent from search, lint and backlinks too, so
 * the counts are surfaced rather than logged and forgotten. */
export function getIndexExclusions(): Promise<IndexExclusions> {
  return invoke<IndexExclusions>("get_index_exclusions");
}

/**
 * Human toast copy for a caught mutation error. An ambiguous match is
 * the one case worth expanding — echoing the query and the candidate
 * bullets turns a dead-end into an actionable "be more specific". Every
 * other CuadernoError already carries a user-facing message, and a
 * plain Error its `.message`; anything else falls back to `String`.
 */
export function errorMessage(error: unknown): string {
  if (error instanceof CuadernoError && error.payload.kind === "ambiguous") {
    const { query, candidates } = error.payload.data;
    const list = candidates.join(", ") || "no candidates";
    return `Ambiguous match for "${query}": ${list} — be more specific`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (raw) {
    if (raw && typeof raw === "object" && "kind" in raw) {
      throw new CuadernoError(raw as CmdError);
    }
    throw raw;
  }
}

export function getOrientation(): Promise<OrientationView> {
  return call("get_orientation");
}

export function getToday(): Promise<string> {
  return call("get_today");
}

export function startAction(project: string, action: string): Promise<void> {
  return call("start_action", { project, action });
}

export function completeAction(project: string, action: string): Promise<void> {
  return call("complete_action", { project, action });
}

export function updateProjectState(project: string, newState: string): Promise<string[]> {
  // Rust `new_state` is `newState` on the wire (Tauri camelCases
  // command args) — pinned by the backend IPC round-trip test.
  // Resolves to any soft length advisories (state_overflow = "warn");
  // empty on the default reject path and whenever the state fits.
  return call("update_project_state", { project, newState });
}

/**
 * Every dated commitment from today through `lookaheadDays` out (90 by
 * default), aggregated and sorted chronologically. Backs the
 * Commitments Timeline. Rust `lookahead_days` is `lookaheadDays` on the
 * wire (Tauri camelCases args) — pinned by the backend IPC round-trip.
 */
export function getCommitments(lookaheadDays = 90): Promise<CommitmentsView> {
  return call("get_commitments", { lookaheadDays });
}

/** Complete a standalone commitment (moves it to `commitments/_done/`). */
export function completeCommitment(slug: string): Promise<void> {
  return call("complete_commitment", { slug });
}

/** Tick an open milestone on `project` to done. */
export function completeMilestone(project: string, milestone: string): Promise<void> {
  return call("complete_milestone", { project, milestone });
}

/** Capture a thought into `inbox/` — the capture window's Enter verb. */
export function captureQuick(text: string): Promise<void> {
  return call("capture_quick", { text });
}

/** Append a thought to today's daily log — the capture window's Cmd+Enter verb. */
export function logQuick(text: string): Promise<void> {
  return call("log_quick", { text });
}

/** Every uncategorised inbox capture, oldest first. Backs the inbox drawer. */
export function listInbox(): Promise<InboxItem[]> {
  return call("list_inbox");
}

/** Hard-delete the inbox capture identified by `slug`. */
export function discardInboxItem(slug: string): Promise<void> {
  return call("discard_inbox_item", { slug });
}

/** Open a vault-relative note path in the user's default editor. */
export function openInEditor(path: string): Promise<void> {
  return call("open_in_editor", { path });
}

/** Open an external link from note content in the user's default browser
 * (or mail client, for `mailto:`). The backend validates the scheme —
 * only `http`/`https`/`mailto` are opened. */
export function openExternalUrl(url: string): Promise<void> {
  return call("open_external_url", { url });
}

/** Read `.cuaderno/custom.css` (the user override stylesheet) — its text,
 * or an empty string when the file doesn't exist yet. */
export function readCustomCss(): Promise<string> {
  return call("read_custom_css");
}

/** Ensure `.cuaderno/custom.css` exists (seeding a documented template the
 * first time) and open it in the user's default editor. */
export function openCustomCss(): Promise<void> {
  return call("open_custom_css");
}

/** Ensure `.cuaderno/custom.css` exists (seeding the template the first
 * time) and return its contents — the in-app editor's loader. */
export function initCustomCss(): Promise<string> {
  return call("init_custom_css");
}

/** Overwrite `.cuaderno/custom.css` with `content` — the in-app editor's save. */
export function writeCustomCss(content: string): Promise<void> {
  return call("write_custom_css", { content });
}

// --- M5: note reader, project detail, actions list, command palette ---

/** Read any vault note for the slide-in reader: parsed frontmatter,
 * markdown body, note type, and title. */
export function readNote(path: string): Promise<NoteView> {
  return call("read_note", { path });
}

/** Read a note's RAW markdown (frontmatter and all) for the editor. */
export function readNoteRaw(path: string): Promise<string> {
  return call("read_note_raw", { path });
}

/** Overwrite a note with `content` (free editing) and reindex. */
export function writeNoteRaw(path: string, content: string): Promise<void> {
  return call("write_note_raw", { path, content });
}

/** Read an image embedded in a note (`src` relative to `notePath`) as a
 * `data:` URI the reader can render inline. */
export function readNoteAsset(notePath: string, src: string): Promise<string> {
  return call("read_note_asset", { notePath, src });
}

/** Resolve a clicked wikilink `target` to its note (path + note_type)
 * for typed navigation. `null` when the target matches no note or is
 * ambiguous — the caller renders that as a muted, un-clickable span. */
export function resolveWikilink(target: string): Promise<ResolvedLink | null> {
  return call("resolve_wikilink", { target });
}

/** The composed Project Detail bundle behind `/projects/:slug`. */
export function getProject(slug: string): Promise<ProjectDetail> {
  return call("get_project", { slug });
}

/** Every active project's open actions — the cross-project Actions
 * list. */
export function listAllActions(): Promise<ProjectActions[]> {
  return call("list_all_actions");
}

/** Full-text vault search feeding the command palette's result list. A
 * blank/term-less query comes back empty rather than erroring. */
export function searchVault(query: string): Promise<SearchResultEntry[]> {
  return call("search_vault", { query });
}

/** Add a next-action bullet to a project's `## Next Actions`. */
export function addAction(
  project: string,
  action: string,
  energy: EnergyLevel,
): Promise<void> {
  return call("add_action", { project, action, energy });
}

/** Promote an open action bullet to a manifest action note. */
export function promoteAction(project: string, action: string): Promise<void> {
  return call("promote_action", { project, action });
}

/** Record a new Waiting-On blocker on a project ("I'm now blocked on X"). */
export function addWaitingOn(project: string, item: string): Promise<void> {
  return call("add_waiting_on", { project, item });
}

/** Resolve (remove) a Waiting-On blocker matching `query`. Ambiguity
 * comes back as a `CuadernoError` the caller toasts with candidates. */
export function resolveWaiting(project: string, query: string): Promise<void> {
  return call("resolve_waiting", { project, query });
}

/** Park an active project (moves its map to `projects/_parked/`). */
export function parkProject(slug: string): Promise<void> {
  return call("park_project", { slug });
}

/** Activate a parked project. At the active cap this fails with the
 * structured `ProjectCapReached` `CuadernoError`. */
export function activateProject(slug: string): Promise<void> {
  return call("activate_project", { slug });
}

// --- M6: Weekly Review ---

/** The composed Weekly Review bundle behind `/weekly`. `weekOf` is an
 * optional ISO date naming any day in the week to review; omitted, it
 * reviews the current week. Rust `week_of` is `weekOf` on the wire
 * (Tauri camelCases args) — pinned by the backend IPC round-trip. */
export function getWeeklyBundle(weekOf?: string): Promise<WeeklyBundle> {
  return call("get_weekly_bundle", { weekOf: weekOf ?? null });
}

/** Write one section of the week's note (compose/overwrite). `section`
 * is the kebab wire string: "wins" | "challenges" | "one-improvement" |
 * "this-weeks-goal". */
export function saveWeeklySection(
  section: string,
  content: string,
  weekOf?: string,
): Promise<void> {
  return call("save_weekly_section", { weekOf: weekOf ?? null, section, content });
}

// --- M7: Stewardship views ---

/** Every indexed stewardship with its staleness line, sorted by slug.
 * Backs the `/stewardships` list. */
export function listStewardships(): Promise<StewardshipSummary[]> {
  return call("list_stewardships");
}

/** The composed Stewardship Detail bundle behind `/stewardships/:slug`:
 * dashboard body, trend series (empty for a flat stewardship), the
 * last-few tracking entries, and the total tracking count. */
export function getStewardshipDetail(slug: string): Promise<StewardshipDetail> {
  return call("get_stewardship_detail", { slug });
}

/** The prompted fields the tracking log form should render for
 * `activity` — the resolved `tracking-<activity>` template's
 * `[variables.prompt]` names (empty for the generic template). */
export function getTrackingTemplateFields(activity: string): Promise<TemplateField[]> {
  return call("get_tracking_template_fields", { activity });
}

/** File one tracking note under an expanded stewardship. `vars` carries
 * the prompted-field values gathered from `getTrackingTemplateFields`.
 * A flat stewardship or a same-day duplicate comes back as a
 * `CuadernoError` the caller toasts. */
export function logTrackingEntry(
  stewardship: string,
  activity: string,
  content: string,
  vars: Record<string, string>,
  routine?: string,
): Promise<void> {
  return call("log_tracking_entry", {
    stewardship,
    activity,
    routine: routine ?? null,
    content,
    vars,
  });
}

// --- M8: Portfolio Browser ---

/** Every indexed portfolio with its evidence count and staleness line,
 * sorted by slug. Backs the `/portfolios` selector. */
export function listPortfolios(): Promise<PortfolioSummary[]> {
  return call("list_portfolios");
}

/** The composed Portfolio Detail bundle behind `/portfolios/:slug`: the
 * unifying question, the linked project + related questions, and every
 * evidence note (newest first). */
export function getPortfolio(slug: string): Promise<PortfolioDetail> {
  return call("get_portfolio", { slug });
}

/** File an evidence note into a portfolio — the quick-add composer. An
 * `origin` that resolves to no note comes back as a `CuadernoError`
 * (kind "invalid") the caller shows inline; this is a deliberate
 * tightening the GUI adds over the scripted MCP/CLI surfaces. */
export function addEvidence(
  portfolio: string,
  source: string,
  origin: string,
  content: string,
): Promise<void> {
  return call("add_evidence", { portfolio, source, origin, content });
}

// --- Calendar view (#340) ---

/** The daily note for `date` (an `YYYY-MM-DD` string) plus the neighbour
 * identities the calendar panel's quick-nav uses — prev/next day, the
 * Monday of the week, and the `YYYY-MM` month — all stamped in Rust so
 * the frontend never computes a domain date for a read (§3.7). A day
 * with no note comes back `exists: false` (a calm empty state), never an
 * error. */
export function readDaily(date: string): Promise<DailyView> {
  return call("read_daily", { date });
}

/** The raw weekly note covering `weekOf` (an `YYYY-MM-DD` string naming
 * any day in the week) — the calendar panel's week jump. Distinct from
 * `getWeeklyBundle`, which composes the guided review. Rust `week_of` is
 * `weekOf` on the wire — pinned by the backend IPC round-trip. Absence
 * is non-error. */
export function readWeekly(weekOf: string): Promise<WeeklyView> {
  return call("read_weekly", { weekOf });
}

/** The raw monthly note covering `month` (a `YYYY-MM` string) — the
 * calendar panel's month jump (#228). Absence is non-error. */
export function readMonthly(month: string): Promise<MonthlyView> {
  return call("read_monthly", { month });
}

/** The `YYYY-MM-DD` dates in `year`/`month` that already have a daily
 * note, so the calendar grid can mark note-bearing days. `month` is
 * 1..=12; out of range comes back as a `CuadernoError` (kind "invalid").
 */
export function listDailyDates(year: number, month: number): Promise<string[]> {
  return call("list_daily_dates", { year, month });
}

// --- M9: Strategic / Monthly ---

/** The composed Strategic / Monthly bundle behind `/strategic`: the
 * active questions, portfolio-health rows, the active + parked project
 * slots with the configured cap, the stewardship overview with a
 * precomputed 12-week habit sparkline each, and the six-week
 * commitments window. One read paints the whole page. */
export function getStrategicBundle(): Promise<StrategicBundle> {
  return call("get_strategic_bundle");
}

// --- Templates view (#357) ---

/** Every note type and the status of its template — the Templates list.
 * Built-ins first, then config-defined custom types. A built-in always
 * has an effective template (`source` set); a custom type with no file
 * comes back `source: null`, which the view offers to `Create`. */
export function listTemplates(): Promise<TemplateSummary[]> {
  return call("list_templates");
}

/** The effective content of `noteType`'s template (custom override if
 * present, else the built-in default; a synthesised starter for a custom
 * type with no file) plus its source rung. `variant` targets a specific
 * variant template (the list view always reads the base — omit it). */
export function readTemplate(noteType: string, variant?: string): Promise<TemplateContent> {
  return call("read_template", { noteType, variant: variant ?? null });
}

/** The full placeholder set `noteType` supports — built-in supplied keys,
 * a custom type's declared schema fields, and config variables/prompts —
 * for the editor's reference panel and its unknown-token check. */
export function listTemplatePlaceholders(noteType: string): Promise<TemplatePlaceholder[]> {
  return call("list_template_placeholders", { noteType });
}

/** Save `content` verbatim as the custom template for `noteType`.
 * Transparently creates the override for a built-in-backed type on first
 * save (the edit-and-save model). Never blocks on unknown tokens — the
 * editor only warns. */
export function saveTemplate(noteType: string, content: string, variant?: string): Promise<void> {
  return call("save_template", { noteType, variant: variant ?? null, content });
}

/** Scaffold a starter template for a config-defined custom type that has
 * none yet. A built-in type comes back a `CuadernoError` (kind "invalid")
 * — edit-and-save its override instead. */
export function createTemplate(noteType: string): Promise<void> {
  return call("create_template", { noteType });
}

// --- Config inspector (#365, PR1) ---

/** The raw `.cuaderno/config.toml` text plus its content hash — the
 * read-only Config inspector's read. The hash is carried through for a
 * later compare-and-swap save (PR3); PR1 only displays the content. */
export function readConfig(): Promise<ConfigDocument> {
  return call("read_config");
}

/** The structured projection of the parsed config — vault meta, the
 * `[note_types.*]` table, and the `[schemas.*]` table (each sorted by
 * name) — backing the read-only structured Config view (#365, PR5a).
 * Distinct from `readConfig`, which returns the raw file text for the
 * raw editor. Reflects the config currently in effect (a live reload
 * keeps it in step with an external edit). */
export function readConfigModel(): Promise<ConfigModel> {
  return call("read_config_model");
}

/** Parse a candidate config draft STRING into the structured model the
 * editable Form renders (#365, PR5b). Distinct from `readConfigModel`,
 * which projects the config currently in effect: this projects the live
 * (possibly unsaved, multi-edit) draft, so the form always mirrors what
 * Save would persist. An unparseable draft rejects with a `CuadernoError`
 * (kind "invalid") the form shows as a calm "fix it in Raw" state. */
export function parseConfigModel(content: string): Promise<ConfigModel> {
  return call("parse_config_model", { content });
}

// --- Config form surgical edits (#365, PR5b) ---
//
// Each takes the current draft string plus the one touched piece, applies
// a comment-preserving `toml_edit` edit to only that table server-side,
// and resolves to the NEW draft string. The form feeds that back into the
// shared `useConfigDraft` draft; the existing `saveConfig` gate persists
// it. These never write — persistence is always the draft string + the
// save gate, never a client re-serialise. Rust `note_type` is `noteType`
// on the wire (Tauri camelCases args).

/** Insert or replace `[note_types.<name>]` in `content`; resolves to the
 * new config string. */
export function configSetNoteType(
  content: string,
  name: string,
  noteType: CustomNoteType,
): Promise<string> {
  return call("config_set_note_type", { content, name, noteType });
}

/** Remove `[note_types.<name>]` from `content`; resolves to the new config
 * string. Idempotent (removing an absent type is a no-op). */
export function configRemoveNoteType(content: string, name: string): Promise<string> {
  return call("config_remove_note_type", { content, name });
}

/** Insert or replace `[schemas.<noteType>.fields.<field>]` in `content`;
 * resolves to the new config string. */
export function configSetSchemaField(
  content: string,
  noteType: string,
  field: string,
  spec: FieldSpec,
): Promise<string> {
  return call("config_set_schema_field", { content, noteType, field, spec });
}

/** Remove `[schemas.<noteType>.fields.<field>]` from `content`; resolves
 * to the new config string. Idempotent. */
export function configRemoveSchemaField(
  content: string,
  noteType: string,
  field: string,
): Promise<string> {
  return call("config_remove_schema_field", { content, noteType, field });
}

/** Insert or replace a static template variable (`[variables].<name>`) in
 * `content`; resolves to the new config string. (#376) */
export function configSetVariable(
  content: string,
  name: string,
  value: string,
): Promise<string> {
  return call("config_set_variable", { content, name, value });
}

/** Remove a static template variable from `[variables]`; resolves to the new
 * config string. Idempotent. */
export function configRemoveVariable(content: string, name: string): Promise<string> {
  return call("config_remove_variable", { content, name });
}

/** Insert or replace a prompted variable (`[variables.prompt].<name>`) in
 * `content`; resolves to the new config string. (#376) */
export function configSetPromptVariable(
  content: string,
  name: string,
  message: string,
): Promise<string> {
  return call("config_set_prompt_variable", { content, name, message });
}

/** Remove a prompted variable from `[variables.prompt]`; resolves to the new
 * config string. Idempotent. */
export function configRemovePromptVariable(content: string, name: string): Promise<string> {
  return call("config_remove_prompt_variable", { content, name });
}

/** The outcome of a dry-run config validation: `ok` when the config
 * would open, otherwise the structured backend error (message + optional
 * line/col for a TOML syntax error). */
export type ValidationResult = { ok: true } | { ok: false; error: ConfigValidationError };

/** Dry-run the backend's config validation against `content` (the same
 * check `Vault::new` runs). Resolves to a discriminated result rather
 * than throwing: an invalid config is an expected answer for the Check
 * button, not an exception. The backend rejects with the serialised
 * `{ message, line, col }` shape, which this maps to `{ ok: false }`. */
export async function validateConfig(content: string): Promise<ValidationResult> {
  try {
    await invoke<void>("validate_config", { content });
    return { ok: true };
  } catch (raw) {
    // The domain validation error serialises as `{ message, line, col }`
    // — it has no `kind` tag, so `call()`'s CuadernoError wrapping never
    // applies; we surface it verbatim as the failed branch.
    return { ok: false, error: raw as ConfigValidationError };
  }
}

// --- Config editor save (#365, PR3) ---

/** The typed error a `saveConfig` rejection surfaces — a discriminated
 * union mirroring the backend `ConfigSaveError`: a `validation` failure
 * carries the same `{ message, line, col }` the dry-run returns; a
 * `conflict` means the file changed on disk since it was read (reload
 * before saving); `internal` is a generic backend fault. Thrown as-is by
 * `saveConfig` so the caller can `switch` on `.kind`. */
export type ConfigSaveErrorPayload = ConfigSaveError;

/** Save an edited `.cuaderno/config.toml`. The backend validates the
 * candidate FIRST (a config that would not reopen is rejected before any
 * write — the never-brick guarantee), then compares `expectedHash`
 * against the current on-disk file (rejecting a concurrent hand-edit),
 * writes verbatim, and live-reloads so the edit applies with no restart.
 *
 * Resolves to the persisted document (content + fresh hash) — the UI's
 * next compare-and-swap baseline. Rejects with the tagged
 * `ConfigSaveError` (validation / conflict / internal), which this
 * surfaces verbatim (it has a `kind` tag but is NOT the `CmdError`
 * taxonomy, so it is thrown raw rather than wrapped). */
export async function saveConfig(
  content: string,
  expectedHash: string,
): Promise<ConfigDocument> {
  try {
    return await invoke<ConfigDocument>("save_config", { content, expectedHash });
  } catch (raw) {
    // `ConfigSaveError` serialises tagged as `{ kind, data? }` — surface
    // it verbatim so the caller can distinguish validation vs conflict vs
    // internal, rather than flattening it to a CuadernoError.
    throw raw as ConfigSaveError;
  }
}
