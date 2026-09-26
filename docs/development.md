# Development

[Project overview](../README.md) · [Toolchain setup](getting-started.md)

## Checks

After installing the toolchain and running `npm ci`:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
./build.sh
npm run typecheck
npm test
npm run test:terminal
npx playwright install chromium
npm run test:e2e
```

## Layout

| Path | Contents |
|---|---|
| `crates/tui2web/` | Rust backend, application adapter, virtual filesystem, and optional Git simulation |
| `runtime/` | Browser runtime package |
| `example/` | Rust file editor |
| `web/` | Static demo |
| `tests/` | Runtime, browser, and release tests |
| `scripts/check-terminal.mjs` | Terminal screen-cell checks |

[CI](../.github/workflows/ci.yml) runs the checks, verifies package artifacts without uploading them to registries, and uploads the static site artifact on PRs and pushes to `master`. It does not publish or deploy.

See [publishing](publishing.md) for registry releases.
