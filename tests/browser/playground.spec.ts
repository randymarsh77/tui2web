import { test, expect, type Page } from "@playwright/test";
import type { Handle } from "../../runtime/index.js";
declare global {
  interface Window {
    testHandle: Handle;
    fixtureUrl: string;
    workerMessageCount: number;
  }
}
async function ready(page: Page) {
  await page.goto("/");
  await expect(page.locator(".status").first()).toHaveText("running");
}
async function file(page: Page, index = 0, name = "hello.txt") {
  return page.evaluate(async ({ index, name }) => {
    const snap = await window.playground.handles[index].snapshot();
    return new TextDecoder().decode(new Uint8Array(snap.files.find(([p]) => p === name)![1]));
  }, { index, name });
}
const save = { type: "key" as const, key: "s", code: "KeyS", repeat: false, modifiers: { ctrl: true, meta: false, shift: false, alt: false } };
test("real editor: text, Unicode paste, modifiers, mouse, resize, export, persistence, reset", async ({ page }) => {
  await ready(page);
  const input = page.locator(".xterm-helper-textarea").first();
  await input.focus();
  await page.keyboard.type("typed ");
  await input.evaluate(element => {
    const data = new DataTransfer();
    data.setData("text/plain", "世界 e\u0301\npasted ");
    element.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: data }));
  });
  await page.keyboard.press("Control+s");
  expect(await file(page)).toMatch(/^typed 世界 é\npasted Welcome/);
  const exported = await page.evaluate(() => window.playground.handles[0].exportSnapshot());
  expect(JSON.parse(exported).directories).toContain("notes/empty");
  await page.reload();
  await expect(page.locator(".status").first()).toHaveText("running");
  expect(await file(page)).toMatch(/^typed 世界/);
  await page.evaluate(() => window.playground.handles[0].resize(40, 10));
  await page.locator(".xterm-helper-textarea").focus();
  await page.keyboard.press("Tab");
  await page.keyboard.type("second ");
  await page.keyboard.press("Control+s");
  expect(await file(page, 0, "notes/todo.txt")).toMatch(/^second /);
  const screen = page.locator(".xterm-screen").first();
  const bounds = (await screen.boundingBox())!;
  await page.mouse.click(bounds.x + 10, bounds.y + 20);
  await page.keyboard.press("Home");
  await page.keyboard.type("mouse ");
  await page.keyboard.press("Control+s");
  expect(await file(page)).toContain("mouse ");
  await page.evaluate(() => window.playground.handles[0].reset());
  expect(await file(page)).toMatch(/^Welcome/);
  await page.evaluate(json => window.playground.handles[0].importSnapshot(json), exported);
  expect(await file(page)).toMatch(/^typed 世界/);
  const invalid = await page.evaluate(async () => {
    try { await window.playground.handles[0].importSnapshot('{"version":2}'); return false; }
    catch { return true; }
  });
  expect(invalid).toBe(true);
  expect(await file(page)).toMatch(/^typed 世界/);
});

test("independent instances, offline-after-load, restart and disposal", async ({ page, context }) => {
  const foreign: string[] = [];
  page.on("request", req => { if (!req.url().startsWith("http://localhost:8080/")) foreign.push(req.url()); });
  await ready(page);
  await page.click("#add");
  await expect(page.locator(".status").nth(1)).toHaveText("running");
  await page.evaluate(async save => {
    await window.playground.handles[1].send({ type: "text", text: "independent " });
    await window.playground.handles[1].send(save);
  }, save);
  expect(await file(page, 1)).toMatch(/^independent/);
  expect(await file(page, 0)).toMatch(/^Welcome/);
  await context.setOffline(true);
  await page.evaluate(async save => {
    await window.playground.handles[0].send({ type: "text", text: "offline " });
    await window.playground.handles[0].send(save);
  }, save);
  expect(await file(page)).toMatch(/^offline/);
  await context.setOffline(false);
  await page.evaluate(() => window.playground.handles[0].restart());
  expect(await file(page)).toMatch(/^offline/);
  await page.evaluate(() => window.playground.handles[0].dispose());
  expect(await page.locator(".terminal").first().locator(".xterm").count()).toBe(0);
  expect(await file(page, 1)).toMatch(/^independent/);
  expect(foreign).toEqual([]);
});

test("isolated frame loads on a static host, denies network and mediates persistence", async ({ page }) => {
  await ready(page);
  await page.click("#isolated");
  await expect(page.locator(".status").nth(1)).toHaveText("running");
  const iframe = page.locator("iframe");
  await expect(iframe).toHaveAttribute("sandbox", "allow-scripts");
  await page.frameLocator("iframe").locator(".xterm-helper-textarea").focus();
  await page.keyboard.type("isolated ");
  await page.keyboard.press("Control+s");
  expect(await file(page, 1)).toMatch(/^isolated /);
  await page.evaluate(() => window.playground.handles[1].restart());
  expect(await file(page, 1)).toMatch(/^isolated /);
  const frame = page.frames().find(f => f.parentFrame())!;
  expect(await frame.evaluate(async () => {
    let parentDenied = false, networkDenied = false, storageDenied = false;
    try { void parent.document.body; } catch { parentDenied = true; }
    try { await fetch("http://localhost:8080/forbidden"); } catch { networkDenied = true; }
    try { localStorage.setItem("bad", "bad"); } catch { storageDenied = true; }
    return { parentDenied, networkDenied, storageDenied };
  })).toEqual({ parentDenied: true, networkDenied: true, storageDenied: true });
  await page.evaluate(() => window.playground.handles[1].reset());
  expect(await file(page, 1)).toMatch(/^Welcome/);
  await page.evaluate(() => window.playground.handles[1].dispose());
  await expect(iframe).toHaveCount(0);
});

test("worker traps and hangs are surfaced and recover through public restart", async ({ page }) => {
  await ready(page);
  await page.evaluate(async () => {
    const runtime = "/runtime/index.js";
    const { mount } = await import(runtime);
    const root = document.createElement("div");
    root.style.cssText = "width:800px;height:300px";
    document.body.append(root);
    const source = `export default async function() {}
      export class App {
        constructor() {}
        start() { return JSON.stringify({version:1,frame:null,snapshot:null,exited:false,wakeAfterMs:null}); }
        dispatch(raw) {
          const c = JSON.parse(raw);
          if (c.input?.text === 'hang') { while(true) {} }
          if (c.input?.text === 'crash') throw new Error('test trap');
          return this.start();
        }
        free() {}
      }`;
    const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
    window.testHandle = await mount(root, { moduleUrl: url, wasm: new ArrayBuffer(0), timeoutMs: 500 });
    window.fixtureUrl = url;
  });
  expect(await page.evaluate(async () => {
    try { await window.testHandle.send({ type: "text", text: "crash" }); return ""; }
    catch (e) { return String(e); }
  })).toContain("test trap");
  await page.evaluate(() => window.testHandle.restart());
  expect(await page.evaluate(() => window.testHandle.status.state)).toBe("running");
  expect(await page.evaluate(async () => {
    try { await window.testHandle.send({ type: "text", text: "hang" }); return ""; }
    catch (e) { return String(e); }
  })).toContain("timed out");
  await expect(page.locator("#add")).toBeEnabled();
  await page.evaluate(() => window.testHandle.restart());
  expect(await page.evaluate(() => window.testHandle.status.state)).toBe("running");
  const overflow = await page.evaluate(async () => {
    const jobs = [window.testHandle.send({ type: "text", text: "hang" })];
    for (let i = 0; i < 130; i++) jobs.push(window.testHandle.send({ type: "tick" }));
    const results = await Promise.allSettled(jobs);
    return { rejected: results.filter(r => r.status === "rejected").length, error: window.testHandle.status.error };
  });
  expect(overflow.rejected).toBe(131);
  expect(overflow.error).toContain("queue quota");
  await page.evaluate(() => window.testHandle.restart());
  await page.evaluate(async () => { await window.testHandle.dispose(); URL.revokeObjectURL(window.fixtureUrl); });
});

  test("guest Worker CSP denies fetch, WebSocket, module imports, importScripts and origin storage", async ({ page }) => {
    await ready(page);
    await page.route("**/guest-probe.js", route => route.fulfill({
      contentType: "text/javascript",
      body: `
        const results = {};
        export default async function() {
          for (const [name, action] of Object.entries({
            fetch: () => fetch('http://localhost:8080/forbidden-fetch'),
            moduleImport: () => import('http://localhost:8080/forbidden-module.js'),
            importScripts: () => importScripts('http://localhost:8080/forbidden-script.js'),
            indexedDB: () => new Promise((resolve,reject) => {
              const request = indexedDB.open('tui2web-v1');
              request.onsuccess = () => { request.result.close(); resolve(); };
              request.onerror = () => reject(request.error);
            }),
            webSocket: () => new Promise((resolve,reject) => {
              const ws = new WebSocket('ws://localhost:8080/forbidden-socket');
              ws.onopen = () => { ws.close(); resolve(); };
              ws.onerror = () => reject(new Error('denied'));
            }),
          })) {
            try { await action(); results[name] = 'ALLOWED'; }
            catch { results[name] = 'denied'; }
          }
        }
        export class App {
          start() { return JSON.stringify({version:1,frame:null,exited:false,wakeAfterMs:null,
            snapshot:{version:1,directories:[''],files:[['report.json',Array.from(new TextEncoder().encode(JSON.stringify(results)))]]}}); }
          dispatch() { return this.start(); }
          free() {}
        }`,
    }));
    await page.evaluate(async () => {
      const runtime = "/runtime/index.js";
      const { mountIsolated } = await import(runtime);
      const element = document.createElement("div");
      element.style.cssText = "width:800px;height:300px";
      document.body.append(element);
      window.testHandle = await mountIsolated(element, {
        moduleUrl: "/guest-probe.js", wasmUrl: "/pkg/tui2web_example_bg.wasm", persistence: "guest-csp-test",
      });
    });
    const snapshot = await page.evaluate(() => window.testHandle.snapshot());
    const report = JSON.parse(new TextDecoder().decode(new Uint8Array(snapshot.files[0][1])));
    expect(report).toEqual({ fetch: "denied", moduleImport: "denied", importScripts: "denied", indexedDB: "denied", webSocket: "denied" });
    expect(await file(page)).toMatch(/^Welcome/);
    await page.evaluate(() => window.testHandle.dispose());
  });

  test("composition commits text once, large input is rejected, and idle apps do not redraw", async ({ page }) => {
    await page.addInitScript(() => {
      window.workerMessageCount = 0;
      const post = Worker.prototype.postMessage;
      Worker.prototype.postMessage = function(message, transfer?: Transferable[] | StructuredSerializeOptions) {
        window.workerMessageCount++;
        return post.call(this, message, Array.isArray(transfer) ? { transfer } : transfer);
      };
    });
    await ready(page);
    const input = page.locator(".xterm-helper-textarea");
    await input.focus();
    await input.evaluate(element => {
      const textarea = element as HTMLTextAreaElement;
      textarea.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
      textarea.value = "日本";
      textarea.dispatchEvent(new CompositionEvent("compositionupdate", { bubbles: true, data: "日本" }));
      textarea.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "日本" }));
    });
    // xterm commits after compositionend in a macrotask.
    await page.waitForTimeout(50);
    await page.keyboard.press("Control+s");
    expect(await file(page)).toMatch(/^日本Welcome/);
    expect(await page.evaluate(async () => {
      try { await window.playground.handles[0].send({ type: "paste", text: "x".repeat(65537) }); return false; }
      catch { return true; }
    })).toBe(true);
    expect(await page.evaluate(() => window.playground.handles[0].status.state)).toBe("running");
    const before = await page.evaluate(() => window.workerMessageCount);
    await page.waitForTimeout(250);
    expect(await page.evaluate(() => window.workerMessageCount)).toBe(before);
  });

  test("dispose during asynchronous restart cannot resurrect a Worker", async ({ page }) => {
    await ready(page);
    const result = await page.evaluate(async () => {
      const runtime = "/runtime/index.js";
      const { mount } = await import(runtime);
      const root = document.createElement("div");
      document.body.append(root);
      let finish: (() => void) | undefined;
      let loads = 0;
      const handle: Handle = await mount(root, {
        moduleUrl: "/pkg/tui2web_example.js",
        persistence: {
          load: async () => { if (loads++ > 0) await new Promise<void>(resolve => { finish = resolve; }); return null; },
          save: async () => {}, clear: async () => {},
        },
      });
      const restart = handle.restart().then(() => "resurrected", () => "cancelled");
      await handle.dispose();
      finish!();
      const outcome = await restart;
      return { outcome, state: handle.status.state, terminals: root.querySelectorAll(".xterm").length };
    });
    expect(result).toEqual({ outcome: "cancelled", state: "disposed", terminals: 0 });
  });

test("host wakeups survive resize/snapshot and persistence failure is explicit", async ({ page }) => {
  await ready(page);
  const result = await page.evaluate(async () => {
    const runtime = "/runtime/index.js";
    const { mount } = await import(runtime);
    const source = `export default async function() {}
      export class App {
        constructor() { this.ticks = 0; }
        output(wakeAfterMs = null) { return JSON.stringify({
          version:1,frame:'tick '+this.ticks,snapshot:{version:1,directories:[''],files:[['ticks',[this.ticks]]]},
          exited:false,wakeAfterMs
        }); }
        start() { return this.output(150); }
        dispatch(json) { if(JSON.parse(json).input?.type === 'tick') this.ticks++; return this.output(); }
        free() {}
      }`;
    const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
    const element = document.createElement("div");
    element.style.cssText = "width:800px;height:300px";
    document.body.append(element);
    let rejectSave = false;
    const handle: Handle = await mount(element, {
      moduleUrl: url, wasm: new ArrayBuffer(0), persistence: {
        load: async () => null,
        clear: async () => {},
        save: async () => { if (rejectSave) throw new Error("deliberate quota failure"); },
      },
    });
    await handle.resize(50, 10);
    await handle.snapshot();
    await new Promise(resolve => setTimeout(resolve, 300));
    const ticks = (await handle.snapshot()).files[0][1][0];
    rejectSave = true;
    let error = "";
    try { await handle.send({ type: "tick" }); } catch (e) { error = String(e); }
    const state = handle.status.state;
    await handle.dispose();
    URL.revokeObjectURL(url);
    return { ticks, error, state };
  });
  expect(result.ticks).toBe(1);
  expect(result.error).toContain("deliberate quota failure");
  expect(result.state).toBe("error");
});

test("exit subscribers can restart without the old response terminating the new Worker", async ({ page }) => {
  await ready(page);
  const result = await page.evaluate(async () => {
    const handle = window.playground.handles[0];
    let restarting: Promise<void> | undefined;
    const unsubscribe = handle.subscribe(status => {
      if (status.state === "exited") restarting = handle.restart();
    });
    await handle.send({ type: "key", key: "q", code: "KeyQ", repeat: false,
      modifiers: { ctrl: true, alt: false, meta: false, shift: false } });
    await restarting;
    unsubscribe();
    return { status: handle.status.state, files: (await handle.snapshot()).files.length };
  });
  expect(result).toEqual({ status: "running", files: 2 });
});
