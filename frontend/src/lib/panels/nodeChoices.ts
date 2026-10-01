/** The "nothing bound" option. Empty on purpose: `Select` never prepends an empty value, so this
 * one cannot collide with a live uid. */
export const NO_NODE = '';

/** Collate the way a user reads a name that ends in a number: `oscillator2` before `oscillator10`. */
export const byName = (a: { name: string }, b: { name: string }): number =>
	a.name.localeCompare(b.name, undefined, { numeric: true });

/** The node a panel binds, as a `Select`'s three inputs. The committed value is the uid. */
export function nodePickList(nodes: readonly { uid: string; name: string }[], bound: string | null, emptyLabel: string) {
	const sorted = [...nodes].sort(byName);
	const labels: Record<string, string> = { [NO_NODE]: emptyLabel };
	for (const n of sorted) labels[n.uid] = n.name;
	return {
		// A binding whose node is gone reads as unbound: this replica can be a round trip behind.
		value: bound && sorted.some((n) => n.uid === bound) ? bound : NO_NODE,
		options: [NO_NODE, ...sorted.map((n) => n.uid)],
		labels
	};
}
