// Shipped with the WASM bundle (scripts/build-wasm.sh copies it next to
// relate.js) so every host classifies engine failures the same way.

/**
 * Whether `err`, thrown by a call into the bundle, means the WebAssembly
 * instance trapped (a Rust panic, stack exhaustion, an out-of-bounds access)
 * and cannot be used again. A trapped instance must be replaced, or the page
 * reloaded; any other error leaves it usable.
 *
 * @param {unknown} err
 * @returns {boolean}
 */
export function isWasmTrap(err) {
  if (err instanceof WebAssembly.RuntimeError) return true;
  const message = err instanceof Error ? err.message : String(err);
  return /unreachable|memory access out of bounds|call stack exhausted|maximum call stack size/i.test(message);
}
