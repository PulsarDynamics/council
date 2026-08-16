<script lang="ts">
	import type { Event, EventEnvelope } from '$lib/types';
	import { eventKindLabel } from '$lib/api';

	interface Props {
		envelope: EventEnvelope;
	}
	let { envelope }: Props = $props();

	const event = $derived(envelope.event);

	// Visual treatment per event kind.
	const palette = $derived(paletteFor(event));
	const headline = $derived(headlineFor(event));

	function paletteFor(e: Event): {
		border: string;
		bg: string;
		tag: string;
		tagText: string;
	} {
		switch (e.kind.type) {
			case 'user_message':
				return {
					border: 'border-l-zinc-500',
					bg: 'bg-zinc-800/40',
					tag: 'bg-zinc-700/60',
					tagText: 'text-zinc-200'
				};
			case 'agent_message':
				return {
					border: 'border-l-sky-400',
					bg: 'bg-sky-950/30',
					tag: 'bg-sky-500/15',
					tagText: 'text-sky-200'
				};
			case 'agent_message_delta':
				// Deltas are filtered out of the rendered list; the case
				// exists only to keep the union exhaustive.
				return {
					border: 'border-l-sky-700',
					bg: 'bg-sky-950/10',
					tag: 'bg-sky-700/15',
					tagText: 'text-sky-300/80'
				};
			case 'agent_thinking':
				return {
					border: 'border-l-sky-700',
					bg: 'bg-sky-950/10',
					tag: 'bg-sky-700/15',
					tagText: 'text-sky-300/80'
				};
			case 'tool_call':
				return {
					border: 'border-l-amber-400',
					bg: 'bg-amber-950/20',
					tag: 'bg-amber-500/15',
					tagText: 'text-amber-200'
				};
			case 'tool_result':
				return {
					border: e.kind.error ? 'border-l-rose-400' : 'border-l-emerald-400',
					bg: e.kind.error ? 'bg-rose-950/20' : 'bg-emerald-950/20',
					tag: e.kind.error ? 'bg-rose-500/15' : 'bg-emerald-500/15',
					tagText: e.kind.error ? 'text-rose-200' : 'text-emerald-200'
				};
			case 'file_change':
				return {
					border: 'border-l-violet-400',
					bg: 'bg-violet-950/20',
					tag: 'bg-violet-500/15',
					tagText: 'text-violet-200'
				};
			case 'agent_status':
				return {
					border: 'border-l-zinc-600',
					bg: 'bg-zinc-900/40',
					tag: 'bg-zinc-700/40',
					tagText: 'text-zinc-300'
				};
			case 'llm_call':
				return {
					border: 'border-l-fuchsia-400',
					bg: 'bg-fuchsia-950/20',
					tag: 'bg-fuchsia-500/15',
					tagText: 'text-fuchsia-200'
				};
			case 'system':
				return {
					border: 'border-l-zinc-700',
					bg: 'bg-transparent',
					tag: 'bg-zinc-800/40',
					tagText: 'text-zinc-400'
				};
			case 'session_created':
				return {
					border: 'border-l-emerald-500',
					bg: 'bg-emerald-950/30',
					tag: 'bg-emerald-500/15',
					tagText: 'text-emerald-200'
				};
			case 'session_completed':
				return {
					border: 'border-l-emerald-500',
					bg: 'bg-emerald-950/30',
					tag: 'bg-emerald-500/15',
					tagText: 'text-emerald-200'
				};
			case 'error':
				return {
					border: 'border-l-rose-500',
					bg: 'bg-rose-950/30',
					tag: 'bg-rose-500/15',
					tagText: 'text-rose-200'
				};
		}
	}

	function headlineFor(e: Event): string {
		return eventKindLabel(e);
	}

	function bodyFor(e: Event): string {
		const k = e.kind;
		switch (k.type) {
			case 'user_message':
				return k.content;
			case 'agent_message':
				return k.content;
			case 'agent_thinking':
				return k.content;
			case 'system':
				return k.message;
			case 'session_created':
				return `Goal: ${k.goal}`;
			case 'session_completed':
				return k.summary;
			case 'error':
				return `${k.source}: ${k.message}`;
			case 'llm_call':
				return `${k.model} — ${k.prompt_tokens} in / ${k.completion_tokens} out · ${k.duration_ms}ms`;
			case 'agent_status':
				return `${k.agent} → ${k.status}`;
			default:
				return '';
		}
	}

	function extraFor(e: Event): string {
		const k = e.kind;
		if (k.type === 'tool_call') return JSON.stringify(k.args, null, 2);
		if (k.type === 'tool_result') {
			return k.error
				? `error: ${k.error}\n${JSON.stringify(k.result, null, 2)}`
				: JSON.stringify(k.result, null, 2);
		}
		if (k.type === 'file_change' && k.diff) return k.diff;
		return '';
	}

	const body = $derived(bodyFor(event));
	const extra = $derived(extraFor(event));
	const hasExtra = $derived(extra.length > 0);
	let expanded = $state(false);

	const time = $derived(formatTime(event.timestamp));

	function formatTime(iso: string): string {
		try {
			const d = new Date(iso);
			return d.toLocaleTimeString(undefined, {
				hour: '2-digit',
				minute: '2-digit',
				second: '2-digit',
				hour12: false
			});
		} catch {
			return iso;
		}
	}
</script>

<article
	class="border-base-300/40 rounded-md border border-l-2 p-3 font-mono text-xs {palette.bg} {palette[
		'border'
	]}"
>
	<header class="mb-1.5 flex items-center justify-between gap-2">
		<div class="flex items-center gap-2">
			<span class="rounded px-1.5 py-0.5 text-[10px] font-semibold {palette.tag} {palette.tagText}">
				{headline}
			</span>
			<span class="text-base-content/40 text-[10px]">on {envelope.channel}</span>
		</div>
		<span class="text-base-content/40 text-[10px]">{time}</span>
	</header>

	{#if body}
		<pre
			class="text-base-content/85 m-0 overflow-x-auto font-sans text-sm leading-relaxed whitespace-pre-wrap">{body}</pre>
	{/if}

	{#if hasExtra}
		<button
			type="button"
			class="text-base-content/50 hover:text-base-content/80 mt-1 text-[10px] underline"
			onclick={() => (expanded = !expanded)}
		>
			{expanded ? 'hide details' : 'show details'}
		</button>
		{#if expanded}
			<pre
				class="text-base-content/70 bg-base-300/30 mt-1 overflow-x-auto rounded p-2 text-[11px]">{extra}</pre>
		{/if}
	{/if}
</article>
