<!-- /dev/ui — a gallery of every `$lib/ui` primitive; the e2e integrity sweep reads the page. -->
<script lang="ts">
	import {
		Button,
		Icon,
		ICONS,
		IconButton,
		ScrollArea,
		Bar,
		Field,
		NumberInput,
		Slider,
		Knob,
		Select,
		TextInput,
		Toggle,
		Tabs,
		Disclosure,
		Popover,
		ConfirmDialog,
		Dialog,
		Badge,
		Chip,
		Combobox,
		Segmented,
		StatusDot,
		EmptyState,
		ChoiceGrid,
		type IconName,
		type ButtonVariant,
		type ButtonSize,
		type TabItem,
		type BadgeTone,
		type StatusTone,
		type Choice
	} from '$lib/ui';

	const choices: Choice[] = [
		{ id: 'wave', label: 'Waveform', icon: 'activity', choose: () => {} },
		{ id: 'terminal', label: 'Terminal', icon: 'terminal', choose: () => {} },
		{ id: 'plain', label: 'No icon of its own', choose: () => {} }
	];

	const variants: ButtonVariant[] = ['default', 'primary', 'ghost', 'danger'];
	const sizes: ButtonSize[] = ['sm', 'md'];
	const badgeTones: BadgeTone[] = ['neutral', 'accent', 'success', 'warning', 'danger'];
	const statusTones: StatusTone[] = ['success', 'warning', 'danger'];
	const glyphs: Record<ButtonVariant, IconName> = {
		default: 'settings',
		primary: 'plus',
		ghost: 'refresh-cw',
		danger: 'x'
	};

	let cutoff = $state(0.3);
	let cqValue = $state(0.4);
	let fieldOpen = $state(false);
	let swapMode = $state<'number' | 'text' | 'raw'>('number');
	let swapNum = $state(2);
	let swapText = $state('sin(x)');
	let refreshValue = $state('sine');
	let textText = $state('hello');
	let toggled = $state(false);

	const tabItems: TabItem[] = [
		{ id: 'signal', label: 'Signal' },
		{ id: 'audio', label: 'Audio' },
		{ id: 'video', label: 'Video' }
	];
	let activeTab = $state('signal');

	let popoverAnchor = $state<HTMLElement | null>(null);
	let popoverOpen = $state(false);
	let dialogOpen = $state(false);
	let confirmOpen = $state(false);

	let segment = $state('a');
	let segmentSet = $state<string[]>(['x']);
	let combo = $state('beta');
</script>

<main class="gallery">
	<h1>UI primitives</h1>

	<section>
		<h2>Button</h2>
		<div class="grid">
			{#each variants as variant (variant)}
				{#each sizes as size (size)}
					<Button {variant} {size} data-testid={`ui-button-${variant}-${size}`}>
						{variant} / {size}
					</Button>
				{/each}
			{/each}
		</div>
	</section>

	<section>
		<h2>IconButton</h2>
		<div class="grid">
			{#each variants as variant (variant)}
				{#each sizes as size (size)}
					<IconButton {variant} {size} label={`${variant} ${size} action`}>
						<Icon name={glyphs[variant]} />
					</IconButton>
				{/each}
			{/each}
		</div>
	</section>

	<section>
		<h2>Icon (the whole vendored set)</h2>
		<div class="grid">
			{#each Object.keys(ICONS) as IconName[] as name (name)}
				<span class="icon-tile" title={name}><Icon {name} /></span>
			{/each}
		</div>
	</section>

	<section>
		<h2>ScrollArea</h2>
		<div class="scroll-frame">
			<ScrollArea>
				<div class="rows">
					{#each Array.from({ length: 30 }, (_, i) => i) as i (i)}
						<div class="box">row {i}</div>
					{/each}
				</div>
			</ScrollArea>
		</div>
	</section>

	<section>
		<h2>Bar</h2>
		<Bar>
			{#snippet start()}
				<span>Title</span>
			{/snippet}
			{#snippet end()}
				<div class="grid">
					<Button size="sm">Save</Button>
					<IconButton size="sm" label="Settings"><Icon name="settings" /></IconButton>
				</div>
			{/snippet}
		</Bar>
	</section>

	<section>
		<h2>Field composition (the north star)</h2>
		<div class="form">
			<Field label="cutoff">
				<Slider value={cutoff} onChange={(v) => (cutoff = v)} min={0} max={1} step={0.01} />
				<NumberInput value={cutoff} onChange={(v) => (cutoff = v)} min={0} max={1} step={0.01} scrub />
			</Field>
		</div>
	</section>

	<section>
		<h2>Field as a disclosure summary (the param row)</h2>
		<div class="form">
			<Field label="frequency" expanded={fieldOpen} onExpand={() => (fieldOpen = !fieldOpen)}>
				<Slider value={cutoff} onChange={(v) => (cutoff = v)} min={20} max={20000} />
			</Field>
			{#if fieldOpen}
				<span class="readout">what the caret revealed</span>
			{/if}
		</div>
	</section>

	{#snippet swapChips()}
		<Chip
			tone={swapMode === 'number' ? 'neutral' : 'accent'}
			onclick={() => (swapMode = swapMode === 'number' ? 'text' : 'number')}>fx</Chip
		>
		<Chip onclick={() => (swapMode = swapMode === 'raw' ? 'text' : 'raw')}><Icon name="maximize-2" /></Chip>
	{/snippet}

	<section>
		<h2>Field with a swapping control region (the ParamField shape)</h2>
		<div class="form">
			<Field label="swap" adornment={swapChips}>
				{#if swapMode === 'number'}
					<NumberInput value={swapNum} onChange={(v) => (swapNum = v)} />
				{:else if swapMode === 'text'}
					<TextInput value={swapText} onChange={(v) => (swapText = v)} />
				{:else}
					<textarea rows="2" aria-label="swap raw" bind:value={swapText}></textarea>
				{/if}
			</Field>
		</div>
	</section>

	<section>
		<h2>Knob</h2>
		<div style="width: 64px; height: 64px">
			<Knob value={cutoff} onChange={(v) => (cutoff = v)} min={0} max={1} step={0.01} label="Cutoff" />
		</div>
	</section>

	<section>
		<h2>Slider</h2>
		<div class="form">
			<Slider value={cutoff} onChange={(v) => (cutoff = v)} min={0} max={1} step={0.01} />
		</div>
	</section>

	<section>
		<h2>Select (with refresh)</h2>
		<div class="form">
			<Field label="waveform">
				<Select
					value={refreshValue}
					onChange={(v) => (refreshValue = v)}
					options={['sine', 'square', 'saw', 'triangle']}
					onRefresh={() => {}}
				/>
			</Field>
		</div>
	</section>

	<section>
		<h2>TextInput</h2>
		<div class="form">
			<Field label="text">
				<TextInput value={textText} onChange={(v) => (textText = v)} />
			</Field>
		</div>
	</section>

	<section>
		<h2>Toggle</h2>
		<div class="form">
			<Field label="enabled">
				<Toggle value={toggled} onChange={(v) => (toggled = v)} />
			</Field>
		</div>
	</section>

	<section>
		<h2>Tabs (the connected bar)</h2>
		<div class="tabs-demo">
			<Tabs items={tabItems} active={activeTab} onSelect={(id) => (activeTab = id)} />
			<div class="tabs-body">{activeTab} panel content</div>
		</div>
	</section>

	<section>
		<h2>Disclosure</h2>
		<div class="form">
			<Disclosure>
				{#snippet summary()}
					Advanced options
				{/snippet}
				<p class="disclosure-content">Collapsed by default; the caret rotates and this region mounts on toggle.</p>
			</Disclosure>
		</div>
	</section>

	<section>
		<h2>Popover (anchored + clamped)</h2>
		<div class="pop-row">
			<span class="pop-anchor" bind:this={popoverAnchor}>
				<Button onclick={() => (popoverOpen = !popoverOpen)}>{popoverOpen ? 'Close' : 'Open'} popover</Button>
			</span>
		</div>
		<Popover anchor={popoverAnchor} open={popoverOpen} onDismiss={() => (popoverOpen = false)}>
			<div class="pop-content">
				<strong>Anchored overlay</strong>
				<p>Portalled, clamped on-screen, self-dismissing on Escape or an outside click.</p>
			</div>
		</Popover>
	</section>

	<section>
		<h2>Dialog (centered + focus-trap)</h2>
		<div class="form">
			<Button onclick={() => (dialogOpen = true)}>Open dialog</Button>
		</div>
		<Dialog open={dialogOpen} onClose={() => (dialogOpen = false)}>
			<div class="dialog-content">
				<h3>Confirm action</h3>
				<p>A centered modal: focus is trapped inside, Escape and a backdrop click both close it.</p>
				<div class="grid end">
					<Button onclick={() => (dialogOpen = false)}>Cancel</Button>
					<Button variant="primary" onclick={() => (dialogOpen = false)}>Confirm</Button>
				</div>
			</div>
		</Dialog>
	</section>

	<section>
		<h2>ConfirmDialog (one question, the answers as buttons)</h2>
		<div class="form">
			<Button onclick={() => (confirmOpen = true)}>Ask</Button>
		</div>
		<ConfirmDialog
			open={confirmOpen}
			question="Throw this away?"
			detail="The dialog owns the question, the prose and the row; the caller supplies the answers."
			onClose={() => (confirmOpen = false)}
		>
			<Button variant="danger" onclick={() => (confirmOpen = false)}>Throw away</Button>
			<Button variant="ghost" onclick={() => (confirmOpen = false)}>Cancel</Button>
		</ConfirmDialog>
	</section>

	<section>
		<h2>Badge (static tone pill)</h2>
		<div class="grid">
			{#each badgeTones as tone (tone)}
				<Badge {tone}>{tone}</Badge>
			{/each}
		</div>
	</section>

	<section>
		<h2>Combobox (a list that typing filters)</h2>
		<div class="form">
			<Combobox
				value={combo}
				options={() => ['alpha', 'beta', 'gamma', 'delta'].map((label) => ({ label, detail: label.length + ' letters' }))}
				onCommit={(v) => (combo = v)}
				placeholder="greek"
				testid="ui-combobox"
			/>
		</div>
	</section>

	<section>
		<h2>Segmented (one lit segment, or a lit set — chrome-height)</h2>
		<div class="form">
			<Segmented
				value={segment}
				segments={[
					{ id: 'a', label: 'A', name: 'first' },
					{ id: 'b', label: 'B', name: 'second' },
					{ id: 'c', label: 'C', name: 'third' }
				]}
				onChange={(id) => (segment = id)}
				aria-label="sample"
			/>
			<Segmented
				value={segmentSet}
				segments={[
					{ id: 'x', label: '', icon: 'sliders-horizontal', count: 3, name: 'glyph' },
					{ id: 'y', label: 'E', count: 12, name: 'lettered' },
					{ id: 'z', label: 'R', count: 0, name: 'empty' }
				]}
				onChange={(id) =>
					(segmentSet = segmentSet.includes(id)
						? segmentSet.filter((s) => s !== id)
						: [...segmentSet, id])}
				aria-label="set sample"
			/>
		</div>
	</section>

	<section>
		<h2>Chip (pressable tone pill)</h2>
		<div class="grid">
			{#each badgeTones as tone (tone)}
				<Chip {tone}>{tone}</Chip>
			{/each}
		</div>
	</section>

	<section>
		<h2>StatusDot (no glow)</h2>
		<div class="grid">
			{#each statusTones as tone (tone)}
				<StatusDot {tone} />
			{/each}
		</div>
	</section>

	<section>
		<h2>ChoiceGrid (icon-over-label tiles)</h2>
		<ChoiceGrid {choices} />
	</section>

	<section>
		<h2>EmptyState (centred placeholder)</h2>
		<div class="empty-frame">
			<EmptyState>
				{#snippet title()}No nodes yet{/snippet}
				{#snippet hint()}Add a node from the palette to get started.{/snippet}
			</EmptyState>
		</div>
		<div class="empty-frame">
			<EmptyState />
		</div>
	</section>

	<section>
		<h2>Field in a narrow @container (single-column stack)</h2>
		<div class="cq-box">
			<Field label="cutoff">
				<Slider value={cqValue} onChange={(v) => (cqValue = v)} min={0} max={1} step={0.01} />
				<NumberInput value={cqValue} onChange={(v) => (cqValue = v)} min={0} max={1} step={0.01} />
			</Field>
		</div>
	</section>
</main>

<style>
	.gallery {
		box-sizing: border-box;
		height: 100vh;
		overflow-y: auto;
		padding: var(--space-8);
		display: flex;
		flex-direction: column;
		gap: var(--space-8);
	}
	h1 {
		margin: 0;
		font-size: var(--fs-title);
		color: var(--text);
	}
	h2 {
		margin: 0 0 var(--space-4);
		font-size: var(--fs-strong);
		color: var(--text-dim);
	}
	.grid {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--space-6);
	}
	.grid.end {
		justify-content: flex-end;
	}
	.icon-tile {
		display: flex;
		font-size: var(--fs-title);
		color: var(--text-dim);
	}
	.rows {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
	}
	.box {
		display: grid;
		place-items: center;
		padding: var(--space-2) var(--space-4);
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text);
	}
	.scroll-frame {
		display: flex;
		flex-direction: column;
		height: 8rem;
		width: 12rem;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: var(--space-4);
	}
	.form {
		display: flex;
		flex-direction: column;
		gap: var(--space-6);
		width: 18rem;
		max-width: 100%;
	}
	.readout {
		font-size: var(--fs-micro);
		color: var(--text-muted);
		font-variant-numeric: tabular-nums;
	}
	.tabs-demo {
		width: 22rem;
		max-width: 100%;
	}
	.tabs-body {
		padding: var(--space-7);
		background: var(--surface-1);
		border-radius: 0 0 var(--radius-sm) var(--radius-sm);
		color: var(--text-dim);
		font-size: var(--fs-small);
	}
	.disclosure-content {
		margin: 0;
		color: var(--text-dim);
		font-size: var(--fs-small);
	}
	.pop-row {
		display: flex;
		justify-content: flex-end;
	}
	.pop-anchor {
		display: inline-flex;
	}
	.pop-content {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		max-width: 16rem;
	}
	.pop-content p,
	.dialog-content p {
		margin: 0;
		color: var(--text-dim);
		font-size: var(--fs-small);
	}
	.dialog-content {
		display: flex;
		flex-direction: column;
		gap: var(--space-6);
	}
	.dialog-content h3 {
		margin: 0;
		font-size: var(--fs-strong);
		color: var(--text);
	}
	/* Narrower than the Field's @container threshold. */
	.cq-box {
		container-type: inline-size;
		box-sizing: border-box;
		width: 200px;
		padding: var(--space-4);
		border: 1px dashed var(--border);
		border-radius: var(--radius-sm);
	}
	.empty-frame {
		display: grid;
		height: 10rem;
		width: 20rem;
		max-width: 100%;
		margin-top: var(--space-4);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
	}
</style>
