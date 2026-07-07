// Typed wrappers over Tauri invoke — one function per backend
// command. Components never call invoke() directly; this module is
// the single seam the tests mock.
import { invoke } from "@tauri-apps/api/core";
import type { OrientationView } from "./bindings/OrientationView";

/** Serialised CmdError shape (crates/cdno-tauri/src/error.rs). */
export type CmdErrorPayload =
  | { kind: "project_cap_reached"; data: { current: number; max: number; active: string[] } }
  | { kind: "not_found"; data: string }
  | { kind: "ambiguous"; data: { query: string; candidates: string[] } }
  | { kind: "invalid"; data: string }
  | { kind: "internal"; data: string };

export class CuadernoError extends Error {
  readonly payload: CmdErrorPayload;

  constructor(payload: CmdErrorPayload) {
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
      throw new CuadernoError(raw as CmdErrorPayload);
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
