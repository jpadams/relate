/** Number of data slots `prepareOpfs` opens for one database (ADR-0145). */
export const OPFS_SLOT_COUNT: number;

/**
 * Create the origin-private directory for `name` and take exclusive sync
 * access handles. Only succeeds in a dedicated worker.
 */
export function prepareOpfs(name: string): Promise<{ catalog: number; slots: number[] }>;

export function slotSize(id: number): number;
export function slotTruncate(id: number, len: number): void;
export function slotFlush(id: number): void;
export function slotClose(id: number): void;
export function slotRead(id: number, memory: WebAssembly.Memory, ptr: number, len: number, at: number): number;
export function slotWrite(id: number, memory: WebAssembly.Memory, ptr: number, len: number, at: number): number;
