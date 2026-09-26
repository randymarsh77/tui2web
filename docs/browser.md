# Browser integration

[Project overview](../README.md) · [Rust integration](rust.md)

`@tui2web/runtime` provides the browser host. `npm run build` produces bundled JavaScript, declarations, CSS, Worker, and isolated-frame entrypoints in `dist/`.

Until a registry release is verified, use an immutable Git dependency, a local file dependency, or copy `dist/` to your static site. Git installs build assets through npm's `prepare` lifecycle.

## Mount an app

Use `mount` only for trusted modules. For restricted guest execution, see [isolation](isolation.md).

```html
<link rel="stylesheet" href="./runtime/style.css">
<div id="terminal" style="height:400px"></div>
<script type="module">
  import { mount } from "./runtime/index.js";
  const terminal = await mount(document.querySelector("#terminal"), {
    moduleUrl: new URL("./pkg/my_app.js", document.baseURI),
    // wasmUrl defaults to my_app_bg.wasm next to my_app.js
    persistence: "my-playground:document-123",
    config: { greeting: "Welcome" },
    onStatus: ({ state, error }) => console.log(state, error ?? ""),
  });
  // Await terminal.dispose() when unmounting.
</script>
```

Keep `index.js`, `worker.js`, `frame.js`, and `style.css` together. Bundlers must copy the Worker assets or set `workerUrl` for trusted mode and `runtimeBaseUrl` for isolated mode.

Serve assets over HTTP(S), not `file://`. The app must be a worker-compatible wasm-bindgen `--target web` ES module exporting `App`, without DOM imports. Trusted mode also accepts a `wasm` ArrayBuffer instead of fetching WASM. Asset fetches omit credentials and referrers; same-origin module imports follow browser module semantics.

## Handle API

| Member | Behavior |
|---|---|
| `status`, `subscribe(listener)` | Report `starting`, `running`, `exited`, `error`, or `disposed`; subscription returns an unsubscribe function |
| `send(input)` | Resolve after output is parsed and any snapshot is stored |
| `resize(columns, rows)` | Resize the app and terminal in order; ResizeObserver also fits the container |
| `snapshot()`, `exportSnapshot()` | Return committed virtual files as a typed value or JSON |
| `importSnapshot(json)` | Validate, restart with imported files, and persist after successful initialization |
| `restart()` | Replace the Worker and reload persisted files, or the last acknowledged snapshot without storage |
| `reset()` | Clear this namespace and restart from app fixtures |
| `focus()` | Focus terminal input |
| `dispose()` | Shut down a running app, then clean up the Worker, listeners, observers, and terminal |

Async operations reject on errors; UI input errors also appear in status. Startup failure rejects `mount` and cleans up the terminal. Timeouts and traps terminate the Worker; `restart()` recovers it.

Concurrent restart, reset, and import operations are rejected. Disposal during restart cancels startup. Container resizing can supersede an explicit resize.

## Input and output

[protocol.ts](../runtime/protocol.ts) and [app.rs](../crates/tui2web/src/app.rs) define protocol version 1. Initialization supplies dimensions, snapshot, clock, entropy seed, and configuration. Ordered requests carry a version and sequence ID. Responses contain optional ANSI output, snapshot, exit state, and wake delay, or an error.

Text and paste are separate from semantic key events. Text comes from xterm's committed text/IME stream, not guesses from `KeyboardEvent.key`. Keys carry `key`, `code`, `repeat`, and Ctrl/Alt/Shift/Meta flags.

Mouse events use zero-based terminal cells, DOM button numbers, down/up/drag/wheel kinds, and modifiers. Focus has its own event. Only in-terminal coordinates are forwarded; there is no global pointer capture. Shift+drag remains browser selection.

Ctrl/Meta+V, L, R, T, W, selected-copy, and developer shortcuts remain browser-owned. Mobile keyboards, IMEs, clipboard permissions, and reserved shortcuts vary by browser and OS. Provide UI controls for actions whose shortcuts may be unavailable.

## Ordering and limits

Only one request is in flight. The next waits for xterm's write callback and snapshot persistence, not just `postMessage`. Dirty frames are serialized in full; there is no idle animation loop.

| Resource | Limit |
|---|---|
| Waiting input | 128 messages / 1 MiB serialized |
| Text or paste | 64 KiB UTF-8 |
| Terminal size | 300 columns by 120 rows |
| Frame | 4 MiB JavaScript characters |
| App output | 24 MiB JSON |
| Request deadline | 10 seconds by default; configurable from 100 to 120000 ms |

Overflow reports an error rather than dropping input. Deadlines include rendering and persistence acknowledgement. These limits are protocol budgets, not browser-process memory quotas.

## Terminal rendering

Full frames reset styles, clear the display, skip wide-character continuation cells, suppress standalone zero-width/control symbols, and disable autowrap while writing the bottom-right cell. Rendering preserves cursor position and visibility, true/indexed colors, Ratatui modifiers, and terminal resizing. VT100 and headless xterm tests inspect screen cells.

Supported widths are one column for non-CJK ambiguous-width characters, two for common CJK, and combining marks attached to a base character. xterm uses its Unicode11 provider.

ZWJ emoji, flags, newly assigned Unicode characters, and terminal-specific ambiguous-width settings can disagree between Ratatui and xterm. Their layout is not supported reliably without a coordinated Unicode/grapheme-provider upgrade. The editor preserves UTF-8 bytes even when display width differs. Font glyph availability depends on the host system.
