/** Literal presentation follows the backend's control reading rules. */
import type { Literal } from '$lib/api/generated';

export function truth(value: Literal | null): boolean {
	return typeof value === 'string' ? value !== '' : typeof value === 'number' ? value > 0 : Array.isArray(value) ? value.some(truth) : value === true;
}

/** A binary32 value as its shortest round-trip decimal, as Rust prints its document literal. */
export function decimal32(value: number): number {
	if (!Number.isFinite(value)) return value;
	for (let digits = 1; digits <= 9; digits++) {
		const decimal = Number(value.toPrecision(digits));
		if (Math.fround(decimal) === value) return decimal;
	}
	return value;
}
