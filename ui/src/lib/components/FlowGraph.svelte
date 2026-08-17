<script lang="ts">
	import { cn } from '$lib/cn';

	/**
	 * Compact channel-bus visualization. One pill per known channel; pills
	 * pulse (border + glow + brighter color) for ~1.4s after the most
	 * recent traffic on that channel, then settle. Clicking a pill toggles
	 * it in the parent's filter set.
	 *
	 * The component is presentation-only; it doesn't talk to the bus.
	 * The parent (the page) maintains a `traffic` map of channel -> ms
	 * timestamp of the last emission, and an `active` array of channels
	 * the user has filtered to.
	 */
	interface Props {
		traffic: Record<string, number>;
		active: string[];
		onToggle?: (channel: string) => void;
	}
	let { traffic, active, onToggle }: Props = $props();

	const channels = ['goal', 'plan', 'spec', 'result', 'broadcast'] as const;

	// Re-render every 200ms so the "hot" pulse decays even when no
	// new traffic arrives. The interval is owned by the component.
	let now = $state(Date.now());
	$effect(() => {
		const id = setInterval(() => (now = Date.now()), 200);
		return () => clearInterval(id);
	});

	function isHot(c: string): boolean {
		const at = traffic[c];
		return typeof at === 'number' && now - at < 1400;
	}
</script>

<div class="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2">
	<span class="font-mono text-[10px] tracking-[0.18em] text-muted-foreground uppercase">bus</span>
	{#each channels as c, i (c)}
		{@const hot = isHot(c)}
		{@const selected = active.includes(c)}
		<div class="flex items-center gap-2">
			{#if i > 0}
				<span
					class={cn(
						'h-px w-6 transition-colors',
						hot ? 'bg-primary' : 'bg-border'
					)}
				></span>
			{/if}
			<button
				type="button"
				onclick={() => onToggle?.(c)}
				class={cn(
					'rounded-sm border px-2 py-1 font-mono text-[10px] tracking-wider transition-all',
					hot
						? 'border-primary bg-primary/15 text-primary shadow-[0_0_16px_-4px_var(--primary)]'
						: 'border-border text-muted-foreground hover:text-foreground',
					selected && !hot && 'border-foreground/40 text-foreground'
				)}
			>
				{c}
				{#if typeof traffic[c] === 'number'}
					<span class="ml-1 opacity-60">●</span>
				{/if}
			</button>
		</div>
	{/each}
</div>
