// OPFS setup for RelateDb.open (ADR-0145).
//
// wasm-bindgen imports this file as a snippet. Directory creation is async;
// after prepareOpfs returns, the Rust VFS only calls the sync functions below.
// Sync access handles exist in a dedicated worker, not on the page thread.

export const OPFS_SLOT_COUNT = 64;

const handles = [];

function inDedicatedWorker() {
  return typeof DedicatedWorkerGlobalScope !== 'undefined' && self instanceof DedicatedWorkerGlobalScope;
}

function locked(name) {
  const err = new Error(`storage locked: opfs://${name}`);
  err.name = 'RelateLocked';
  return err;
}

function stash(handle) {
  handles.push(handle);
  return handles.length - 1;
}

function take(id) {
  const handle = handles[id];
  if (!handle) {
    throw new Error(`OPFS handle ${id} is closed`);
  }
  return handle;
}

/**
 * Create `relate/<name>/` and take exclusive sync access handles for the
 * catalog plus {@link OPFS_SLOT_COUNT} slots. Returns numeric ids into this
 * module's handle table.
 *
 * @param {string} name
 * @returns {Promise<{ catalog: number, slots: number[] }>}
 */
export async function prepareOpfs(name) {
  if (!inDedicatedWorker()) {
    throw new Error('OPFS needs a dedicated worker');
  }
  if (typeof navigator === 'undefined' || !navigator.storage || !navigator.storage.getDirectory) {
    throw new Error('OPFS is not available in this browser');
  }
  const root = await navigator.storage.getDirectory();
  const base = await root.getDirectoryHandle('relate', { create: true });
  const dir = await base.getDirectoryHandle(name, { create: true });
  const opened = [];
  const openHandle = async (fileName) => {
    const file = await dir.getFileHandle(fileName, { create: true });
    try {
      const handle = await file.createSyncAccessHandle();
      opened.push(handle);
      return handle;
    } catch (err) {
      for (const held of opened) {
        try {
          held.close();
        } catch {
          // The exclusive lock is what the caller needs to hear about.
        }
      }
      if (err && (err.name === 'NoModificationAllowedError' || err.name === 'InvalidStateError')) {
        throw locked(name);
      }
      throw err;
    }
  };
  const catalog = await openHandle('catalog');
  const slots = [];
  for (let i = 0; i < OPFS_SLOT_COUNT; i++) {
    const label = String(i).padStart(2, '0');
    slots.push(await openHandle(`slot-${label}`));
  }
  return { catalog: stash(catalog), slots: slots.map(stash) };
}

export function slotSize(id) {
  return take(id).getSize();
}

export function slotTruncate(id, len) {
  take(id).truncate(len);
}

export function slotFlush(id) {
  take(id).flush();
}

export function slotClose(id) {
  const handle = handles[id];
  handles[id] = undefined;
  if (handle) handle.close();
}

export function slotRead(id, memory, ptr, len, at) {
  const view = new Uint8Array(memory.buffer, ptr, len);
  return take(id).read(view, { at });
}

export function slotWrite(id, memory, ptr, len, at) {
  const view = new Uint8Array(memory.buffer, ptr, len);
  return take(id).write(view, { at });
}
