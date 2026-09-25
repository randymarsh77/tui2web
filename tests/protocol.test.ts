import { test } from "node:test";
import assert from "node:assert/strict";
import "fake-indexeddb/auto";
import { validateSnapshot, parseSnapshot, validateOutput, dimensions, type Snapshot } from "../runtime/protocol.js";
import { indexedDbStore } from "../runtime/storage.js";

const fixture = (): Snapshot => ({ version: 1, directories: ["", "empty", "src"], files: [["src/file", [0, 255, 42]]] });
test("versioned snapshots reject invalid paths, bytes, parents, duplicates and quotas", () => {
  assert.deepEqual(parseSnapshot(JSON.stringify(fixture())), fixture());
  for (const change of [
    { version: 2 }, { directories: ["src"] }, { directories: ["", ""] },
    { files: [["../bad", []]] }, { files: [["/absolute", []]] },
    { files: [["missing/file", []]] }, { files: [["src", []]] },
    { files: [["a", [256]]] }, { files: [["a", [1.1]]] },
    { files: [["a", []], ["a", []]] }, { extra: 1 },
    { files: [["a", Array(1024 * 1024 + 1).fill(0)]] },
  ]) assert.throws(() => validateSnapshot({ ...fixture(), ...change }));
  assert.throws(() => parseSnapshot(" ".repeat(16 * 1024 * 1024 + 1)));
});
test("output/dimension protocol guards", () => {
  assert.throws(() => dimensions(0, 1));
  assert.throws(() => dimensions(301, 1));
  assert.throws(() => dimensions(10, NaN));
  assert.throws(() => validateOutput({ version: 1, frame: null, snapshot: null, exited: false, wakeAfterMs: -1 }));
  assert.throws(() => validateOutput({ version: 2 }));
  dimensions(300, 120);
});
test("IndexedDB namespaces preserve empty directories and arbitrary bytes", async () => {
  const a = indexedDbStore("unit-a");
  const b = indexedDbStore("unit-b");
  await a.save(fixture());
  assert.deepEqual(await a.load(), fixture());
  assert.equal(await b.load(), null);
  await b.save({ version: 1, directories: [""], files: [] });
  await a.clear();
  assert.equal(await a.load(), null);
  assert.deepEqual(await b.load(), { version: 1, directories: [""], files: [] });
  await assert.rejects(a.save({ ...fixture(), version: 2 } as unknown as Snapshot));
});
