/** A ViewBinding is one viewer instance's kind and settings; the components read nothing else. */
import { resolveKind, resolveSettings, type ViewerKind } from './registry';
import type { SettingValue, SettingsMap } from './module';
import type { SlotView } from './inlineView';

export interface ViewBinding {
	readonly kind: ViewerKind;
	readonly settings: SettingsMap;
	setKind(kind: ViewerKind): void;
	setSetting(key: string, value: SettingValue): void;
}

/** A binding over a raw stored view; `write` merges the patch it is given into that view. */
export function viewBinding(
	dtype: string | null,
	read: () => SlotView,
	write: (patch: SlotView, label: string) => void
): ViewBinding {
	return {
		get kind() {
			return resolveKind(dtype, read().kind);
		},
		get settings() {
			return resolveSettings(this.kind, read().settings);
		},
		setKind(kind) {
			write({ kind }, `Viewer → ${kind}`);
		},
		setSetting(key, value) {
			write({ settings: { [key]: value } }, `Viewer ${key}`);
		}
	};
}
