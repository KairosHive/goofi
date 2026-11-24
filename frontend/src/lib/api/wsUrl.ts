/** The WebSocket URL of a path on the serving host, each segment URL-encoded. */
export function wsUrl(segments: string[], loc: { protocol: string; host: string } = location): string {
	const proto = loc.protocol === 'https:' ? 'wss:' : 'ws:';
	return `${proto}//${loc.host}/${segments.map(encodeURIComponent).join('/')}`;
}
