/** What the pointer rests on in one machine, shared by its canvas and its score: a state lit on
 * one lights its box or its column on the other, and a transition its line or its arc. */
export class MachineHover {
	machine = $state<string | null>(null);
	state = $state<string | null>(null);
	transition = $state<string | null>(null);

	overState(machine: string, state: string | null): void {
		this.machine = machine;
		this.state = state;
		this.transition = null;
	}
	overTransition(machine: string, id: string | null): void {
		this.machine = machine;
		this.transition = id;
		this.state = null;
	}
	clear(): void {
		this.state = null;
		this.transition = null;
	}
	litState(machine: string, state: string): boolean {
		return this.machine === machine && this.state === state;
	}
	litTransition(machine: string, id: string): boolean {
		return this.machine === machine && this.transition === id;
	}
}

let _hover: MachineHover | null = null;
export function machineHover(): MachineHover {
	if (!_hover) _hover = new MachineHover();
	return _hover;
}
