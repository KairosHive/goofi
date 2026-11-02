// GENERATED from backend/goofi-codec/src/lib.rs — do not edit by hand. The GOOF header's
// size, its boundary and its tags are declared once, in the codec. Regenerate by running
// `cargo test -p goofi-tests contracts::`, which rewrites this file when it drifts.
export const HEADER_SIZE = 13;
/** The boundary the body and an array's samples start on. */
export const ALIGN = 4;
/** The tag of a frame that carries a held frame's per-emit stamps and no body. */
export const STAMPS_TAG = 255;
export type DataType = 'ARRAY' | 'STRING' | 'TABLE' | 'TEXTURE' | 'FUNCTIONAL';
export const DTYPE_TAG: Record<number, DataType> = {
	0: 'ARRAY',
	1: 'STRING',
	2: 'TABLE',
	3: 'TEXTURE',
	4: 'FUNCTIONAL'
};
