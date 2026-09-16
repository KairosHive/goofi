/** Runtime interfaces passed to a plugin frontend's default activation function. */
export interface Context {
	call<T = unknown>(operation: string, arguments_?: Record<string, unknown>): Promise<T>;
	log(message: string, level?: 'info' | 'warning' | 'error'): void;
}

/** A node the user dropped on the panel. `zone` is the part after `#` of the `data-node-drop`
 * attribute the drop landed on, or empty for the panel itself. */
export interface NodeDrop {
	node: string;
	zone: string;
	x: number;
	y: number;
}

/** A node drag in progress: which zone of this panel, if any, the pointer is over. */
export interface NodeDrag {
	node: string;
	over: boolean;
	zone: string | null;
}

export interface PanelContext extends Context {
	readonly panel_id: string;
	readonly state: unknown;
	set_state(value: Record<string, unknown>, intent?: 'navigation' | 'edit'): void;
	/** Receive nodes dropped on the panel or on its `data-node-drop="<panel_id>#<zone>"` elements. */
	on_node_drop(handler: (drop: NodeDrop) => void): () => void;
	/** Follow a node drag; called with null when no drag runs. */
	on_node_drag(handler: (drag: NodeDrag | null) => void): () => void;
}

export interface Panel {
	id: string;
	title: string;
	/** Whether a node dragged from the editor can be dropped on this panel. */
	accepts_node?: boolean;
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
