<script lang="ts">
	import { cn } from '$lib/cn';

	/**
	 * Compact top-right nav. Three destinations: console / history /
	 * settings. We use buttons (not links) so the parent can decide
	 * whether the destination is a route, a modal, or a side panel —
	 * the visual treatment is the same either way.
	 */
	export type NavTarget = 'console' | 'history' | 'settings';

	interface Props {
		active?: NavTarget;
		onNavigate?: (target: NavTarget) => void;
	}
	let { active = 'console', onNavigate }: Props = $props();

	const links: { id: NavTarget; label: string }[] = [
		{ id: 'console', label: 'console' },
		{ id: 'history', label: 'history' },
		{ id: 'settings', label: 'settings' }
	];
</script>

<nav class="flex items-center gap-1 font-mono text-[11px]">
	{#each links as l (l.id)}
		<button
			type="button"
			onclick={() => onNavigate?.(l.id)}
			class={cn(
				'rounded-sm px-2 py-1 tracking-wide text-muted-foreground transition-colors hover:text-foreground',
				active === l.id && 'bg-card text-foreground'
			)}
		>
			{l.label}
		</button>
	{/each}
</nav>
