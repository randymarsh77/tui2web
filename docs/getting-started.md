# Run the playground

[Project overview](../README.md)

Install Node 24, stable Rust with the `wasm32-unknown-unknown` target, and wasm-pack 0.15:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
npm ci
./build.sh --serve
```

Open <http://localhost:8080>. Alternatively, `nix develop` supplies the toolchain; then run `npm ci` and `./build.sh --serve`. The locked flake supports Intel macOS.

## Use the editor

The demo starts with seeded files and an empty directory.

| Action | Control |
|---|---|
| Switch files and save the previous file | Click a file, Tab, or Shift+Tab |
| Move the cursor | Arrow, Home, and End keys |
| Edit | Type, paste, compose text, or delete by grapheme |
| Save | Ctrl+S or Save |
| Create a file | Ctrl+N |
| Save and exit | Ctrl+Q |
| Transfer saved files | Export / Import |
| Reload saved files | Restart |
| Restore fixtures | Reset |

Snapshots contain committed virtual files, not unsaved editor text.

You can add independent or isolated editors. Each numbered instance has a distinct persistence namespace that survives reloads. Data belongs to the browser profile; storage eviction and private-browsing restrictions still apply. Export files you need to keep.

Editing needs no network after assets load. Reloading or mounting again requires cached or served assets. tui2web does not install a service worker.

To embed your own app, follow the [Rust](rust.md) and [browser](browser.md) guides.
