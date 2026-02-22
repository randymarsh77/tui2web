# tui2web

**A sandboxed shell environment for the web.**  
Compile your Rust [ratatui](https://ratatui.rs) TUI applications to WebAssembly and deploy them as interactive previews on any static web page.

---

## Overview

`tui2web` bridges the gap between native terminal UIs and the browser:

1. Your ratatui app is compiled to **`wasm32-unknown-unknown`** (no OS access – fully sandboxed).  
2. The `tui2web` library provides a **`WebBackend`** that implements ratatui's `Backend` trait and serialises every frame as ANSI escape codes.  
3. A tiny JavaScript glue layer writes those escape codes into **[xterm.js](https://xtermjs.org)**, giving users an interactive terminal in the browser.

```
┌─────────────────────────────────┐
│  Your ratatui app  (Rust/WASM)  │
│  ┌───────────┐  ┌─────────────┐ │
│  │  App logic│→ │ WebBackend  │ │
│  └───────────┘  └──────┬──────┘ │
└─────────────────────────┼───────┘
          ANSI frame (string)
┌─────────────────────────┼───────┐
│  Browser                ▼       │
│  ┌──────────────────────────┐   │
│  │        xterm.js          │   │
│  └──────────────────────────┘   │
└─────────────────────────────────┘
```

## Repository layout

```
tui2web/
├── crates/
│   └── tui2web/        # Core library – WebBackend + ANSI serialiser
├── example/            # Counter demo app (cdylib, compiled to WASM)
├── web/
│   ├── index.html      # Static web page template
│   └── main.js         # JS glue: loads WASM, drives xterm.js
├── build.sh            # One-command build script (uses wasm-pack)
└── Cargo.toml          # Workspace root
```

## Prerequisites

| Tool | Installation |
|------|-------------|
| Rust + Cargo | <https://rustup.rs> |
| `wasm32-unknown-unknown` target | `rustup target add wasm32-unknown-unknown` |
| `wasm-pack` | `cargo install wasm-pack` |
| Node.js (for local serving) | <https://nodejs.org> |

> **Tip:** If you use [Nix](https://nixos.org), run `nix develop` to enter a dev shell with all prerequisites pre-installed.

## Quick start

```bash
# 1. Build the WASM module and stage it in web/pkg/
./build.sh

# 2. Serve the web directory (any static server works)
npx serve web -l 8080

# 3. Open in the browser
open http://localhost:8080
```

The `--serve` flag combines steps 2 & 3:

```bash
./build.sh --serve
```

## Using tui2web in your own app

Add the library as a workspace dependency:

```toml
# your-app/Cargo.toml
[package]
name = "my-tui-app"
crate-type = ["cdylib"]

[dependencies]
tui2web = { path = "../crates/tui2web" }   # or publish to crates.io
ratatui  = { version = "0.26", default-features = false }
wasm-bindgen = "0.2"
```

Implement your app and export it via `wasm-bindgen`:

```rust
use tui2web::WebBackend;
use ratatui::Terminal;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct MyApp {
    terminal: Terminal<WebBackend>,
    // … your state …
}

#[wasm_bindgen]
impl MyApp {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u16, height: u16) -> MyApp {
        let backend = WebBackend::new(width, height);
        let terminal = Terminal::new(backend).unwrap();
        MyApp { terminal }
    }

    /// Call from JS on every animation frame / key event.
    pub fn tick(&mut self) -> bool {
        self.terminal.draw(|frame| {
            // render your widgets …
        }).unwrap();
        true // return false to stop the loop
    }

    /// Return the latest ANSI frame and write it to xterm.js.
    pub fn get_frame(&self) -> String {
        self.terminal.backend().get_ansi_output().to_string()
    }
}
```

Build with `wasm-pack` and load in the browser using the template in `web/`.

## Architecture details

### `WebBackend`

`WebBackend` implements ratatui's `Backend` trait:

- **`draw()`** – stores the diff of changed cells provided by `Terminal::draw`.  
- **`flush()`** – serialises the full cell buffer to a single ANSI escape-code string using absolute cursor positioning (`\x1b[row;colH`), true-colour codes (`\x1b[38;2;R;G;Bm`), and SGR attributes.  
- **`resize(width, height)`** – resizes the cell buffer in-place.  

### Sandboxing

Compiling to `wasm32-unknown-unknown` provides a natural sandbox:

- No filesystem access.  
- No network access.  
- No OS signals.  
- All terminal I/O is routed through the WASM ↔ JavaScript bridge.  

### Event handling

Keyboard events from xterm.js are forwarded to Rust as `KeyboardEvent.key` strings (e.g. `"j"`, `"ArrowUp"`, `"Escape"`) via `App::push_key()`. The app dequeues and processes them on the next `tick()`.

## Running the tests

```bash
cargo test -p tui2web
```

## License

MIT – see [LICENSE](LICENSE).
