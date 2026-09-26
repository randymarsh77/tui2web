import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import {
  VERSION, MAX_INPUT_BYTES, dimensions, errorOf, parseSnapshot, validateOutput, validateSnapshot,
  type Input, type Modifiers, type Snapshot, type Command, type Output, type WorkerRequest, type WorkerResponse,
} from "./protocol.js";
import { indexedDbStore, type SnapshotStore } from "./storage.js";

export type Status = { state: "starting" | "running" | "exited" | "error" | "disposed"; error?: string };
export interface MountOptions {
  /** Trusted prebuilt wasm-bindgen ES module. This mode is NOT a security boundary. */
  moduleUrl: string | URL;
  wasmUrl?: string | URL;
  /** Already fetched assets also allow offline restarts and opaque-origin mounting. */
  wasm?: ArrayBuffer;
  workerUrl?: string | URL;
  /** Opaque-origin blob Workers require classic loading in Chromium. The app remains an ES module. */
  workerType?: WorkerType;
  columns?: number;
  rows?: number;
  config?: Record<string, string>;
  persistence?: string | SnapshotStore;
  snapshot?: Snapshot;
  timeoutMs?: number;
  onStatus?: (status: Status) => void;
}
export interface Handle {
  readonly status: Status;
  resize(columns: number, rows: number): Promise<void>;
  send(input: Input): Promise<void>;
  snapshot(): Promise<Snapshot>;
  exportSnapshot(): Promise<string>;
  importSnapshot(json: string): Promise<void>;
  restart(): Promise<void>;
  reset(): Promise<void>;
  focus(): void;
  subscribe(listener: (status: Status) => void): () => void;
  dispose(): Promise<void>;
}
type Job = {
  request: WorkerRequest; bytes: number;
  resolve: (output: Output) => void; reject: (error: Error) => void;
};
const modifiers = (event: MouseEvent | KeyboardEvent): Modifiers => ({
  ctrl: event.ctrlKey, alt: event.altKey, shift: event.shiftKey, meta: event.metaKey,
});
export async function fetchAsset(url: string | URL, limit: number): Promise<ArrayBuffer> {
  const response = await fetch(url, { credentials: "omit", referrerPolicy: "no-referrer" });
  if (!response.ok) throw new Error(`Asset load failed (${response.status}): ${url}`);
  if (!response.body) throw new Error("Asset response has no body");
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) { await reader.cancel(); throw new Error("Asset exceeds size limit"); }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  return bytes.buffer;
}

/** Mount a trusted module. Use mountIsolated for the restricted opaque-origin option. */
export async function mount(element: HTMLElement, options: MountOptions): Promise<Handle> {
  dimensions(options.columns ?? 80, options.rows ?? 24);
  const timeout = options.timeoutMs ?? 10000;
  if (!Number.isFinite(timeout) || timeout < 100 || timeout > 120000) throw new Error("timeoutMs must be 100..120000");
  const store = typeof options.persistence === "string" ? indexedDbStore(options.persistence) : options.persistence;
  const moduleUrl = new URL(options.moduleUrl, document.baseURI).href;
  const wasm = options.wasm ?? await fetchAsset(options.wasmUrl ?? moduleUrl.replace(/\.js$/, "_bg.wasm"), 32 * 1024 * 1024);
  const terminal = new Terminal({
    cols: options.columns ?? 80, rows: options.rows ?? 24, scrollback: 0,
    convertEol: false, fontFamily: "monospace", fontSize: 14,
    allowProposedApi: true, theme: { background: "#141821", foreground: "#d6e0ee" },
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.loadAddon(new Unicode11Addon());
  terminal.unicode.activeVersion = "11";
  terminal.open(element);
  const listeners = new Set<(status: Status) => void>();
  if (options.onStatus) listeners.add(options.onStatus);
  let status: Status = { state: "starting" };
  let worker: Worker | undefined;
  let generation = 0;
  let sequence = 0;
  let active: Job | undefined;
  let receiving = false;
  const queue: Job[] = [];
  let queuedBytes = 0;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  let wake: ReturnType<typeof setTimeout> | undefined;
  let lastSnapshot: Snapshot | null = null;
  let lifecycleBusy = false;
  let disposing = false;
  const cleanup: (() => void)[] = [];
  function notify(next: Status) {
    status = next;
    for (const listener of listeners) {
      try { listener({ ...next }); }
      catch (error) { console.error("tui2web status listener failed", error); }
    }
  }
  function stop(error: Error) {
    generation++;
    clearTimeout(deadline);
    clearTimeout(wake);
    worker?.terminate();
    worker = undefined;
    active?.reject(error);
    active = undefined;
    receiving = false;
    for (const job of queue.splice(0)) job.reject(error);
    queuedBytes = 0;
  }
  function fail(error: unknown) {
    if (status.state === "disposed") return;
    const e = errorOf(error);
    stop(e);
    notify({ state: "error", error: e.message });
  }
  function pump() {
    if (active || !worker || !queue.length) return;
    active = queue.shift()!;
    queuedBytes -= active.bytes;
    if (active.request.type === "command" && active.request.command.type === "resize") {
      terminal.resize(active.request.command.columns, active.request.command.rows);
    }
    deadline = setTimeout(() => fail(new Error("Worker/output acknowledgement timed out; restart to recover")), timeout);
    try { worker.postMessage(active.request); }
    catch (error) { fail(error); }
  }
  function request(request: WorkerRequest): Promise<Output> {
    return new Promise((resolve, reject) => {
      if (!worker) { reject(new Error(`Runtime is ${status.state}`)); return; }
      const bytes = request.type === "init" ? 0 : new TextEncoder().encode(JSON.stringify(request)).length;
      if (queue.length >= 128 || request.type !== "init" && queuedBytes + bytes > 1024 * 1024) {
        const error = new Error("Input queue quota exceeded; restart to recover");
        reject(error);
        fail(error);
        return;
      }
      queue.push({ request, bytes, resolve, reject });
      queuedBytes += bytes;
      pump();
    });
  }
  function command(command: Command): Promise<Output> {
    if (status.state !== "running" && command.type !== "shutdown") return Promise.reject(new Error(`Runtime is ${status.state}`));
    return request({ version: VERSION, id: ++sequence, type: "command", command });
  }
  async function receive(data: WorkerResponse, epoch: number) {
    if (epoch !== generation) return;
    try {
      if (!active || receiving || data.version !== VERSION || data.id !== active.request.id) throw new Error("Unexpected worker response");
      receiving = true;
      if (data.type === "error") throw new Error(data.error);
      if (data.type !== "output") throw new Error("Invalid worker response");
      const output = validateOutput(data.output);
      const job = active;
      if (output.frame !== null) {
        await new Promise<void>(resolve => terminal.write(output.frame!, resolve));
      }
      if (epoch !== generation) return;
      if (output.snapshot) {
        if (store) await store.save(output.snapshot);
        if (epoch !== generation) return;
        lastSnapshot = structuredClone(output.snapshot);
      }
      clearTimeout(deadline);
      const schedulesWake = job.request.type === "init"
        || job.request.type === "command" && job.request.command.type === "event";
      if (schedulesWake || output.exited) clearTimeout(wake);
      active = undefined;
      receiving = false;
      job.resolve(output);
      if (output.exited) {
        worker?.terminate();
        worker = undefined;
        for (const pending of queue.splice(0)) pending.reject(new Error("Application exited"));
        queuedBytes = 0;
        notify({ state: "exited" });
      } else {
        if (schedulesWake && output.wakeAfterMs !== null) {
          wake = setTimeout(() => {
            void send({ type: "tick" }).catch(fail);
          }, Math.max(16, Math.min(output.wakeAfterMs, 0x7fffffff)));
        }
        if (status.state === "starting") notify({ state: "running" });
      }
      if (epoch === generation) pump();
    } catch (error) { if (epoch === generation) fail(error); }
  }
  async function start(snapshot: Snapshot | null) {
    if (disposing || status.state === "disposed") throw new Error("Runtime disposed");
    stop(new Error("Runtime restarted"));
    terminal.reset();
    notify({ state: "starting" });
    const epoch = generation;
    worker = new Worker(options.workerUrl ?? new URL("./worker.js", import.meta.url), { type: options.workerType ?? "module", name: "tui2web" });
    worker.onmessage = event => { void receive(event.data, epoch); };
    worker.onmessageerror = () => fail(new Error("Worker message could not be decoded"));
    worker.onerror = event => { event.preventDefault(); fail(new Error(event.message || "Worker crashed")); };
    await request({
      version: VERSION, id: ++sequence, type: "init", moduleUrl, wasm,
      init: { version: VERSION, columns: terminal.cols, rows: terminal.rows,
        nowMs: Date.now(), randomSeed: crypto.getRandomValues(new Uint32Array(1))[0],
        config: options.config ?? {}, snapshot },
    });
  }
  async function send(input: Input) {
    if (disposing || lifecycleBusy) throw new Error("Runtime lifecycle operation in progress");
    if ((input.type === "text" || input.type === "paste") && new TextEncoder().encode(input.text).length > MAX_INPUT_BYTES) {
      throw new Error("Text/paste exceeds 64 KiB");
    }
    await command({ type: "event", input, nowMs: Date.now() });
  }
  function userInput(input: Input) {
    if (status.state !== "running" || disposing || lifecycleBusy) return;
    void send(input).catch(fail);
  }
  terminal.attachCustomKeyEventHandler(event => {
    if (event.isComposing || event.key === "Process" || event.key === "Dead") return true;
    const key = event.key.toLowerCase();
    const printable = [...event.key].length === 1;
    const shortcut = event.ctrlKey || event.metaKey;
    const altGraph = event.getModifierState("AltGraph") || (event.ctrlKey && event.altKey && printable);
    // Leave browser navigation, clipboard and developer shortcuts to the browser.
    if (shortcut && !altGraph && (["v", "l", "r", "t", "w"].includes(key)
      || (key === "c" && (terminal.hasSelection() || event.metaKey)) || event.shiftKey && ["i", "j", "c"].includes(key))) return false;
    if (!printable || (shortcut || event.altKey) && !altGraph) {
      if (event.type === "keydown" && !["Shift", "Control", "Alt", "Meta", "CapsLock"].includes(event.key)) {
        event.preventDefault();
        userInput({ type: "key", key: event.key, code: event.code, repeat: event.repeat, modifiers: modifiers(event) });
      }
      return false;
    }
    return true;
  });
  cleanup.push(() => terminal.dispose());
  const dataListener = terminal.onData(text => userInput({ type: "text", text }));
  cleanup.push(() => dataListener.dispose());
  function listen<K extends keyof HTMLElementEventMap>(name: K, callback: (event: HTMLElementEventMap[K]) => void, capture = false) {
    element.addEventListener(name, callback, capture);
    cleanup.push(() => element.removeEventListener(name, callback, capture));
  }
  listen("paste", event => {
    event.preventDefault();
    event.stopImmediatePropagation();
    userInput({ type: "paste", text: event.clipboardData?.getData("text/plain") ?? "" });
  }, true);
  listen("focusin", () => userInput({ type: "focus", focused: true }));
  listen("focusout", () => userInput({ type: "focus", focused: false }));
  function mouse(event: MouseEvent, kind: "down" | "up" | "move" | "wheelUp" | "wheelDown") {
    if (event.shiftKey || kind === "move" && event.buttons === 0) return;
    const screen = element.querySelector(".xterm-screen");
    if (!screen) return;
    const rect = screen.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    const column = Math.floor((event.clientX - rect.left) / rect.width * terminal.cols);
    const row = Math.floor((event.clientY - rect.top) / rect.height * terminal.rows);
    if (column < 0 || column >= terminal.cols || row < 0 || row >= terminal.rows) return;
    userInput({ type: "mouse", kind, column, row, button: event.button, modifiers: modifiers(event) });
  }
  listen("mousedown", e => { terminal.focus(); mouse(e, "down"); });
  listen("mouseup", e => mouse(e, "up"));
  listen("mousemove", e => mouse(e, "move"));
  listen("wheel", e => mouse(e, e.deltaY < 0 ? "wheelUp" : "wheelDown"));
  async function resize(columns: number, rows: number) {
    dimensions(columns, rows);
    if (disposing || lifecycleBusy) throw new Error("Runtime lifecycle operation in progress");
    await command({ type: "resize", columns, rows });
  }
  const observer = new ResizeObserver(() => {
    if (status.state !== "running" || disposing || lifecycleBusy) return;
    const size = fit.proposeDimensions();
    if (!size) return;
    const columns = Math.min(300, Math.max(1, size.cols));
    const rows = Math.min(120, Math.max(1, size.rows));
    if (columns !== terminal.cols || rows !== terminal.rows) void resize(columns, rows).catch(fail);
  });
  cleanup.push(() => observer.disconnect());
  async function lifecycle(action: () => Promise<void>) {
    if (status.state === "disposed" || disposing || lifecycleBusy) throw new Error("Runtime lifecycle unavailable");
    lifecycleBusy = true;
    try { await action(); }
    catch (error) { fail(error); throw error; }
    finally { lifecycleBusy = false; }
  }
  const handle: Handle = {
    get status() { return { ...status }; },
    resize, send,
    async snapshot() {
      const output = await command({ type: "snapshot" });
      if (!output.snapshot) throw new Error("App returned no snapshot");
      return structuredClone(output.snapshot);
    },
    async exportSnapshot() { return JSON.stringify(await handle.snapshot()); },
    async importSnapshot(json) {
      const snapshot = parseSnapshot(json);
      await lifecycle(() => start(snapshot));
    },
    async restart() {
      await lifecycle(async () => {
        stop(new Error("Restart requested"));
        await start(store ? await store.load() : lastSnapshot);
      });
    },
    async reset() {
      await lifecycle(async () => {
        stop(new Error("Reset requested"));
        if (store) await store.clear();
        lastSnapshot = null;
        await start(null);
      });
    },
    focus() { if (status.state !== "disposed") terminal.focus(); },
    subscribe(listener) { listeners.add(listener); listener({ ...status }); return () => listeners.delete(listener); },
    async dispose() {
      if (status.state === "disposed") return;
      disposing = true;
      try {
        if (status.state === "running" && !lifecycleBusy) await command({ type: "shutdown" });
      } finally {
        stop(new Error("Runtime disposed"));
        for (const dispose of cleanup.reverse()) dispose();
        notify({ state: "disposed" });
        listeners.clear();
      }
    },
  };
  try {
    const snapshot = options.snapshot ? validateSnapshot(options.snapshot) : store ? await store.load() : null;
    await start(snapshot);
    observer.observe(element);
    return handle;
  } catch (error) {
    stop(errorOf(error));
    for (const dispose of cleanup.reverse()) dispose();
    notify({ state: "error", error: errorOf(error).message });
    throw error;
  }
}
