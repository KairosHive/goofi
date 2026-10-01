// GENERATED from backend/goofi-codec/src/lib.rs — do not edit by hand. The GOOF header's
// version, size and tags are declared once, in the codec. Regenerate by running
// `cargo test -p goofi-tests contracts::`, which rewrites this file when it drifts.
export const VERSION = 2;
export const HEADER_SIZE = 14;
/** The tag of a frame that carries a held frame's per-emit stamps and no body. */
export const STAMPS_TAG = 4;
export type DataType = 'ARRAY' | 'STRING' | 'TABLE' | 'TEXTURE';
export const DTYPE_TAG: Record<number, DataType> = {
	0: 'ARRAY',
	1: 'STRING',
	2: 'TABLE',
	3: 'TEXTURE'
};
