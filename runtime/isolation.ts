import { fetchAsset, type Handle, type MountOptions, type Status } from "./mount.js";
import { indexedDbStore } from "./storage.js";
import { errorOf, parseSnapshot, validateSnapshot, type Snapshot } from "./protocol.js";

export interface IsolatedOptions extends Omit<MountOptions, "workerUrl" | "workerType" | "wasm"> {
  /** Trusted runtime assets, NEVER supplied by the playground application. */
  runtimeBaseUrl?: string | URL;
}
/** An opaque-origin frame runs trusted host code; only its network-denied Worker imports app glue.
 * Browser memory/process limits still apply. See README's security contract before embedding.
 */
export async function mountIsolated(element: HTMLElement, options: IsolatedOptions): Promise<Handle> {
  const base = options.runtimeBaseUrl ?? new URL("./", import.meta.url);
  const moduleUrl = new URL(options.moduleUrl, document.baseURI);
  const [frameBytes, cssBytes, workerBytes, moduleBytes, wasm] = await Promise.all([
    fetchAsset(new URL("frame.js", base), 4 * 1024 * 1024),
    fetchAsset(new URL("style.css", base), 1024 * 1024),
    fetchAsset(new URL("worker.js", base), 1024 * 1024),
    fetchAsset(moduleUrl, 4 * 1024 * 1024),
    fetchAsset(options.wasmUrl ?? moduleUrl.href.replace(/\.js$/, "_bg.wasm"), 32 * 1024 * 1024),
  ]);
  const decode = (bytes: ArrayBuffer) => new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  const store = typeof options.persistence === "string" ? indexedDbStore(options.persistence) : options.persistence;
  const iframe = document.createElement("iframe");
  iframe.title = "Isolated tui2web playground";
  iframe.sandbox.add("allow-scripts");
  iframe.setAttribute("referrerpolicy", "no-referrer");
  iframe.style.cssText = "width:100%;height:100%;border:0;display:block";
  const channel = new MessageChannel();
  const port = channel.port1;
  let status: Status = { state: "starting" };
  let disposed = false;
  let storageBusy = false;
  let sequence = 0;
  const listeners = new Set<(status: Status) => void>();
  if (options.onStatus) listeners.add(options.onStatus);
  const pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  function notify(next: Status) {
    status = next;
    for (const listener of listeners) {
      try { listener({ ...next }); } catch (error) { console.error("tui2web status listener failed", error); }
    }
  }
  function close(error: Error) {
    disposed = true;
    port.close();
    iframe.remove();
    for (const job of pending.values()) { clearTimeout(job.timer); job.reject(error); }
    pending.clear();
  }
  function rpc(operation: string, args: unknown[] = []): Promise<unknown> {
    if (disposed) return Promise.reject(new Error("Isolated runtime disposed"));
    if (pending.size >= 128) return Promise.reject(new Error("Isolated request queue exceeded"));
    const id = ++sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const error = new Error("Isolated frame timed out; dispose and remount to recover");
        notify({ state: "error", error: error.message });
        close(error);
      }, (options.timeoutMs ?? 10000) + 2000);
      pending.set(id, { resolve, reject, timer });
      port.postMessage({ type: "rpc", id, operation, args });
    });
  }
  port.onmessage = async ({ data }) => {
    if (disposed) return;
    if (data?.type === "storage") {
      try {
        if (!store || storageBusy) throw new Error("Storage capability unavailable/busy");
        storageBusy = true;
        let value: Snapshot | null = null;
        if (data.operation === "load") value = await store.load();
        else if (data.operation === "save") await store.save(validateSnapshot(data.snapshot));
        else if (data.operation === "clear") await store.clear();
        else throw new Error("Unknown storage operation");
        if (value) validateSnapshot(value);
        port.postMessage({ type: "storageResult", id: data.id, value });
      } catch (error) {
        port.postMessage({ type: "storageResult", id: data.id, error: errorOf(error).message });
      } finally { storageBusy = false; }
    } else if (data?.type === "status") {
      if (["starting", "running", "exited", "error"].includes(data.status?.state)) notify(data.status);
    } else if (data?.type === "result") {
      const job = pending.get(data.id);
      if (!job) return;
      pending.delete(data.id);
      clearTimeout(job.timer);
      if (typeof data.error === "string") job.reject(new Error(data.error));
      else job.resolve(data.value);
    }
  };
  const loaded = new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Isolated frame did not load")), options.timeoutMs ?? 10000);
    iframe.onload = () => { clearTimeout(timer); resolve(); };
  });
  // Neither app source nor any app-controlled configuration is interpolated into HTML.
  iframe.srcdoc = `<!doctype html><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' 'wasm-unsafe-eval' blob:; worker-src blob:; child-src blob:; style-src 'unsafe-inline'; connect-src 'none'; img-src 'none'; font-src 'none'; form-action 'none'; base-uri 'none'">
<style>${decode(cssBytes).replace(/<\/style/gi, "<\\/style")} html,body,#terminal{width:100%;height:100%;margin:0;overflow:hidden;background:#141821}</style>
<div id="terminal"></div><script>${decode(frameBytes).replace(/<\/script/gi, "<\\/script")}</script>`;
  element.append(iframe);
  try {
    await loaded;
    iframe.contentWindow!.postMessage({ type: "tui2web-connect" }, "*", [channel.port2]);
    await rpc("init", [{
      moduleSource: decode(moduleBytes), workerSource: decode(workerBytes), wasm,
      columns: options.columns, rows: options.rows, config: options.config,
      snapshot: options.snapshot ? validateSnapshot(options.snapshot) : undefined,
      persistence: Boolean(store), timeoutMs: options.timeoutMs,
    }]);
  } catch (error) {
    close(errorOf(error));
    notify({ state: "error", error: errorOf(error).message });
    throw error;
  }
  const handle: Handle = {
    get status() { return { ...status }; },
    async resize(columns, rows) { await rpc("resize", [columns, rows]); },
    async send(input) { await rpc("send", [input]); },
    async snapshot() { return validateSnapshot(await rpc("snapshot")); },
    async exportSnapshot() { return JSON.stringify(await handle.snapshot()); },
    async importSnapshot(json) { parseSnapshot(json); await rpc("importSnapshot", [json]); },
    async restart() { await rpc("restart"); },
    async reset() { await rpc("reset"); },
    focus() { void rpc("focus").catch(error => notify({ state: "error", error: errorOf(error).message })); },
    subscribe(listener) { listeners.add(listener); listener({ ...status }); return () => listeners.delete(listener); },
    async dispose() {
      if (disposed) return;
      try { await rpc("dispose"); }
      finally { close(new Error("Isolated runtime disposed")); notify({ state: "disposed" }); listeners.clear(); }
    },
  };
  return handle;
}
