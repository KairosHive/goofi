/** The in-house primitive library; import primitives from here, not by file. */
export { Button, IconButton, Icon, TabStrip as Tabs, type ButtonVariant, type ButtonSize, type TabItem } from 'panelty';
export { ICONS, type IconName } from './icons';

export { default as ScrollArea } from './ScrollArea.svelte';
export { default as Bar } from './Bar.svelte';

export { default as Field } from './Field.svelte';
export { default as NumberInput } from './NumberInput.svelte';
export { default as Slider } from './Slider.svelte';
export { default as Knob } from './Knob.svelte';
export { default as Select } from './Select.svelte';
export { default as Combobox, type ComboOption } from './Combobox.svelte';
export { default as TextInput } from './TextInput.svelte';
export { MODE_ATTRS } from './inputMode';
export { default as Toggle } from './Toggle.svelte';

export { default as Disclosure } from './Disclosure.svelte';

export { default as Popover } from './Popover.svelte';
export { default as Dialog } from './Dialog.svelte';
export { default as ConfirmDialog } from './ConfirmDialog.svelte';

export { isTextEditingTarget } from './textEditing';

export { default as Badge, type BadgeTone } from './Badge.svelte';
export { default as Chip } from './Chip.svelte';
export { default as Segmented, type Segment } from './Segmented.svelte';
export { default as StatusDot, type StatusTone, type StatusDotSize } from './StatusDot.svelte';
export { default as EmptyState } from './EmptyState.svelte';
export { default as ChoiceGrid, type Choice } from './ChoiceGrid.svelte';
export { default as PaintPad } from './PaintPad.svelte';
