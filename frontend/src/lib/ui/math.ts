/** `v` held between `lo` and `hi`. */
export const clamp = (v: number, lo = 0, hi = 1): number => Math.min(hi, Math.max(lo, v));
/** `v` rounded to `places` decimals, as a number. */
export const fixed = (v: number, places = 3): number => Number(v.toFixed(places));
