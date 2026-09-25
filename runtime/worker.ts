import { VERSION, errorOf, validateOutput, type WorkerRequest, type WorkerResponse } from "./protocol.js";

interface WasmApp { start(): string; dispatch(command: string): string; free(): void }
interface AppModule {
  default(options: { module_or_path: ArrayBuffer }): Promise<unknown>;
  App: new (init: string) => WasmApp;
}
let app: WasmApp | undefined;
let busy = false;
const scope = globalThis as unknown as {
  onmessage: (event: MessageEvent<WorkerRequest>) => void;
  postMessage(message: WorkerResponse): void;
};
scope.onmessage = async ({ data }) => {
  try {
    if (data.version !== VERSION || busy) throw new Error("Invalid worker protocol or concurrent request");
    busy = true;
    let json: string;
    if (data.type === "init") {
      if (app) throw new Error("Worker already initialized");
      const module: AppModule = await import(/* @vite-ignore */ data.moduleUrl);
      await module.default({ module_or_path: data.wasm });
      app = new module.App(JSON.stringify(data.init));
      json = app.start();
    } else {
      if (!app) throw new Error("Worker not initialized");
      json = app.dispatch(JSON.stringify(data.command));
    }
    if (json.length > 24 * 1024 * 1024) throw new Error("App response exceeds 24 MiB");
    const output = validateOutput(JSON.parse(json));
    if (output.exited) { app?.free(); app = undefined; }
    scope.postMessage({ version: VERSION, id: data.id, type: "output", output });
  } catch (error) {
    scope.postMessage({ version: VERSION, id: data.id, type: "error", error: errorOf(error).message });
  } finally {
    busy = false;
  }
};
