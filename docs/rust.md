# Adapt a Rust app

[Project overview](../README.md) · [Browser integration](browser.md)

tui2web targets Ratatui 0.29 with `unicode-width` 0.2.0. The browser runtime uses xterm 5.5.0, fit 0.10.0, and Unicode11 addon 0.8.0. Keep the Cargo and npm lockfiles; changes to Ratatui or Unicode providers need the [terminal checks](development.md).

## Dependencies

For a local checkout, adjust the `tui2web` path to match your project:

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
tui2web = { path = "../tui2web/crates/tui2web", features = ["wasm"] }
ratatui = { version = "0.29", default-features = false }
wasm-bindgen = "0.2"
```

## Application interface

Implement `Application` and export it with `export_app!`. The adapter manages the terminal, protocol, resizing, snapshots, and WASM lifetime.

```rust
use ratatui::{Frame, widgets::Paragraph};
use tui2web::app::{Application, AppResult, Context, Input, Update};

pub struct Greeting(String);
impl Application for Greeting {
    fn init(_: &mut Context) -> AppResult<Self> {
        Ok(Self("Type here".into()))
    }
    fn update(&mut self, input: Input, _: &mut Context) -> AppResult<Update> {
        match input {
            Input::Text { text } | Input::Paste { text } => {
                self.0.push_str(&text);
                Ok(Update::render())
            }
            _ => Ok(Update::default()),
        }
    }
    fn render(&self, frame: &mut Frame, _: &Context) {
        frame.render_widget(Paragraph::new(self.0.as_str()), frame.area());
    }
}
tui2web::export_app!(Greeting);
```

Build with `wasm-pack build --target web`. The macro exports `App(initJson)`, `start()`, `dispatch(commandJson)`, and wasm-bindgen's `free()`. Native builds can use `Runner<A>` directly.

The [example app](../example/src/lib.rs) includes file editing, cursor placement, scrolling, modifiers, mouse input, and native tests.

## State and lifecycle

`Context` supplies a synchronous `MemoryFilesystem`, host epoch `now_ms`, terminal dimensions, string configuration, and a reproducible PRNG seeded by `crypto.getRandomValues`. The PRNG is not cryptographic; do not use it for secrets.

Set `Update.files_changed` when committing virtual files. The host acknowledges the update after persistence completes. Set `dirty` only when the display needs rendering.

Implement `initial_wake_after_ms` to request the first timer. Later updates use `wake_after_ms` for one-shot ticks with a 16 ms minimum; `None` cancels the previous timer. Background tabs can delay ticks.

App errors become visible runtime errors. Handle expected validation failures, such as invalid filenames, in the app UI instead. Exit calls `shutdown`, captures files, frees WASM, and terminates the Worker.

## Porting limits

tui2web does not intercept `std::fs`, crossterm event loops, threads, sockets, or native Git dependencies. Replace them with the application input and [filesystem](filesystem.md) interfaces. It is not a shell, browser-based Rust compiler, or WASI/OS compatibility layer.
