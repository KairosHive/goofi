/** Copy text to the clipboard. The Clipboard API is absent outside a secure context (plain http on
 * a LAN), so fall back to the hidden-textarea `execCommand` path, which needs a user gesture. */
export async function copyText(text: string): Promise<boolean> {
	try {
		await navigator.clipboard.writeText(text);
		return true;
	} catch {
		const ta = Object.assign(document.createElement('textarea'), { value: text, readOnly: true });
		ta.style.cssText = 'position: fixed; top: -9999px; opacity: 0';
		document.body.append(ta);
		ta.select();
		try {
			return document.execCommand('copy');
		} catch {
			return false;
		} finally {
			ta.remove();
		}
	}
}
