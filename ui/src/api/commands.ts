// Typed wrappers over Tauri invoke — one function per backend
// command. Components never call invoke() directly; this module is
// the single seam the tests mock.
import { invoke } from "@tauri-apps/api/core";
import type { CmdError } from "./bindings/CmdError";
import type { CommitmentsView } from "./bindings/CommitmentsView";
import type { EnergyLevel } from "./bindings/EnergyLevel";
import type { InboxItem } from "./bindings/InboxItem";
import type { NoteView } from "./bindings/NoteView";
import type { OrientationView } from "./bindings/OrientationView";
import type { ProjectActions } from "./bindings/ProjectActions";
import type { ProjectDetail } from "./bindings/ProjectDetail";
import type { ResolvedLink } from "./bindings/ResolvedLink";
import type { SearchResultEntry } from "./bindings/SearchResultEntry";
import type { WeeklyBundle } from "./bindings/WeeklyBundle";

export class CuadernoError extends Error {
  readonly payload: CmdError;

  constructor(payload: CmdError) {
    super(typeof payload.data === "string" ? payload.data : payload.kind);
    this.name = "CuadernoError";
    this.payload = payload;
  }
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

export function updateProjectState(project: string, newState: string): Promise<void> {
  // Rust `new_state` is `newState` on the wire (Tauri camelCases
  // command args) — pinned by the backend IPC round-trip test.
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

// --- M5: note reader, project detail, actions list, command palette ---

/** Read any vault note for the slide-in reader: parsed frontmatter,
 * markdown body, note type, and title. */
export function readNote(path: string): Promise<NoteView> {
  return call("read_note", { path });
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
