# Publishing packages

[Project overview](../README.md) · [Development](development.md)

The [Publish packages workflow](../.github/workflows/release.yml) publishes `tui2web` to crates.io and `@tui2web/runtime` to npm from a stable `vX.Y.Z` tag. Use this workflow, not local registry login.

It does not merge PRs, deploy a site, create tags, or create GitHub Releases. Downstream consumers should stay on validated Git pins until both registry artifacts are verified by a successful release receipt.

## Owner setup

Create the `package-release` GitHub Actions environment. Add these environment secrets, or repository Actions secrets with the same names:

| Secret | Authorization |
|---|---|
| `CARGO_REGISTRY_TOKEN` | crates.io token authorized to publish `tui2web`, including new-crate creation for the first release |
| `NPM_TOKEN` | npm granular token with read/write access to `@tui2web/runtime` or its scope, including package creation |

Confirm crate ownership and membership in the `tui2web` npm organization with package creation/write rights. An unclaimed package name does not grant scope ownership. The workflow does not rename packages.

For unattended npm publishing on a 2FA-protected account, enable the token's supported bypass-2FA capability; interactive OTPs are not supported. Check expiration and IP restrictions. The publish step maps `NPM_TOKEN` to `NODE_AUTH_TOKEN` and uses `--access public`.

Protect the environment with selected release tags (`v*`) and preferably required reviewers. Add a tag ruleset preventing updates and deletion of `v*` tags. Keep workflow and branch review protections enabled.

The workflow token has `contents: read` only. Registry credentials are passed only to the final publish step, not installation, builds, tests, PRs, or ordinary pushes. Authentication uses registry tokens, not OIDC trusted publishing or npm provenance.

## Create a release

1. Merge the reviewed implementation and workflow into `master`. The tag commit must be an ancestor of `origin/master`.
2. Set matching versions in `crates/tui2web/Cargo.toml`, `example/Cargo.toml`, `package.json`, and the root entry in `package-lock.json`. Update lockfiles with the package managers and merge the changes through a reviewed PR.
3. Once secrets, ownership, and protections are configured, tag the intended merged commit and push the tag. For version `0.1.0`:

```sh
git fetch origin master
git tag -a v0.1.0 origin/master -m "tui2web 0.1.0"
git push origin refs/tags/v0.1.0
```

The tag push starts publication. Ordinary pushes and PRs do not. Only stable `vX.Y.Z` tags are accepted; prereleases are not. Package names, versions, lockfile root version, tag commit, and workflow ref must agree.

## Build and artifact verification

The release job pins Rust 1.98.1, Node 24.19.0, and wasm-pack 0.15.0. It runs formatting, Clippy, Rust tests, WASM/site builds, TypeScript checks, JavaScript and release-guard tests, terminal parsing, and Chromium end-to-end tests.

It verifies the Cargo archive with all features, exercises Cargo's publish repack through `--dry-run`, packs npm assets without lifecycle scripts, and installs and typechecks the exact npm tarball in a clean consumer. These steps do not use registry credentials.

Before upload, Actions retains `release-artifacts-vX.Y.Z` for 90 days. It contains the `.crate`, `.tgz`, and `manifest.json` with commit, tag, version, and SHA-256/SHA-512 checksums. Cargo's embedded VCS record and npm's `dist/release.json` identify the same source commit.

npm publishes the tested tarball. Cargo publishes from the same clean checkout; the dry run checks that the repack is byte-identical to the tested archive, and publication verifies the archive again.

## Partial failure and retry

Cargo publishes first, then npm. If only Cargo succeeds, the release is partial. The step summary and `release-receipt-vX.Y.Z` artifact record verified packages and uncertain publication attempts.

Fix permissions, token expiry, registry access, or environment approval, then retry the same immutable tag:

```sh
gh workflow run release.yml --ref v0.1.0 -f release_tag=v0.1.0
gh run list --workflow release.yml --limit 5
```

Run the workflow from the tag itself, not a newer branch. Both registries are checked before either write. An existing version is skipped only if its registry checksum and downloaded archive match the expected local artifact byte-for-byte. Local archives must identify the expected source and tag.

HTTP errors other than an absent version, yanked Cargo versions, unexpected npm artifact origins, malformed responses, source mismatches, and differing archive bytes stop publication. Delayed registry visibility also fails verification and requires an exact-tag retry.

Do not move or recreate tags, overwrite versions, bypass checks, or switch to local publishing. Compare retained artifacts and registry state. If artifacts cannot be reproduced or an existing version differs, resolve the mismatch with maintainers and prepare a new version and reviewed tag.

After both packages are verified, share the successful run URL, source SHA, and receipt with downstream consumers before they switch to registry versions and rerun their checks.

A GitHub Release can be created manually from the published tag after success, for example with `gh release create v0.1.0 --verify-tag ...`. It is optional; the publication workflow lacks permission to create it.
