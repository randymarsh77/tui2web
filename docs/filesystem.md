# Virtual files and persistence

[Project overview](../README.md) · [Browser API](browser.md)

## Filesystem

Apps use the `Filesystem` interface; native filesystem APIs are not patched.

Paths are UTF-8 and slash-separated. Relative paths are root-relative. Normalization collapses `.` and duplicate separators, and resolves `..` without allowing escape above root. Controls, backslashes, and overlong paths are rejected.

Read errors distinguish invalid encoding from the wrong entry type. Root cannot be removed or moved. Rename is atomic, allows self-rename, requires an existing parent, and rejects descendant moves or an existing destination.

## Snapshots

Version 1 snapshots contain `version`, `directories`, and `files`. Files are byte arrays; directories include empty directories and the root `""`.

Restore validates canonical paths, duplicates, file/directory conflicts, parent directories, version, and quotas before replacing state.

| Resource | Limit |
|---|---|
| Total file contents | 1 MiB |
| Entries, including root | 4096 |
| Path | 1024 UTF-8 bytes |
| Imported JSON | 16 MiB |
| Edited file in the demo | 256 KiB |

Unversioned file-only and localStorage snapshots are not migrated automatically.

## Persistence

Pass a namespace string or a `SnapshotStore` implementation to `mount`. `indexedDbStore(namespace)` provides asynchronous IndexedDB storage.

Storage quota, blocked or denied access, malformed snapshots, and transaction failures report errors. There is no silent fallback. Custom stores must complete operations and preserve write ordering.

Different namespaces keep instances separate. Reusing a namespace shares last-writer-wins data; it does not provide collaboration. Reset clears only the selected namespace.

Snapshot imports restart the app from virtual files, not arbitrary WASM memory. Export important data: browser storage can be evicted or restricted in private browsing.
