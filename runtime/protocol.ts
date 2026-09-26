export const VERSION = 1;
export const MAX_SNAPSHOT_JSON = 16 * 1024 * 1024;
export const MAX_INPUT_BYTES = 65536;
export interface Snapshot {
  version: 1;
  directories: string[];
  files: [string, number[]][];
}
export interface Modifiers { ctrl: boolean; alt: boolean; shift: boolean; meta: boolean }
export type Input =
  | { type: "key"; key: string; code: string; repeat: boolean; modifiers: Modifiers }
  | { type: "text" | "paste"; text: string }
  | { type: "mouse"; kind: "down" | "up" | "move" | "wheelUp" | "wheelDown"; column: number; row: number; button: number; modifiers: Modifiers }
  | { type: "focus"; focused: boolean }
  | { type: "tick" };
export interface Init {
  version: 1; columns: number; rows: number; nowMs: number; randomSeed: number;
  config: Record<string, string>; snapshot: Snapshot | null;
}
export type Command =
  | { type: "event"; input: Input; nowMs: number }
  | { type: "resize"; columns: number; rows: number }
  | { type: "snapshot" | "shutdown" };
export interface Output {
  version: 1; frame: string | null; snapshot: Snapshot | null;
  exited: boolean; wakeAfterMs: number | null;
}
export type WorkerRequest =
  | { version: 1; id: number; type: "init"; moduleUrl: string; wasm: ArrayBuffer; init: Init }
  | { version: 1; id: number; type: "command"; command: Command };
export type WorkerResponse =
  | { version: 1; id: number; type: "output"; output: Output }
  | { version: 1; id: number; type: "error"; error: string };

export function errorOf(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function path(value: unknown): value is string {
  return typeof value === "string" && new TextEncoder().encode(value).length <= 1024
    && !/[\\\u0000-\u001f\u007f-\u009f]/u.test(value)
    && (value === "" || value.split("/").every(part => part !== "" && part !== "." && part !== ".."));
}
/** Validation before storage or crossing the trust boundary; matches Rust's v1 format. */
export function validateSnapshot(value: unknown): Snapshot {
  if (!record(value) || value.version !== VERSION || !Array.isArray(value.directories)
    || !Array.isArray(value.files) || Object.keys(value).some(k => !["version", "directories", "files"].includes(k))) {
    throw new Error("Invalid snapshot schema/version");
  }
  if (value.directories.length + value.files.length > 4096) throw new Error("Snapshot entry quota exceeded");
  const directories = new Set<string>();
  const files = new Set<string>();
  let bytes = 0;
  for (const p of value.directories) {
    if (!path(p) || directories.has(p)) throw new Error("Invalid/duplicate directory");
    directories.add(p);
  }
  if (!directories.has("")) throw new Error("Snapshot root is missing");
  for (const entry of value.files) {
    if (!Array.isArray(entry) || entry.length !== 2 || !path(entry[0]) || entry[0] === ""
      || !Array.isArray(entry[1]) || directories.has(entry[0]) || files.has(entry[0])) throw new Error("Invalid file entry");
    bytes += entry[1].length;
    if (bytes > 1024 * 1024) throw new Error("Snapshot byte quota exceeded (1 MiB)");
    if (!entry[1].every(b => Number.isInteger(b) && b >= 0 && b <= 255)) throw new Error("Invalid file byte");
    files.add(entry[0]);
  }
  for (const p of [...directories, ...files]) {
    const parent = p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : "";
    if (!directories.has(parent)) throw new Error("Snapshot parent is missing");
  }
  return {
    version: VERSION,
    directories: [...directories],
    files: value.files.map(([name, bytes]) => [name, bytes]),
  };
}
export function parseSnapshot(json: string): Snapshot {
  if (new TextEncoder().encode(json).length > MAX_SNAPSHOT_JSON) throw new Error("Snapshot JSON exceeds limit");
  return validateSnapshot(JSON.parse(json));
}
export function validateOutput(value: unknown): Output {
  if (!record(value) || value.version !== VERSION || typeof value.exited !== "boolean"
    || !(value.frame === null || (typeof value.frame === "string" && value.frame.length <= 4 * 1024 * 1024))
    || !(value.wakeAfterMs === null || (Number.isInteger(value.wakeAfterMs) && Number(value.wakeAfterMs) >= 0 && Number(value.wakeAfterMs) <= 0xffffffff))) {
    throw new Error("Invalid app output");
  }
  return {
    version: VERSION,
    frame: value.frame,
    snapshot: value.snapshot === null ? null : validateSnapshot(value.snapshot),
    exited: value.exited,
    wakeAfterMs: value.wakeAfterMs === null ? null : Number(value.wakeAfterMs),
  };
}
export function dimensions(columns: number, rows: number): void {
  if (!Number.isInteger(columns) || !Number.isInteger(rows) || columns < 1 || columns > 300 || rows < 1 || rows > 120) {
    throw new Error("Terminal dimensions must be 1..300 columns and 1..120 rows");
  }
}
