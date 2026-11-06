/**
 * Whether `err`, thrown by a call into the bundle, means the WebAssembly
 * instance trapped and cannot be used again.
 */
export function isWasmTrap(err: unknown): boolean;
