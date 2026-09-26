# tui2web

**Embed adapted Rust/Ratatui applications as browser-local WASM playgrounds.**
Build Rust ahead of time, serve static assets, and mount an interactive terminal through a typed JavaScript API. Application logic runs in a dedicated Worker, not the UI thread. There is no runtime backend, PTY server, native subprocess or CDN dependency.

This is **not a shell, a Rust compiler in the browser, WASI/OS compatibility, or a way to run arbitrary native executables**. Existing `std::fs`, crossterm event loops, threads, sockets and native Git dependencies are not automatically intercepted. Applications must adopt the event and filesystem interfaces below.

## Run the playground

Prerequisites: Node 24, stable Rust with `wasm32-unknown-unknown`, and wasm-pack 0.15.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
npm ci
./build.sh --serve
# http://localhost:8080
```

Alternatively, `nix develop` supplies the toolchain, then run the last two commands. The flake is locked to a stable nixpkgs branch that still supports Intel macOS.

The demo is a real file browser/editor with seeded files and an empty directory. Click file names or use Tab/Shift+Tab to switch; switching saves the previous file. Type, paste or compose text, move with arrow/Home/End keys, delete by grapheme, and save with Ctrl+S (or the Save button). Ctrl+N creates a file; Ctrl+Q saves and exits. Export/import transfers a versioned JSON snapshot. Restart reloads saved virtual files; Reset replaces them with fixtures. Unsaved editor text is intentionally not in the filesystem snapshot.

Add a second independent editor or an isolated editor. Namespaces are stable across reloads and distinct per numbered demo instance. A browser profile owns its data; IndexedDB eviction/private-browsing restrictions still apply. Export important work. No network is needed for editing after assets load. Normal page reloads/new mounts still require assets from HTTP cache or your own offline asset cache; this project does not install a service worker.

## Consumer: Rust

Version pairing is deliberate: **Ratatui 0.29.0**, `unicode-width` 0.2.0, **xterm 5.5.0**, fit 0.10.0, Unicode11 addon 0.8.0. Ratatui 0.29 matches the real hunky consumer. Keep the Cargo/npm lockfiles. Upgrading Ratatui or Unicode providers requires rerunning the real-terminal tests.

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

Implement the native-testable contract; the adapter owns the terminal, protocol, resize, snapshots and WASM lifetime:

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

Build with `wasm-pack build --target web`. `export_app!` emits the stable `App(initJson)`, `start()`, `dispatch(commandJson)` and wasm-bindgen `free()` surface. For non-WASM builds use `Runner<A>` directly. `example/src/lib.rs` demonstrates file editing, cursor placement, scrolling, modifiers, mouse and native tests without hand-written JS/WASM adapter boilerplate.

`Context` provides synchronous `MemoryFilesystem`, host epoch `now_ms`, dimensions, string configuration and a reproducible non-cryptographic PRNG seeded by `crypto.getRandomValues`. Do not use that PRNG for secrets. Use `Update { files_changed: true, .. }` when committing virtual files. The host acknowledges that update only after persistence completes. Return `dirty` only when rendering is needed. To request a first timer implement `initial_wake_after_ms`; subsequent `Update.wake_after_ms` requests a one-shot tick (minimum 16 ms). `None` cancels the previous timer. Timers can be delayed in background tabs; they are not real-time. Errors returned by the app become visible runtime errors. Expected editor validation errors should instead become app UI messages. Exit runs `shutdown`, captures files, frees WASM and terminates the Worker.

## Consumer: JavaScript / TypeScript

This repository builds the `@tui2web/runtime` package. `npm run build` produces `dist/` with bundled JS, declarations, CSS, Worker and isolated-frame entrypoints. Until the first verified registry release, use an immutable Git dependency, a local file dependency or copy `dist/` to your static site. Git installs build assets through npm's `prepare` lifecycle.

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
  // Keep terminal to call its public methods; await terminal.dispose() on unmount.
</script>
```

Keep `index.js`, `worker.js`, `frame.js` and `style.css` together. Bundlers must copy the worker assets or set `workerUrl` (trusted mode) / `runtimeBaseUrl` (isolated mode). Assets need HTTP(S), not `file://`. The module must be a worker-compatible wasm-bindgen `--target web` ES module, exporting `App`; no DOM imports. A `wasm` ArrayBuffer can replace WASM fetching in trusted mode. Normal asset loads use omitted credentials and no referrer; same-origin module imports retain normal browser module semantics.

| Handle | Behavior |
|---|---|
| `status`, `subscribe(listener)` | `starting`, `running`, `exited`, `error`, `disposed`; unsubscribe function returned |
| `send(input)` | Structured input; promise resolves after output is parsed and any snapshot is stored |
| `resize(columns, rows)` | Ordered application and terminal resize; ResizeObserver also fits the container |
| `snapshot()`, `exportSnapshot()` | Current committed virtual files, as a typed value or JSON |
| `importSnapshot(json)` | Validate, restart from imported files, persist on successful initialization |
| `restart()` | Terminate current worker and reload persisted files (last acknowledged snapshot without storage) |
| `reset()` | Clear this namespace, restart from app fixtures |
| `focus()` | Focus input |
| `dispose()` | Graceful shutdown when running, then unconditional Worker/listener/observer/terminal cleanup |

Handle operations return rejecting promises on errors; UI input errors are also reported through status. Startup failure rejects `mount` and cleans up its terminal. A timeout or trap terminates the Worker; `restart()` recovers it. Disposing during restart cancels startup rather than resurrecting the Worker. Concurrent restart/reset/import operations are rejected. Explicit resize can later be superseded by a container resize.

### Input and output contract

`runtime/protocol.ts` and `crates/tui2web/src/app.rs` define version **1**. Initialization includes dimensions, snapshot, clock, entropy seed and configuration. Each ordered request has a protocol version and sequence ID. Responses contain optional ANSI output, optional snapshot, exit state and a wake delay, or an explicit error.

Input separates **text** (xterm's committed text/IME stream) and **paste** (clipboard text) from semantic keys with `key`, `code`, `repeat`, and Ctrl/Alt/Shift/Meta flags. Key events are for navigation/shortcuts; printable input is not guessed from `KeyboardEvent.key`. Mouse events use zero-based cells, DOM button numbers, down/up/drag/wheel kinds and modifiers; focus is separate. Shift+drag is left to browser selection. Only in-terminal mouse coordinates are forwarded; there is no global pointer capture. Mobile keyboard, platform IMEs, clipboard permission rules and browser-reserved shortcuts differ by browser. Ctrl/Meta+V, L, R, T, W, selected-copy and developer shortcuts remain browser-owned. Some OS/browser shortcuts cannot be captured; use UI buttons where provided.

One request/output is in flight. The next request is sent only after xterm's **write callback** and snapshot acknowledgement, not just after `postMessage`. The queue is bounded to 128 waiting messages / 1 MiB serialized input, text/paste to 64 KiB UTF-8, terminal size to 300x120, frame length to 4 MiB JS characters. Overflow fails explicitly rather than silently dropping keystrokes. App output is capped at 24 MiB JSON. Full frames are serialized only when dirty; there is no idle requestAnimationFrame loop. Deadline defaults to 10 seconds (configurable 100..120000 ms) and includes rendering/persistence acknowledgement. These are protocol budgets, **not hard browser process memory quotas**.

### Terminal fidelity

Full dirty frames reset styles and clear the display, skip Ratatui's wide-character continuation cells, suppress standalone zero-width/control symbols, and disable autowrap while writing the bottom-right cell. Cursor visibility/position, true/indexed colors, hidden and other Ratatui modifiers, clearing, and shrinking/growing are preserved. VT100 and headless xterm tests inspect actual screen cells, not just ANSI substrings.

Supported width assumptions are non-CJK ambiguous-width characters as one column, common CJK as two, and combining marks attached to a base character. xterm uses its Unicode11 width provider. **Do not assume reliable layout for ZWJ emoji sequences, flags, newly assigned Unicode characters or terminal-specific ambiguous-width settings**: Ratatui/unicode-width and xterm can disagree. Those need a coordinated grapheme/Unicode-provider upgrade; they are not claimed as supported by this release. The editor preserves their UTF-8 bytes, but visual layout may differ. Missing system-font glyphs are outside the terminal serializer.

## Virtual filesystem and persistence

No native APIs are patched. Replace filesystem dependencies with `Filesystem`; it uses UTF-8 slash-separated paths, treats relative paths as root-relative, collapses `.`/duplicate separators and resolves `..` without allowing escape above root. Controls, backslashes and overlong paths are rejected. Read errors distinguish invalid encoding from wrong entry type. Root cannot be removed/moved. Rename is atomic, permits self-rename, rejects moving into descendants or overwriting any existing destination, and requires an existing parent.

Snapshots preserve files as byte arrays **and all directories including empty ones**, with root `""`, `version: 1`, `directories` and `files`. Restore validates canonical paths, duplicates, file/directory conflicts, parents, version and quotas before replacing state. Limits are 1 MiB total file contents, 4096 entries (including root), 1024 UTF-8 bytes per path, and 16 MiB imported JSON. The demo additionally limits an edited file to 256 KiB. Legacy unversioned file-only/localStorage snapshots are not automatically migrated.

`indexedDbStore(namespace)` persists asynchronously; pass its namespace string or a `SnapshotStore` implementation to `mount`. Quota, blocked/denied storage, malformed snapshots and transaction failures are explicit errors, never a fallback to apparent success. Different namespaces isolate instances; **reusing a namespace intentionally shares last-writer-wins data**, not a collaborative filesystem. Custom stores must complete operations and preserve write ordering. Snapshot imports restart app state rather than restoring arbitrary WASM memory. Reset affects only the selected namespace.

## Isolation: two different trust boundaries

### `mount`: trusted modules only

A same-origin Worker keeps computation off the UI thread but **is not a security boundary for imported JavaScript**. Glue can use browser networking and same-origin storage or other Worker APIs. Only mount modules you trust in this mode. WASM has only the imports supplied by its glue; compilation to WASM alone is not a promise of no network access.

### `mountIsolated`: restricted guest Worker in an opaque-origin frame

```js
import { mountIsolated } from "./runtime/index.js";
const terminal = await mountIsolated(container, {
  moduleUrl: new URL("./pkg/my_app.js", document.baseURI),
  persistence: "isolated:my-document",
});
```

The host first fetches **only the configured app glue and WASM assets plus its own trusted runtime assets**, with size limits. These fetches are a deliberate provisioning capability: the caller chooses and trusts those URLs. Do not let arbitrary guest input control host asset URLs or `runtimeBaseUrl`; use an application allowlist when exposing uploads/build selections.

The frame is `sandbox="allow-scripts"` **without `allow-same-origin`**. Only trusted, bundled host code enters its document. App glue is never inserted into HTML or executed in that document; it is sent to a dedicated classic blob Worker, which dynamically imports a blob ES module. Classic loading is necessary for Chromium's opaque-origin Worker support. Bundle app glue's dependencies into that single ES module; relative/import-map/remote dependencies are not supported in isolated mode.

The frame and its inherited Worker CSP deny all network destinations: `default-src 'none'`, `connect-src 'none'`, `script-src 'unsafe-inline' 'wasm-unsafe-eval' blob:`, `worker-src blob:`, and no images, fonts, forms or base URL overrides. There is **no asset-origin network exception inside the guest**. `unsafe-inline` boots trusted frame code; `unsafe-eval` is not granted. Browser tests execute denied fetch, WebSocket, dynamic HTTP imports, `importScripts` and IndexedDB **inside the guest Worker**, as well as checking iframe parent-DOM/network/storage denial.

Persistence stays in the parent through a transferred, per-frame `MessagePort`, not window-wide storage messages. Initial connection checks the sending parent Window and accepts one port only. Guest Workers do not receive that port. The parent closes over one store/namespace, accepts only load/save/clear, revalidates snapshot schema/quotas, and never honors a guest-supplied namespace or filesystem path on the host. Only virtual-file bytes cross this boundary. Disposal removes the frame/ports and frees blob URLs.

This is a practical static-host option, **not an adversarial multi-tenant compute service**. There are no portable hard CPU/memory limits; a guest can exhaust its browser process, allocate before validation or spawn blob workers. Watchdogs cannot guarantee recovery from renderer/process OOM. Browser bugs, side channels and untrusted extensions are not addressed. A stalled guest Worker is restartable; if the trusted frame/channel itself stalls, its timeout removes it and you must remount. Embedding sites must permit the necessary sandbox/srcdoc/blob/WASM CSP capabilities; a stricter ancestor policy may prevent startup, which is reported rather than silently falling back to trusted mode. Automated browser support is currently **Chromium**; other engines and actual OS IMEs require separate qualification.

## Optional Git simulation

`tui2web` defaults to no Git. Enable its `git` Cargo feature for `InMemoryGitRepository`, the seeded simulation used by the adapted hunky review UI. It has linear in-memory history, counter-based pseudo-identifiers and text diffs, not Git objects, real hashes, branches, remotes, `.git` compatibility or transport. The core runtime/file editor does not require it.

The repository owns independent HEAD, index and working trees. `filesystem_mut()` changes only the working filesystem; `Clone` is a deep copy. `GitRepository` provides whole-file stage/unstage, index-to-worktree and HEAD-to-index diffs, commit and history. `review()` additionally returns HEAD-to-worktree `ReviewFile`/`ReviewHunk`/`ReviewLine` values, with three context lines, staged flags and opaque `ChangeId` selections. `ReviewSource::IndexOnly` exposes staged edits that are no longer present in the working tree, including a worktree reverted to HEAD. Include the source in UI hunk/cache identities.

`stage_change`, `unstage_change`, `discard_change` and atomic `toggle_hunk` operate on those IDs rather than mutable display offsets. IDs anchor deletions to HEAD lines and insertions to HEAD gaps, text and duplicate occurrence; retained index provenance preserves partial selections. Adjacent distinct inserted lines do not invalidate existing insertion IDs. Reordered identical lines use deterministic canonical occurrence identities. A changed HEAD or a removed/replaced operation rejects stale selections. Staging a new insertion supersedes obsolete index-only insertions at that same HEAD gap, while preserving staged groups elsewhere. Discard reverses a selected HEAD-to-worktree edit without changing the index. This is a deliberately specified simulation, not libgit2's patch engine.

Line review requires UTF-8 and retains LF/no-final-LF data. Empty-file adds/removes use a selectable `[empty file]` metadata line. Whole-file operations also support binary bytes; requesting text review for a changed binary file returns `GitError::BinaryFile`. Quadratic LCS tables are bounded; large diffs use replacement edits rather than unbounded allocation.

`RepositorySnapshot` captures version 1, the complete working filesystem (including empty directories), HEAD, index, selection provenance, linear commit history and counter. It round-trips through serde and `restore` validates atomically, including history/HEAD consistency and edit provenance. Each tree has the filesystem's quotas; snapshots are limited to 16 MiB serialized JSON and 64 commits. Commits that exceed those limits fail without mutation. This repository snapshot is separate from the runtime's filesystem snapshot; apps may serialize it into their own virtual files or implement an explicit restore policy. It does not silently persist or restore repository state.

## Verification and layout

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

`crates/tui2web` contains the backend, app adapter and memory filesystem; `runtime/` the package; `example/` the adapted Rust app; `web/` the static demo; `tests/` and `scripts/check-terminal.mjs` exercise the public API. `.github/workflows/ci.yml` runs the checks above, verifies publishable artifacts without uploading to registries, and uploads the static site artifact on PRs and pushes to the actual default branch, `master`. Ordinary CI does not publish or deploy.

## Publishing through GitHub Actions

**Publication happens only through `.github/workflows/release.yml` (`Publish packages`), never through local registry login.** The workflow publishes **`tui2web` on crates.io** and **`@tui2web/runtime` on npm**, both with the exact version selected by a stable `vX.Y.Z` tag. It does not merge PRs, deploy a website, create tags, or create GitHub Releases. Hunky remains on its validated Git-pinned review candidate until **both** registry artifacts are verified by a successful workflow receipt.

### One-time owner setup

Create the GitHub Actions environment **`package-release`** in this repository. Add these environment secrets (repository Actions secrets with the same names also work):

| Secret | Required authorization |
|---|---|
| `CARGO_REGISTRY_TOKEN` | A crates.io API token for an account authorized to publish `tui2web`. The first release needs permission to publish a new crate; later releases need publish/update permission for this crate. Existing crate ownership must belong to the intended account/team. |
| `NPM_TOKEN` | A supported npm **granular access token** with read/write package permissions for `@tui2web/runtime` (or the `@tui2web` scope), including first-package creation. For unattended CI on a 2FA-protected account, enable the token's supported **bypass 2FA** capability. Do not require an interactive OTP. Respect any token IP restrictions/expiration and rotate as needed. |

The npm account must belong to the **`tui2web` npm organization** with package creation/write rights. An unclaimed package name does **not** establish ownership of its scope. Resolve namespace ownership before triggering; the workflow never renames packages. It publishes npm with `--access public`. `actions/setup-node` consumes `NODE_AUTH_TOKEN`, which the publish step maps from the GitHub secret **`NPM_TOKEN`**; do not create a differently named secret by accident.

Protect the `package-release` environment with selected release tags (`v*`) and, preferably, required reviewers. Configure a tag ruleset preventing updates/deletion of `v*` tags so release refs remain immutable. Keep GitHub's workflow/branch review protections enabled. The workflow token has only `contents: read`; no write or OIDC permission is required. Registry tokens are passed only to the final publish step, not dependency installation, build scripts, tests, PRs or ordinary pushes. It uses token authentication rather than npm trusted publishing/provenance; enabling OIDC is a separate future policy change.

### First release and later versions

1. Merge the reviewed release workflow and validated implementation into `master`. Do not tag a feature-branch-only commit: the workflow requires the tag commit to be an ancestor of `origin/master`.
2. Set the **same version** in `crates/tui2web/Cargo.toml`, `example/Cargo.toml`, root `package.json`, and the root entry in `package-lock.json`; update/commit `Cargo.lock` and `package-lock.json` with the package managers. The initial candidate is `0.1.0`. Commit version changes through the normal reviewed PR workflow.
3. After the secrets/ownership/protections above are configured and the intended commit is merged, create and push the immutable tag:

```sh
git fetch origin master
git tag -a v0.1.0 origin/master -m "tui2web 0.1.0"
git push origin refs/tags/v0.1.0
```

Do **not** execute those commands before secrets are configured. The tag push is the publication trigger; ordinary pushes and PRs never publish. Only stable `vX.Y.Z` tags are accepted (no prerelease suffixes). Both package names, all package versions, the lockfile root version, the checked-out tag commit, and the workflow ref must agree.

The release job pins Rust **1.98.1**, Node **24.19.0** and wasm-pack **0.15.0**, then runs formatting, Clippy, Rust tests, the WASM/site build, TypeScript checks, JavaScript/release-guard tests, real-terminal parsing and Chromium end-to-end tests. It builds/verifies the Cargo archive with all features, exercises Cargo's publish repack path using **`--dry-run`**, packs npm's already-built assets without lifecycle scripts, and installs/typechecks that exact npm tarball in a clean consumer. No credentials are needed for these steps.

Before any upload to a registry, `release-artifacts-vX.Y.Z` is retained in Actions for 90 days: the exact `.crate` and `.tgz`, plus `manifest.json` containing the commit, tag, version and SHA-256/SHA-512 checksums. Cargo's embedded VCS record and npm's `dist/release.json` identify the same source commit. npm publishes the tested tarball directly. Cargo publishes from the same clean immutable checkout; the dry run first proves the repack is byte-identical to the tested archive, and publication verifies the resulting archive again.

### Partial failure and safe retry

Cargo publishes first, then npm. A successful Cargo upload followed by an npm failure is a **partial release**, not overall success. The step summary and `release-receipt-vX.Y.Z` artifact record which packages were publicly verified and which publication attempts remain uncertain.

Fix secret permissions, token expiry, transient registry access or environment approval, then retry the **same immutable tag**:

```sh
gh workflow run release.yml --ref v0.1.0 -f release_tag=v0.1.0
gh run list --workflow release.yml --limit 5
```

The dispatch must run the workflow **from the tag itself**, not a newer branch, so it cannot silently substitute a different publisher/build script. It rebuilds with the locked source/toolchain. Both registries are checked before either write: an existing version is skipped **only** when its registry checksum and downloaded archive match the expected local artifact byte-for-byte. The local archives must also identify the expected source/tag. No "version exists" error is blindly ignored.

HTTP errors other than an absent version, yanked Cargo versions, unexpected npm artifact origins, malformed registry responses, source mismatches, or different archive bytes **fail closed**. A registry may take time to expose an upload publicly; an uncertain verification fails the run and requires the exact-tag retry. Do not delete/recreate/move a tag, overwrite a published version, disable the checks, or switch to local publishing to recover. Compare the retained artifacts/manifest and registry state. If identical artifacts cannot be reproduced or the existing version genuinely differs, resolve that explicitly with maintainers and prepare a **new version and new reviewed tag** instead.

After both packages are verified, send the successful run URL, immutable source SHA and receipt to downstream consumers. Hunky can then switch to the actual published versions and re-run its clean consumer/site checks. An optional GitHub Release can be created manually from that **already published tag** (`gh release create v0.1.0 --verify-tag ...`) after success; it is not required and this workflow intentionally lacks permission to create it.

MIT. See [LICENSE](LICENSE).
