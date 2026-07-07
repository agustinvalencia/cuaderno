// Typed wrappers over Tauri invoke — one function per backend
// command. Components never call invoke() directly; this module is
// the single seam the tests mock.
import { invoke } from "@tauri-apps/api/core";
import type { CmdError } from "./bindings/CmdError";
import type { OrientationView } from "./bindings/OrientationView";

export class CuadernoError extends Error {
  readonly payload: CmdError;

  constructor(payload: CmdError) {
    super(typeof payload.data === "string" ? payload.data : payload.kind);
    this.name = "CuadernoError";
    this.payload = payload;
  }
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
