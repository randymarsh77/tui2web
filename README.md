# tui2web

Run Rust [Ratatui](https://ratatui.rs/) apps in the browser with WebAssembly. Build once, serve static files, and embed a terminal with a JavaScript or TypeScript API. No backend or PTY server required.

- Run application logic in a Web Worker, off the UI thread.
- Render terminal colors, styles, cursors, and resizable layouts through xterm.js.
- Handle keyboard, mouse, paste, and composed text input.
- Store virtual files in IndexedDB and import or export snapshots.
- Mount multiple independent apps, with optional sandboxed isolation.
- Test application logic natively using the same Rust interface.

Apps must use tui2web's event and filesystem interfaces. It does not run arbitrary native binaries or replace OS APIs.

## Get started

The demo is a file browser and editor. [Run it locally](https://github.com/randymarsh77/tui2web/blob/master/docs/getting-started.md), or integrate your own app with the [Rust guide](https://github.com/randymarsh77/tui2web/blob/master/docs/rust.md) and [browser API guide](https://github.com/randymarsh77/tui2web/blob/master/docs/browser.md).

## Documentation

- [Virtual files and persistence](https://github.com/randymarsh77/tui2web/blob/master/docs/filesystem.md)
- [Isolation and security limits](https://github.com/randymarsh77/tui2web/blob/master/docs/isolation.md)
- [Optional Git simulation](https://github.com/randymarsh77/tui2web/blob/master/docs/git.md)
- [Development](https://github.com/randymarsh77/tui2web/blob/master/docs/development.md)
- [Publishing packages](https://github.com/randymarsh77/tui2web/blob/master/docs/publishing.md)

MIT license. See [LICENSE](https://github.com/randymarsh77/tui2web/blob/master/LICENSE).
