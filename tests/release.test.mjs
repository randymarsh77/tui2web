import { test } from "node:test";
import assert from "node:assert/strict";
import { digest, inspectRemote, versions, main } from "../scripts/release.mjs";

const version = "0.1.0";
const npm = { name: "@tui2web/runtime", version };
const lock = { ...npm, packages: { "": npm } };
const packages = [{ name: "tui2web", version }, { name: "tui2web-example", version }];
const crateBytes = Buffer.from("immutable crate artifact");
const npmBytes = Buffer.from("immutable npm artifact");
const manifest = { version, crate: { sha256: digest(crateBytes, "sha256") }, npm: { integrity: `sha512-${digest(npmBytes, "sha512")}` } };

function registry({ crate = true, npm = true, checksum, integrity, yanked = false, corrupt = false, status, tarball } = {}) {
  return async url => {
    if (status) return new Response("", { status });
    if (url.includes("index.crates.io")) return new Response(crate ? JSON.stringify({
      name: "tui2web", vers: version, cksum: checksum ?? manifest.crate.sha256, yanked,
    }) + "\n" : "", { status: crate ? 200 : 404 });
    if (url.includes("static.crates.io")) return new Response(corrupt ? "corrupt" : crateBytes);
    if (url.endsWith(`/${version}`)) return new Response(npm ? JSON.stringify({
      name: "@tui2web/runtime", version,
      dist: { integrity: integrity ?? manifest.npm.integrity, tarball: tarball ?? "https://registry.npmjs.org/artifact.tgz" },
    }) : "", { status: npm ? 200 : 404 });
    return new Response(corrupt ? "corrupt" : npmBytes);
  };
}

test("release version/tag/name guards agree across both packages and lockfiles", () => {
  assert.equal(versions("v0.1.0", npm, lock, packages), version);
  assert.equal(versions(null, npm, lock, packages), version);
  for (const tag of ["v0.2.0", "master", "v0.1.0;echo bad", "v0.1.0-rc.1"]) {
    assert.throws(() => versions(tag, npm, lock, packages));
  }
  assert.throws(() => versions("v0.1.0", { ...npm, name: "renamed" }, lock, packages));
  assert.throws(() => versions("v0.1.0", npm, { ...lock, version: "0.2.0" }, packages));
  assert.throws(() => versions("v0.1.0", npm, lock, [{ name: "tui2web", version: "0.2.0" }]));
});

test("only verified byte-identical registry artifacts are accepted on retry", async () => {
  assert.deepEqual(await inspectRemote(manifest, registry()), { crate: true, npm: true });
  assert.deepEqual(await inspectRemote(manifest, registry({ crate: false, npm: false })), { crate: false, npm: false });
  assert.deepEqual(await inspectRemote(manifest, registry({ npm: false })), { crate: true, npm: false });
  assert.deepEqual(await inspectRemote(manifest, registry({ crate: false })), { crate: false, npm: true });
});

test("conflicts, yanks, network errors and corrupt downloads fail closed", async () => {
  for (const options of [
    { checksum: "wrong" }, { integrity: "wrong" }, { yanked: true }, { corrupt: true },
    { status: 401 }, { status: 403 }, { status: 429 }, { status: 500 },
    { tarball: "https://untrusted.example/package.tgz" },
  ]) await assert.rejects(inspectRemote(manifest, registry(options)));
  await assert.rejects(inspectRemote(manifest, async () => { throw new Error("network failed"); }));
  for (const body of ["", "not-json", "{}", "[]"]) {
    await assert.rejects(inspectRemote(manifest, async () => new Response(body)));
  }
});

test("publication refuses local and foreign-repository execution before credentials", async () => {
  const previous = process.env.GITHUB_ACTIONS;
  const previousRepo = process.env.GITHUB_REPOSITORY;
  delete process.env.GITHUB_ACTIONS;
  try {
    await assert.rejects(main("publish"), /only in GitHub Actions/);
    process.env.GITHUB_ACTIONS = "true";
    process.env.GITHUB_REPOSITORY = "someone/else";
    await assert.rejects(main("publish"), /owning repository/);
  }
  finally {
    if (previous === undefined) delete process.env.GITHUB_ACTIONS;
    else process.env.GITHUB_ACTIONS = previous;
    if (previousRepo === undefined) delete process.env.GITHUB_REPOSITORY;
    else process.env.GITHUB_REPOSITORY = previousRepo;
  }
});
