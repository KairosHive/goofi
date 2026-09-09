/** Runtime interfaces passed to a plugin frontend's default activation function. */
export interface Context {
	call<T = unknown>(operation: string, arguments_?: Record<string, unknown>): Promise<T>;
	log(message: string, level?: 'info' | 'warning' | 'error'): void;
}

export interface PanelContext extends Context {
	readonly panel_id: string;
	readonly state: unknown;
	set_state(value: Record<string, unknown>, intent?: 'navigation' | 'edit'): void;
}

export interface Panel {
	id: string;
	title: string;
	mount(element: HTMLElement, ctx: PanelContext): void | (() => void);
}

export interface Header {
	id: string;
	label: string;
	title?: string;
	disabled?: boolean;
	onActivate?: () => void | Promise<void>;
}

export interface HeaderHandle {
	update(patch: Partial<Omit<Header, 'id'>>): void;
	dispose(): void;
}

export interface Plugin {
	register_panel(panel: Panel): void;
	register_header(entry: Header): HeaderHandle;
}

export type Activate = (plugin: Plugin, ctx: Context) => void | (() => void) | Promise<void | (() => void)>;
