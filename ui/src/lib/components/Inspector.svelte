<script lang="ts">
	import { accentOf } from '$lib/agentColors';
	import type { EventEnvelope, EventKind } from '$lib/types';

	interface Props {
		event: EventEnvelope | null;
		goal: string;
	}
	let { event, goal }: Props = $props();

	function Row(k: string, v: string | undefined) {
		if (!v) return null;
		return { k, v };
	}

	// Pull a small set of "interesting" fields from any event kind for
	// the key/value table. We don't try to render every variant — the
	// raw JSON payload below is the source of truth.
	type RowT = { k: string; v: string };
	const rows = $derived.by((): RowT[] => {
		if (!event) return [];
		const out: RowT[] = [];
		const k = event.event.kind;
		out.push({ k: 'kind', v: k.type });
		out.push({ k: 'at', v: event.event.timestamp });
		switch (k.type) {
			case 'agent_message':
			case 'agent_message_delta':
			case 'agent_thinking':
			case 'agent_status':
				out.push({ k: 'agent', v: k.agent });
				break;
			case 'tool_call':
			case 'tool_result':
				out.push({ k: 'agent', v: k.agent });
				out.push({ k: 'tool', v: k.tool });
				break;
			case 'file_change':
				out.push({ k: 'path', v: k.path });
				out.push({ k: 'change', v: k.kind });
				break;
			case 'llm_call':
				out.push({ k: 'agent', v: k.agent });
				out.push({ k: 'model', v: k.model });
				out.push({
					k: 'tokens',
					v: `${k.prompt_tokens} in / ${k.completion_tokens} out`
				});
				out.push({ k: 'duration', v: `${k.duration_ms}ms` });
				break;
			case 'session_created':
				out.push({ k: 'goal', v: k.goal });
				break;
			case 'session_completed':
			case 'session_cancelled':
				out.push({ k: 'reason', v: 'reason' in k ? k.reason : '' });
				break;
			case 'error':
				out.push({ k: 'source', v: k.source });
				break;
		}
		return out;
	});

	const payloadText = $derived.by(() => {
		if (!event) return '';
		const k = event.event.kind;
		switch (k.type) {
			case 'user_message':
			case 'agent_message':
			case 'agent_thinking':
				return k.content;
			case 'system':
				return k.message;
			case 'session_created':
				return k.goal;
			case 'session_completed':
				return k.summary;
			case 'session_cancelled':
				return k.reason;
			case 'agent_message_delta':
				return k.delta;
			case 'tool_call':
				return JSON.stringify({ tool: k.tool, args: k.args }, null, 2);
			case 'tool_result':
				return JSON.stringify(
					{ tool: k.tool, result: k.result, error: k.error ?? null },
					null,
					2
				);
			case 'file_change':
				return k.diff ?? '(no diff)';
			case 'llm_call':
				return JSON.stringify(k, null, 2);
			case 'agent_status':
				return `${k.agent} → ${k.status}`;
			case 'error':
				return `${k.source}: ${k.message}`;
		}
	});
</script>

<div class="flex h-full flex-col overflow-y-auto">
	<header
		class="text-muted-foreground sticky top-0 z-10 border-b border-border bg-background/95 px-4 py-3 font-mono text-[11px] tracking-[0.18em] uppercase backdrop-blur"
	>
		Inspector
	</header>

	<div class="px-4 py-3">
		{#if !event}
			<p class="text-muted-foreground text-[12px] leading-relaxed">
				{goal
					? 'Select any line in the stream to inspect its payload — tool arguments, published message body, or wire detail.'
					: 'No run yet. The inspector shows the raw payload of whichever event you select.'}
			</p>
		{:else}
			{@const accent = accentOf(('agent' in event.event.kind && (event.event.kind as { agent?: string }).agent) || null)}
			<div class="mb-3 font-mono text-[12px]" style="color: {accent}">
				{('agent' in event.event.kind && (event.event.kind as { agent?: string }).agent) || 'system'}
			</div>

			{#each rows as r (r.k)}
				<div class="border-border/60 flex gap-2 border-b py-1.5">
					<span
						class="text-muted-foreground w-20 shrink-0 font-mono text-[10px] tracking-wider uppercase"
					>
						{r.k}
					</span>
					<span class="text-foreground min-w-0 flex-1 break-words font-mono text-[11.5px]">
						{r.v}
					</span>
				</div>
			{/each}

			<div class="mt-4">
				<span
					class="text-muted-foreground font-mono text-[10px] tracking-wider uppercase"
				>
					payload
				</span>
				<pre
					class="border-border bg-card text-foreground/90 mt-2 max-h-72 overflow-auto rounded-sm border p-3 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap">{payloadText || '(empty)'}</pre>
			</div>
		{/if}
	</div>

	{#if goal}
		<footer class="mt-auto border-t border-border px-4 py-3">
			<span class="text-muted-foreground font-mono text-[10px] tracking-wider uppercase">goal</span>
			<p class="text-foreground/80 mt-1 text-[12px] leading-relaxed">{goal}</p>
		</footer>
	{/if}
</div>
