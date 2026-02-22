/**
 * main.js – tui2web web frontend
 *
 * Loads the WASM module produced by `wasm-pack`, creates an xterm.js terminal,
 * and runs the ratatui example application inside it.
 */

import init, { App } from './pkg/tui2web_example.js';

const COLS = 80;
const ROWS = 24;

// ── localStorage-backed filesystem bridge ────────────────────────────────────
// These helpers allow the WASM-side MemoryFilesystem to persist its state
// across page reloads through the browser's localStorage.  The WASM app can
// call `snapshot()` and pass the result to `Tui2webFs.save()`, then on next
// load call `Tui2webFs.load()` and feed the entries into `restore()`.

const FS_STORAGE_KEY = 'tui2web_fs';

const Tui2webFs = {
  /**
   * Persist an array of `[path, base64Content]` pairs to localStorage.
   * @param {Array<[string, string]>} entries
   */
  save(entries) {
    try {
      localStorage.setItem(FS_STORAGE_KEY, JSON.stringify(entries));
    } catch (e) {
      console.warn('tui2web: failed to persist filesystem to localStorage', e);
    }
  },

  /**
   * Load previously-persisted filesystem entries.
   * @returns {Array<[string, string]>|null} entries or null if nothing saved.
   */
  load() {
    try {
      const raw = localStorage.getItem(FS_STORAGE_KEY);
      return raw ? JSON.parse(raw) : null;
    } catch (e) {
      console.warn('tui2web: failed to load filesystem from localStorage', e);
      return null;
    }
  },

  /** Remove persisted filesystem state. */
  clear() {
    localStorage.removeItem(FS_STORAGE_KEY);
  },
};

// Expose on window so WASM can call via js_sys / web_sys if needed.
window.Tui2webFs = Tui2webFs;

async function run() {
  const statusEl = document.getElementById('status');

  // ── Initialise the WASM module ─────────────────────────────────────────────
  try {
    await init();
  } catch (err) {
    statusEl.textContent = `Failed to load WebAssembly module: ${err.message}`;
    console.error(err);
    return;
  }

  // ── Set up xterm.js ────────────────────────────────────────────────────────
  const term = new Terminal({
    cols: COLS,
    rows: ROWS,
    fontFamily: '"Cascadia Code", "Fira Code", "JetBrains Mono", "Courier New", monospace',
    fontSize: 14,
    lineHeight: 1.1,
    theme: {
      background: '#1e1e2e',
      foreground: '#cdd6f4',
      cursor:     '#f5e0dc',
      selectionBackground: '#45475a',
    },
    cursorBlink: true,
    allowProposedApi: true,
  });

  const fitAddon = new FitAddon.FitAddon();
  term.loadAddon(fitAddon);

  const wrapper = document.getElementById('terminal-wrapper');
  term.open(wrapper);
  fitAddon.fit();

  // ── Create the Rust/WASM application ──────────────────────────────────────
  const app = new App(term.cols, term.rows);

  // ── Keyboard forwarding ────────────────────────────────────────────────────
  // xterm.js fires onKey with the DOM event; we forward KeyboardEvent.key
  // (e.g. "j", "ArrowUp", "Escape") to the Rust app.
  term.onKey(({ domEvent }) => {
    if (!app.should_quit()) {
      app.push_key(domEvent.key);
      // Render synchronously on input for immediate feedback.
      app.tick();
      term.write(app.get_frame());
    }
    domEvent.preventDefault();
  });

  // ── Initial render ─────────────────────────────────────────────────────────
  app.tick();
  term.write(app.get_frame());

  statusEl.textContent = 'Click the terminal and use the keyboard to interact.';

  // ── Animation loop ─────────────────────────────────────────────────────────
  // Keeps the app ticking at ~60 fps so timer-based TUI apps work correctly.
  // For the counter demo this is mostly idle work; no harm done.
  function renderLoop() {
    if (app.should_quit()) {
      term.write(
        '\r\n\x1b[32mApplication has quit.\x1b[0m Refresh the page to restart.\r\n',
      );
      statusEl.textContent = 'Application has quit. Refresh to restart.';
      return;
    }

    const running = app.tick();
    term.write(app.get_frame());

    if (running) {
      requestAnimationFrame(renderLoop);
    }
  }

  requestAnimationFrame(renderLoop);

  // ── Terminal resize handling ───────────────────────────────────────────────
  const resizeObserver = new ResizeObserver(() => {
    fitAddon.fit();
    app.resize(term.cols, term.rows);
    app.tick();
    term.write(app.get_frame());
  });
  resizeObserver.observe(wrapper);

  term.focus();
}

run().catch(console.error);
