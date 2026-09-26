import { mount, type Handle } from "./mount.js";
import { errorOf, validateSnapshot, type Input, type Snapshot } from "./protocol.js";
import type { SnapshotStore } from "./storage.js";

interface FrameInit {
  moduleSource: string; workerSource: string; wasm: ArrayBuffer;
  columns?: number; rows?: number; config?: Record<string, string>;
  snapshot?: Snapshot; persistence: boolean; timeoutMs?: number;
}
let connected = false;
window.addEventListener("message", event => {
  if (connected || event.source !== parent || event.data?.type !== "tui2web-connect" || event.ports.length !== 1) return;
  connected = true;
  const port = event.ports[0];
  let handle: Handle | undefined;
  let sequence = 0;
  const urls: string[] = [];
  const pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();
  function storage(operation: string, snapshot?: Snapshot) {
    return new Promise<unknown>((resolve, reject) => {
      const id = ++sequence;
      pending.set(id, { resolve, reject });
      port.postMessage({ type: "storage", id, operation, snapshot });
    });
  }
  const store: SnapshotStore = {
    async load() { const value = await storage("load"); return value === null ? null : validateSnapshot(value); },
    async save(snapshot) { await storage("save", snapshot); },
    async clear() { await storage("clear"); },
  };
  function blob(source: string) {
    const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
    urls.push(url);
    return url;
  }
  port.onmessage = async ({ data }) => {
    if (data?.type === "storageResult") {
      const job = pending.get(data.id);
      if (!job) return;
      pending.delete(data.id);
      if (data.error) job.reject(new Error(data.error));
      else job.resolve(data.value);
      return;
    }
    if (data?.type !== "rpc") return;
    try {
      let value: unknown;
      if (data.operation === "init") {
        if (handle) throw new Error("Frame already initialized");
        const init: FrameInit = data.args[0];
        handle = await mount(document.getElementById("terminal")!, {
          moduleUrl: blob(init.moduleSource), workerUrl: blob(init.workerSource), workerType: "classic", wasm: init.wasm,
          columns: init.columns, rows: init.rows, config: init.config, snapshot: init.snapshot,
          timeoutMs: init.timeoutMs, persistence: init.persistence ? store : undefined,
          onStatus: status => port.postMessage({ type: "status", status }),
        });
      } else {
        if (!handle) throw new Error("Frame not initialized");
        switch (data.operation) {
          case "resize": await handle.resize(data.args[0], data.args[1]); break;
          case "send": await handle.send(data.args[0] as Input); break;
          case "snapshot": value = await handle.snapshot(); break;
          case "importSnapshot": await handle.importSnapshot(data.args[0]); break;
          case "restart": await handle.restart(); break;
          case "reset": await handle.reset(); break;
          case "focus": handle.focus(); break;
          case "dispose":
            await handle.dispose();
            for (const url of urls) URL.revokeObjectURL(url);
            break;
          default: throw new Error("Unknown frame operation");
        }
      }
      port.postMessage({ type: "result", id: data.id, value });
    } catch (error) {
      port.postMessage({ type: "result", id: data.id, error: errorOf(error).message });
    }
  };
});
