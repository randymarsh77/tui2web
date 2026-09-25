import { validateSnapshot, type Snapshot } from "./protocol.js";

export interface SnapshotStore {
  load(): Promise<Snapshot | null>;
  save(snapshot: Snapshot): Promise<void>;
  clear(): Promise<void>;
}
/** Each handle opens/closes its own connection, so disposal never leaves a DB open. */
export function indexedDbStore(namespace: string): SnapshotStore {
  if (!namespace || namespace.length > 128) throw new Error("Persistence namespace must be 1..128 characters");
  const transaction = <T>(mode: IDBTransactionMode, action: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> =>
    new Promise((resolve, reject) => {
      const open = indexedDB.open("tui2web-v1", 1);
      let blocked = false;
      open.onupgradeneeded = () => open.result.createObjectStore("snapshots");
      open.onerror = () => reject(open.error);
      open.onblocked = () => { blocked = true; reject(new Error("IndexedDB open blocked")); };
      open.onsuccess = () => {
        const db = open.result;
        if (blocked) { db.close(); return; }
        try {
          const tx = db.transaction("snapshots", mode);
          const request = action(tx.objectStore("snapshots"));
          tx.oncomplete = () => { db.close(); resolve(request.result); };
          tx.onabort = tx.onerror = () => { db.close(); reject(tx.error ?? request.error ?? new Error("IndexedDB transaction failed")); };
        } catch (error) { db.close(); reject(error); }
      };
    });
  return {
    async load() {
      const snapshot = await transaction("readonly", s => s.get(namespace));
      return snapshot === undefined ? null : validateSnapshot(snapshot);
    },
    async save(snapshot) {
      validateSnapshot(snapshot);
      await transaction("readwrite", s => s.put(snapshot, namespace));
    },
    async clear() { await transaction("readwrite", s => s.delete(namespace)); },
  };
}
