// This import intentionally resolves to the built public package, not private runtime modules.
import { mount, mountIsolated } from "@tui2web/runtime";
import type { Handle, Status } from "../runtime/index.js";
const handles: Handle[] = [];
let nextInstance = 1;
// Expose demo handles for browser integration tests and console exploration.
declare global { interface Window { playground: { handles: Handle[]; add(isolated?: boolean): Promise<void> } } }
async function add(isolated = false) {
  const index = nextInstance++;
  const template = document.querySelector<HTMLTemplateElement>("#editor-template")!;
  const article = template.content.firstElementChild!.cloneNode(true) as HTMLElement;
  const terminal = article.querySelector<HTMLElement>(".terminal")!;
  const status = article.querySelector<HTMLOutputElement>(".status")!;
  article.querySelector("h2")!.textContent = `Editor ${index} / ${isolated ? "isolated" : "trusted module"}`;
  document.querySelector("#editors")!.append(article);
  function report(next: Status) {
    status.textContent = next.error ? `${next.state}: ${next.error}` : next.state;
    status.classList.toggle("error", next.state === "error");
  }
  try {
    const handle: Handle = await (isolated ? mountIsolated : mount)(terminal, {
      moduleUrl: new URL("./pkg/tui2web_example.js", import.meta.url),
      persistence: `playground-${index}-${isolated ? "isolated" : "trusted"}`,
      onStatus: report,
    });
    handles.push(handle);
    const saveKey = { type: "key" as const, key: "s", code: "KeyS", repeat: false,
      modifiers: { ctrl: true, alt: false, meta: false, shift: false } };
    article.addEventListener("click", async event => {
      const button = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-action]");
      if (!button) return;
      button.disabled = true;
      try {
        switch (button.dataset.action) {
          case "save": await handle.send(saveKey); break;
          case "restart": await handle.restart(); break;
          case "reset": await handle.reset(); break;
          case "dispose": await handle.dispose(); break;
          case "export": {
            const url = URL.createObjectURL(new Blob([await handle.exportSnapshot()], { type: "application/json" }));
            const link = document.createElement("a");
            link.href = url; link.download = `tui2web-${index}.json`; link.click();
            setTimeout(() => URL.revokeObjectURL(url), 1000);
            break;
          }
        }
      } catch (error) { report({ state: "error", error: String(error) }); }
      finally { button.disabled = false; }
    });
    article.querySelector<HTMLInputElement>("input")!.addEventListener("change", async event => {
      const input = event.target as HTMLInputElement;
      const file = input.files?.[0];
      if (!file) return;
      try {
        if (file.size > 16 * 1024 * 1024) throw new Error("Import exceeds 16 MiB");
        await handle.importSnapshot(await file.text());
      } catch (error) { report({ state: "error", error: String(error) }); }
      input.value = "";
    });
  } catch (error) { report({ state: "error", error: String(error) }); }
}
window.playground = { handles, add };
document.querySelector("#add")!.addEventListener("click", () => { void add(); });
document.querySelector("#isolated")!.addEventListener("click", () => { void add(true); });
void add();
