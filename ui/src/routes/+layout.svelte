<script lang="ts">
	import type { Snippet } from 'svelte';
	import './layout.css';
	import TopNav from '$lib/components/TopNav.svelte';

	/**
	 * Shared chrome for every route in the app.
	 *
	 *   ┌────────────────────────────────────────────────────────────────┐
	 *   │  [⏺ COUNCIL]  multi-agent orchestration           console ...  │  ← shared header
	 *   ├────────────────────────────────────────────────────────────────┤
	 *   │  {children}                                                    │
	 *   └────────────────────────────────────────────────────────────────┘
	 *
	 * The wordmark and TopNav are constant across all pages. Pages
	 * render any page-specific status indicators inside their own
	 * `<main>` (the console, for instance, shows its live status +
	 * event count in a thin sub-header just above the 3-column grid).
	 */

	interface Props {
		children: Snippet;
	}
	let { children }: Props = $props();
</script>

<div class="bg-base-100 text-base-content min-h-screen font-sans">
	<header
		class="border-border bg-background/95 sticky top-0 z-20 flex items-center gap-4 border-b px-4 py-3 backdrop-blur"
	>
		<a href="/" class="flex items-center gap-2" aria-label="Council home">
			<svg
				viewBox="0 0 24 24"
				class="text-primary size-4"
				fill="none"
				stroke="currentColor"
				stroke-width="2"
			>
				<circle cx="12" cy="5" r="2" />
				<circle cx="5" cy="18" r="2" />
				<circle cx="19" cy="18" r="2" />
				<path d="M12 7 L 6 16 M 12 7 L 18 16" />
			</svg>
			<h1 class="font-mono text-[15px] font-semibold tracking-[0.32em] uppercase">Council</h1>
		</a>
		<span class="text-muted-foreground hidden font-mono text-[11px] sm:inline">
			multi-agent orchestration
		</span>

		<div class="ml-auto flex items-center gap-3">
			<TopNav />
		</div>
	</header>

	{@render children()}
</div>
