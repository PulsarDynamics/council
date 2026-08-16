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

	// Streaming buffers: session_id -> agent_name -> accumulated text.
	// Deltas are appended here; the final AgentMessage for that (session,
	// agent) pair (or any non-delta event for that session) clears the
	// buffer because the canonical message has been (or is about to be)
	// published into `events`.
	let streamingBufs: Record<string, Record<string, string>> = $state({});

	// Hide the noisy per-delta events from the visible list — the
	// in-progress buffer at the bottom of the stream shows the live text.
	const visible = $derived.by(() => {
		const filtered = events.filter(
			(e) => e.event.kind.type !== 'agent_message_delta'
		);
		return activeSessionId
			? filtered.filter((e) => e.event.session_id === activeSessionId)
			: filtered;
	});

	// Streaming items to render *after* the events list. One per
	// (session, agent) pair with a non-empty buffer; filtered to the
	// active session when set.
	const streamingItems = $derived.by(() => {
		const items: { key: string; agent: string; text: string }[] = [];
		for (const [sid, agents] of Object.entries(streamingBufs)) {
			if (activeSessionId && sid !== activeSessionId) continue;
			for (const [agent, text] of Object.entries(agents)) {
				if (!text) continue;
				items.push({ key: `${sid}:${agent}`, agent, text });
			}
		}
		// Most-recent first (we render them at the bottom, so newer on
		// top within the streaming block reads more naturally).
		items.reverse();
		return items;
	});

	// Maintain the streaming buffer as events flow in.
	$effect(() => {
		// Re-scan the latest few events whenever the array changes. We
		// only need to look at events we haven't seen yet; the simplest
		// correct version re-scans everything (deltas are cheap), but we
		// can speed it up by tracking a cursor if it ever matters.
		const next: Record<string, Record<string, string>> = {};
		for (const env of events) {
			const sid = env.event.session_id;
			const k = env.event.kind;
			if (k.type === 'agent_message_delta') {
				const agentMap = (next[sid] = next[sid] || {});
				agentMap[k.agent] = (agentMap[k.agent] || '') + k.delta;
			} else {
				// Any non-delta event ends the in-progress stream for
				// every agent in that session.
				if (next[sid]) next[sid] = {};
			}
		}
		streamingBufs = next;
	});

	$effect(() => {
		// Track the visible count so the auto-scroll triggers only when it grows.
		void visible.length;
		void streamingItems.length;
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
	{#if visible.length === 0 && streamingItems.length === 0}
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
			{#each streamingItems as item (item.key)}
				<article
					class="border-l-sky-400 bg-sky-950/30 border-base-300/40 rounded-md border border-l-2 p-3"
				>
					<header class="mb-1.5 flex items-center justify-between gap-2">
						<div class="flex items-center gap-2">
							<span
								class="bg-sky-500/15 text-sky-200 rounded px-1.5 py-0.5 text-[10px] font-semibold"
							>
								{item.agent}
							</span>
							<span class="text-sky-300/70 text-[10px]">streaming…</span>
						</div>
						<span class="text-base-content/40 text-[10px]">live</span>
					</header>
					<pre
						class="text-base-content/85 m-0 overflow-x-auto font-sans text-sm leading-relaxed whitespace-pre-wrap">{item.text}</pre>
				</article>
			{/each}
		</div>
	{/if}

	{#if !stickToBottom && (visible.length > 0 || streamingItems.length > 0)}
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
