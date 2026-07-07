// Typed wrappers over Tauri invoke — one function per backend
// command. Components never call invoke() directly; this module is
// the single seam the tests mock.
import { invoke } from "@tauri-apps/api/core";
import type { CmdError } from "./bindings/CmdError";
import type { CommitmentsView } from "./bindings/CommitmentsView";
import type { InboxItem } from "./bindings/InboxItem";
import type { OrientationView } from "./bindings/OrientationView";

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
