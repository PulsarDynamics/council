<script lang="ts">
	import { onMount, tick } from 'svelte';
	import EventItem from './EventItem.svelte';
	import type { EventEnvelope } from '$lib/types';

	interface Props {
		events: EventEnvelope[];
		activeSessionId: string | null;
	}
	let { events, activeSessionId }: Props = $props();

	let scrollEl: HTMLElement | null = $state(null);
	let stickToBottom = $state(true);

	const visible = $derived(
		activeSessionId ? events.filter((e) => e.event.session_id === activeSessionId) : events
	);

	$effect(() => {
		// Track the visible count so the auto-scroll triggers only when it grows.
		void visible.length;
		if (!stickToBottom || !scrollEl) return;
		tick().then(() => {
			if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
		});
	});

	function onScroll() {
		if (!scrollEl) return;
		const dist = scrollEl.scrollHeight - (scrollEl.scrollTop + scrollEl.clientHeight);
		stickToBottom = dist < 24;
	}
</script>

<section
	bind:this={scrollEl}
	onscroll={onScroll}
	class="bg-base-200/40 border-base-300/60 relative flex-1 overflow-y-auto rounded-md border p-3"
>
	{#if visible.length === 0}
		<div class="text-base-content/50 flex h-full flex-col items-center justify-center text-center">
			<p class="text-sm">No events yet.</p>
			<p class="text-base-content/40 mt-1 text-xs">
				Set a goal above and the Council will start deliberating.
			</p>
		</div>
	{:else}
		<div class="flex flex-col gap-2">
			{#each visible as envelope (envelope.event.id)}
				<EventItem {envelope} />
			{/each}
		</div>
	{/if}

	{#if !stickToBottom && visible.length > 0}
		<button
			type="button"
			class="bg-primary text-primary-content absolute right-4 bottom-4 rounded-full px-3 py-1 text-xs font-medium shadow-lg"
			onclick={() => {
				stickToBottom = true;
				if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
			}}
		>
			Jump to latest
		</button>
	{/if}
</section>
