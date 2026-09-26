import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const CRATE = "tui2web";
const NPM = "@tui2web/runtime";
const OUT = ".release";
const encoder = value => Buffer.from(JSON.stringify(value, null, 2) + "\n");
export const digest = (bytes, algorithm) => createHash(algorithm).update(bytes).digest(algorithm === "sha512" ? "base64" : "hex");
const capture = (command, args) => execFileSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] });
const run = (command, args) => execFileSync(command, args, { stdio: "inherit" });
const json = async path => JSON.parse(await readFile(path, "utf8"));

export function versions(tag, npm, lock, packages) {
  const version = npm.version;
  assert.match(version, /^\d+\.\d+\.\d+$/, "Only stable x.y.z releases are supported");
  assert.equal(npm.name, NPM, "npm package name changed");
  assert.equal(lock.name, NPM);
  assert.equal(lock.version, version);
  assert.equal(lock.packages[""].name, NPM);
  assert.equal(lock.packages[""].version, version, "npm lockfile version drift");
  assert.equal(packages.find(p => p.name === CRATE)?.version, version, "Cargo/npm version mismatch");
  assert.equal(packages.find(p => p.name === "tui2web-example")?.version, version, "Example version mismatch");
  if (tag !== null) assert.equal(tag, `v${version}`, "Release tag must exactly match all package versions");
  return version;
}

async function identity(release) {
  const tag = release ? process.env.RELEASE_TAG : null;
  if (release) assert.match(tag ?? "", /^v\d+\.\d+\.\d+$/, "RELEASE_TAG must be an exact stable vX.Y.Z tag");
  const metadata = JSON.parse(capture("cargo", ["metadata", "--locked", "--no-deps", "--format-version=1"]));
  const version = versions(tag, await json("package.json"), await json("package-lock.json"), metadata.packages);
  const source = capture("git", ["rev-parse", "HEAD"]).trim();
  assert.match(source, /^[0-9a-f]{40}$/);
  assert.equal(capture("git", ["status", "--porcelain"]).trim(), "", "Build/release requires a clean tracked checkout");
  if (release) {
    assert.equal(process.env.GITHUB_REF, `refs/tags/${tag}`, "Run the release workflow from the exact tag, including dispatch retries");
    assert.equal(capture("git", ["rev-parse", `refs/tags/${tag}^{commit}`]).trim(), source, "Checkout is not the requested tag");
    run("git", ["merge-base", "--is-ancestor", source, "origin/master"]);
  }
  return { schema: 1, version, source, tag };
}

function archiveJson(archive, entry) {
  return JSON.parse(capture("tar", ["-xOf", archive, entry]));
}

async function verifyFiles(manifest) {
  const { version, source, tag } = manifest;
  assert.match(version, /^\d+\.\d+\.\d+$/);
  assert.match(source, /^[0-9a-f]{40}$/);
  assert.equal(manifest.crate.file, `${CRATE}-${version}.crate`);
  assert.equal(manifest.npm.file, `tui2web-runtime-${version}.tgz`);
  const cratePath = `${OUT}/${manifest.crate.file}`;
  const npmPath = `${OUT}/${manifest.npm.file}`;
  assert.equal(digest(await readFile(cratePath), "sha256"), manifest.crate.sha256, "Local Cargo artifact changed");
  assert.equal(`sha512-${digest(await readFile(npmPath), "sha512")}`, manifest.npm.integrity, "Local npm artifact changed");
  const vcs = archiveJson(cratePath, `${CRATE}-${version}/.cargo_vcs_info.json`);
  assert.equal(vcs.git.sha1, source, "Cargo archive source SHA mismatch");
  assert.notEqual(vcs.git.dirty, true, "Cargo archive was built from dirty source");
  assert.equal(vcs.path_in_vcs, "crates/tui2web");
  const stamp = archiveJson(npmPath, "package/dist/release.json");
  assert.deepEqual(stamp, { schema: 1, version, source, tag }, "npm archive source identity mismatch");
  const npm = archiveJson(npmPath, "package/package.json");
  assert.equal(npm.name, NPM);
  assert.equal(npm.version, version);
  const entries = new Set(capture("tar", ["-tzf", npmPath]).trim().split("\n"));
  for (const entry of ["LICENSE", "README.md", "dist/index.js", "dist/index.d.ts", "dist/worker.js",
    "dist/frame.js", "dist/style.css", "dist/licenses/xterm.txt", "dist/licenses/addon-fit.txt",
    "dist/licenses/addon-unicode11.txt"]) {
    assert.ok(entries.has(`package/${entry}`), `Missing npm artifact file: ${entry}`);
  }
  capture("tar", ["-xOf", cratePath, `${CRATE}-${version}/LICENSE`]);
  capture("tar", ["-xOf", cratePath, `${CRATE}-${version}/README.md`]);
}

async function packageArtifacts(release) {
  const build = await identity(release);
  await mkdir(OUT, { recursive: true });
  run("cargo", ["package", "--locked", "--all-features", "-p", CRATE]);
  const crateFile = `${CRATE}-${build.version}.crate`;
  const before = digest(await readFile(`target/package/${crateFile}`), "sha256");
  // cargo publish repackages source. Exercise that exact path without credentials/upload first.
  run("cargo", ["publish", "--dry-run", "--locked", "--all-features", "--no-verify", "-p", CRATE]);
  assert.equal(digest(await readFile(`target/package/${crateFile}`), "sha256"), before, "Cargo publish repack differs from tested artifact");
  await copyFile(`target/package/${crateFile}`, `${OUT}/${crateFile}`);
  await writeFile("dist/release.json", encoder(build));
  const [packed] = JSON.parse(capture("npm", ["pack", "--ignore-scripts", "--json", "--pack-destination", OUT]));
  const manifest = {
    ...build,
    crate: { file: crateFile, sha256: before },
    npm: { file: packed.filename, integrity: `sha512-${digest(await readFile(`${OUT}/${packed.filename}`), "sha512")}` },
  };
  await verifyFiles(manifest);
  await writeFile(`${OUT}/manifest.json`, encoder(manifest));
  await rm(`${OUT}/consumer`, { recursive: true, force: true });
  await mkdir(`${OUT}/consumer`, { recursive: true });
  await writeFile(`${OUT}/consumer/package.json`, encoder({
    name: "release-artifact-consumer", private: true, type: "module",
    dependencies: { [NPM]: `file:../${manifest.npm.file}` },
  }));
  await writeFile(`${OUT}/consumer/index.ts`, `import { mountIsolated, type Handle } from "${NPM}";
export const mount = (element: HTMLElement): Promise<Handle> =>
  mountIsolated(element, { moduleUrl: "/app.js", runtimeBaseUrl: "/runtime/" });\n`);
  run("npm", ["install", "--prefix", `${OUT}/consumer`, "--ignore-scripts", "--no-audit", "--no-fund"]);
  run("npm", ["ci", "--prefix", `${OUT}/consumer`, "--ignore-scripts", "--no-audit", "--no-fund"]);
  run(process.execPath, ["node_modules/typescript/bin/tsc", "--noEmit", "--strict", "--skipLibCheck",
    "--target", "ES2022", "--module", "NodeNext", "--moduleResolution", "NodeNext", `${OUT}/consumer/index.ts`]);
  console.log(`Verified exact Cargo/npm artifacts for ${build.tag ?? "untagged candidate"} at ${build.source}`);
}

async function responseBytes(response, limit = 32 * 1024 * 1024) {
  const chunks = [];
  let size = 0;
  for await (const chunk of response.body ?? []) {
    size += chunk.byteLength;
    if (size > limit) throw new Error("Registry response exceeds size limit");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

/** Registry reads deliberately have no tokens; only immutable PUBLIC packages can be accepted. */
export async function inspectRemote(manifest, fetcher = fetch) {
  const get = url => fetcher(url, {
    redirect: "error", signal: AbortSignal.timeout(30000),
    headers: { "User-Agent": "tui2web-release-verification", "Cache-Control": "no-cache" },
  });
  const required = async (url, missingAllowed = false, limit) => {
    const response = await get(url);
    if (response.status === 404 && missingAllowed) return null;
    if (!response.ok) throw new Error(`Registry verification HTTP ${response.status}: ${url}`);
    return responseBytes(response, limit);
  };
  const index = await required("https://index.crates.io/tu/i2/tui2web", true, 4 * 1024 * 1024);
  const entries = index === null ? [] : index.toString().trim().split("\n").map(line => JSON.parse(line));
  for (const entry of entries) {
    assert.equal(entry.name, CRATE, "Unexpected crate index entry");
    assert.equal(typeof entry.vers, "string");
    assert.match(entry.cksum, /^[a-f0-9]{64}$/);
    assert.equal(typeof entry.yanked, "boolean");
  }
  const crate = entries.find(entry => entry.vers === manifest.version);
  let crateExists = false;
  if (crate) {
    assert.equal(crate.name, CRATE);
    assert.equal(crate.yanked, false, "Existing Cargo version is yanked; manual recovery required");
    assert.equal(crate.cksum, manifest.crate.sha256, "Existing Cargo version conflicts with expected immutable artifact");
    const bytes = await required(`https://static.crates.io/crates/${CRATE}/${CRATE}-${manifest.version}.crate`);
    assert.equal(digest(bytes, "sha256"), manifest.crate.sha256, "Downloaded Cargo artifact checksum mismatch");
    crateExists = true;
  }
  const metadata = await required(`https://registry.npmjs.org/@tui2web%2fruntime/${manifest.version}`, true, 4 * 1024 * 1024);
  let npmExists = false;
  if (metadata) {
    const npm = JSON.parse(metadata.toString());
    assert.equal(npm.name, NPM);
    assert.equal(npm.version, manifest.version);
    assert.equal(npm.dist.integrity, manifest.npm.integrity, "Existing npm version conflicts with expected immutable artifact");
    assert.equal(new URL(npm.dist.tarball).origin, "https://registry.npmjs.org", "Unexpected npm artifact origin");
    const bytes = await required(npm.dist.tarball);
    assert.equal(`sha512-${digest(bytes, "sha512")}`, manifest.npm.integrity, "Downloaded npm artifact integrity mismatch");
    npmExists = true;
  }
  return { crate: crateExists, npm: npmExists };
}

async function publish() {
  assert.ok(process.env.GITHUB_ACTIONS === "true", "Publication is supported only in GitHub Actions");
  const build = await identity(true);
  for (const name of ["CARGO_REGISTRY_TOKEN", "NODE_AUTH_TOKEN"]) {
    assert.ok(process.env[name], `Missing ${name} (configure the documented GitHub Actions secrets)`);
  }
  const manifest = await json(`${OUT}/manifest.json`);
  for (const key of ["schema", "version", "source", "tag"]) assert.equal(manifest[key], build[key], `Release manifest ${key} mismatch`);
  await verifyFiles(manifest);
  const receipt = { ...build, crate: "not attempted", npm: "not attempted" };
  try {
    // Preflight BOTH registries before publishing either. A conflicting existing version blocks all writes.
    const existing = await inspectRemote(manifest);
    receipt.crate = existing.crate ? "verified existing exact artifact" : "not published";
    receipt.npm = existing.npm ? "verified existing exact artifact" : "not published";
    if (!existing.crate) {
      receipt.crate = "publication attempted; verification pending";
      run("cargo", ["publish", "--locked", "--all-features", "--no-verify", "-p", CRATE]);
      assert.equal(digest(await readFile(`target/package/${manifest.crate.file}`), "sha256"), manifest.crate.sha256);
      assert.equal((await inspectRemote(manifest)).crate, true, "Cargo publication not yet publicly verifiable; retry this exact tag");
      receipt.crate = "published and verified exact artifact";
    }
    if (!existing.npm) {
      receipt.npm = "publication attempted; verification pending";
      run("npm", ["publish", `${OUT}/${manifest.npm.file}`, "--access", "public", "--ignore-scripts", "--provenance=false"]);
      assert.equal((await inspectRemote(manifest)).npm, true, "npm publication not yet publicly verifiable; retry this exact tag");
      receipt.npm = "published and verified exact artifact";
    }
    console.log("Both registries verified against the tested immutable artifacts.");
  } finally {
    await writeFile(`${OUT}/receipt.json`, encoder(receipt));
    if (process.env.GITHUB_STEP_SUMMARY) {
      await writeFile(process.env.GITHUB_STEP_SUMMARY,
        `## tui2web release ${build.tag}\n\nSource: \`${build.source}\`\n\n` +
        `Cargo: ${receipt.crate}\n\nnpm: ${receipt.npm}\n\nSee uploaded manifest for artifact checksums.\n`);
    }
  }
}

export async function main(command) {
  if (command === "validate") {
    const build = await identity(true);
    console.log(`Validated ${build.tag} at ${build.source}, reachable from origin/master`);
  } else if (command === "package") {
    await packageArtifacts(Boolean(process.env.RELEASE_TAG));
  } else if (command === "publish") {
    await publish();
  } else {
    throw new Error("Usage: node scripts/release.mjs validate|package|publish");
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv[2]).catch(error => { console.error(error.message); process.exitCode = 1; });
}
