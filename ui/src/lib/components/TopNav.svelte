<script lang="ts">
	import { page } from '$app/state';
	import { cn } from '$lib/cn';

	/**
	 * Compact top-right nav. Three destinations: console / history /
	 * settings. Each is a real SvelteKit route so the URL stays
	 * meaningful and the pages are deep-linkable / refreshable.
	 *
	 * Active state is derived from `$page.url.pathname` so we don't
	 * need the parent to pass anything in.
	 */

	type NavTarget = 'console' | 'history' | 'settings';

	const links: { id: NavTarget; label: string; href: string }[] = [
		{ id: 'console', label: 'console', href: '/' },
		{ id: 'history', label: 'history', href: '/history' },
		{ id: 'settings', label: 'settings', href: '/settings' }
	];

	function activeFor(path: string): NavTarget {
		if (path === '/' || path.startsWith('/?')) return 'console';
		if (path.startsWith('/history')) return 'history';
		if (path.startsWith('/settings')) return 'settings';
		return 'console';
	}

	const active = $derived(activeFor(page.url.pathname));
</script>

<nav class="flex items-center gap-1 font-mono text-[11px]">
	{#each links as l (l.id)}
		<a
			href={l.href}
			aria-current={active === l.id ? 'page' : undefined}
			data-sveltekit-preload-data="hover"
			class={cn(
				'rounded-sm px-2 py-1 tracking-wide text-muted-foreground transition-colors hover:text-foreground',
				active === l.id && 'bg-card text-foreground'
			)}
		>
			{l.label}
		</a>
	{/each}
</nav>
