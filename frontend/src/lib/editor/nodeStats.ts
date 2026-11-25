/** Pure formatting of a node's execution telemetry. */
import type { NodeStats } from '$lib/api/control';

/** Tick cadence, e.g. `12.4` / `1000`. */
function fmtRate(ups: number): string {
	return ups >= 100 ? Math.round(ups).toString() : ups.toFixed(1);
}

/** A fraction of the frame as a percentage, e.g. `0.3%` / `50%`. */
function fmtShare(share: number): string {
	const percent = share * 100;
	return `${percent >= 10 ? Math.round(percent).toString() : percent.toFixed(1)}%`;
}

/** Whether two readings say the same thing: what keeps an unchanged push from re-rendering. */
export function sameStats(a: NodeStats | null | undefined, b: NodeStats): boolean {
	if (!a) return false;
	if ('frame_share' in a) return 'frame_share' in b && a.frame_share === b.frame_share;
	return 'updates_per_second' in b && a.updates_per_second === b.updates_per_second;
}

/** The node header's readout: a node's update rate, or the share of the frame a graphics node's
 * pass takes; null until the first NODE_STATS push. */
export function formatNodeLoad(stats: NodeStats | null | undefined): string | null {
	if (!stats) return null;
	if ('frame_share' in stats) return `${fmtShare(stats.frame_share)} frame`;
	return `${fmtRate(stats.updates_per_second)} upd/s`;
}

/** Labelled rows for the metadata panel, or `[]` before any stats arrive. `drops` is the selected
 * slot's coalescing rate; null omits the row, while 0 still shows, meaning "running, dropping none". */
export function nodeStatsRows(
	stats: NodeStats | null | undefined,
	drops?: number | null
): { label: string; value: string }[] {
	if (!stats) return [];
	const rows =
		'frame_share' in stats
			? [{ label: 'Frame time', value: fmtShare(stats.frame_share) }]
			: [{ label: 'Update rate', value: `${fmtRate(stats.updates_per_second)} upd/s` }];
	if (drops != null) rows.push({ label: 'Dropped', value: `${drops.toFixed(1)}/s` });
	return rows;
}
