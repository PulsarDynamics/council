<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { accentOf } from '$lib/agentColors';
	import { cn } from '$lib/cn';
	import type { EventEnvelope, EventKind } from '$lib/types';

	interface Props {
		events: EventEnvelope[];
		activeSessionId: string | null;
		selectedEventId?: string | null;
		agentFilter?: string[];
		channelFilter?: string[];
		onSelect?: (id: string) => void;
	}
	let {
		events,
		activeSessionId,
		selectedEventId = null,
		agentFilter = [],
		channelFilter = [],
		onSelect
	}: Props = $props();

	let scrollEl: HTMLElement | null = $state(null);
	let stickToBottom = $state(true);

	// Streaming buffers: session_id -> agent_name -> accumulated text.
	// Deltas are appended here; the final AgentMessage for that
	// (session, agent) pair (or any non-delta event for that session)
	// clears the buffer because the canonical message has been (or is
	// about to be) published into `events`.
	let streamingBufs: Record<string, Record<string, string>> = $state({});

	// Per-kind glyph + channel for the terminal-style row.
	type DisplayKind = 'message' | 'thinking' | 'tool' | 'status' | 'system' | 'error' | 'llm' | 'session' | 'file' | 'delta';
	function classify(k: EventKind): DisplayKind {
		switch (k.type) {
			case 'agent_message':
			case 'user_message':
				return 'message';
			case 'agent_message_delta':
				return 'delta';
			case 'agent_thinking':
				return 'thinking';
			case 'tool_call':
			case 'tool_result':
				return 'tool';
			case 'agent_status':
				return 'status';
			case 'system':
				return 'system';
			case 'error':
				return 'error';
			case 'llm_call':
				return 'llm';
			case 'session_created':
			case 'session_completed':
			case 'session_cancelled':
				return 'session';
			case 'file_change':
				return 'file';
		}
	}

	function glyphFor(k: EventKind): string {
		switch (k.type) {
			case 'agent_message':
			case 'user_message':
			case 'agent_message_delta':
				return '»';
			case 'agent_thinking':
				return '?';
			case 'tool_call':
				return '⚙';
			case 'tool_result':
				return '←';
			case 'agent_status':
				return '·';
			case 'system':
				return '·';
			case 'error':
				return '!';
			case 'llm_call':
				return '◊';
			case 'session_created':
				return '+';
			case 'session_completed':
				return '✓';
			case 'session_cancelled':
				return '×';
			case 'file_change':
				return '✎';
		}
	}

	function rowFor(k: EventKind): string {
		switch (k.type) {
			case 'agent_message':
			case 'user_message':
			case 'agent_thinking':
				return k.content;
			case 'agent_message_delta':
				return k.delta;
			case 'tool_call':
				return JSON.stringify(k.args ?? {}).slice(0, 240);
			case 'tool_result': {
				const s = k.error ? `error: ${k.error}` : JSON.stringify(k.result).slice(0, 240);
				return s;
			}
			case 'file_change':
				return `${k.kind} ${k.path}`;
			case 'agent_status':
				return `${k.agent} → ${k.status}`;
			case 'system':
				return k.message;
			case 'error':
				return `${k.source}: ${k.message}`;
			case 'llm_call':
				return `${k.model} — ${k.prompt_tokens} in / ${k.completion_tokens} out · ${k.duration_ms}ms`;
			case 'session_created':
				return `goal: ${k.goal}`;
			case 'session_completed':
				return k.summary;
			case 'session_cancelled':
				return k.reason;
		}
	}

	function channelOf(k: EventKind): string | null {
		// We don't currently emit a `channel` field on our wire events
		// (the orchestrator uses "broadcast" for everything, with
		// per-agent publishes going to a dedicated channel — but the
		// published `channel` is on the envelope, not the event). For
		// now, return null; if we wire it through later, the FlowGraph
		// will light up.
		if (k.type === 'agent_message' || k.type === 'agent_message_delta' || k.type === 'agent_thinking' || k.type === 'agent_status' || k.type === 'tool_call' || k.type === 'tool_result') {
			return k.agent.toLowerCase();
		}
		return null;
	}

	function timeOf(env: EventEnvelope): string {
		try {
			const d = new Date(env.event.timestamp);
			return (
				String(d.getHours()).padStart(2, '0') +
				':' +
				String(d.getMinutes()).padStart(2, '0') +
				':' +
				String(d.getSeconds()).padStart(2, '0')
			);
		} catch {
			return '';
		}
	}

	function accentFor(env: EventEnvelope): string {
		const k = env.event.kind;
		if ('agent' in k) {
			return accentOf((k as { agent: string }).agent);
		}
		return 'var(--agent-council)';
	}

	// Filter + visible set.
	const visible = $derived.by(() => {
		const filtered = events.filter((e) => e.event.kind.type !== 'agent_message_delta');
		const sessionScoped = activeSessionId
			? filtered.filter((e) => e.event.session_id === activeSessionId)
			: filtered;
		const agentScoped = agentFilter.length
			? sessionScoped.filter((e) => {
					const k = e.event.kind;
					return 'agent' in k && agentFilter.includes((k as { agent: string }).agent.toLowerCase());
				})
			: sessionScoped;
		const channelScoped = channelFilter.length
			? agentScoped.filter((e) => {
					const c = channelOf(e.event.kind);
					return c !== null && channelFilter.includes(c);
				})
			: agentScoped;
		return channelScoped;
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
		items.reverse();
		return items;
	});

	// Maintain the streaming buffer as events flow in.
	$effect(() => {
		const next: Record<string, Record<string, string>> = {};
		for (const env of events) {
			const sid = env.event.session_id;
			const k = env.event.kind;
			if (k.type === 'agent_message_delta') {
				const agentMap = (next[sid] = next[sid] || {});
				agentMap[k.agent] = (agentMap[k.agent] || '') + k.delta;
			} else {
				if (next[sid]) next[sid] = {};
			}
		}
		streamingBufs = next;
	});

	$effect(() => {
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
	class="min-h-0 flex-1 overflow-y-auto px-2 py-2 font-mono text-[12.5px] leading-relaxed"
>
	{#if visible.length === 0 && streamingItems.length === 0}
		<div class="text-muted-foreground flex h-full flex-col items-center justify-center px-8 text-center">
			<p class="font-mono text-[11px] tracking-[0.2em] uppercase">bus idle</p>
			<p class="text-muted-foreground mt-3 text-sm leading-relaxed">
				Type a goal below and convene the council. Planner, designer and implementer will
				deliberate over the bus — every token, tool call and delegation streams here.
			</p>
		</div>
	{:else}
		<div class="flex flex-col">
			{#each visible as env (env.event.id)}
				{@const accent = accentFor(env)}
				<button
					type="button"
					onclick={() => onSelect?.(env.event.id)}
					style="--accent: {accent}"
					class={cn(
						'flex w-full items-start gap-2 rounded-sm px-2 py-1 text-left transition-colors hover:bg-card',
						selectedEventId === env.event.id && 'bg-card ring-1 ring-border'
					)}
				>
					<span class="text-muted-foreground/70 shrink-0 text-[11px]">{timeOf(env)}</span>
					<span
						class="w-3 shrink-0 text-center text-[11px]"
						style="color: {accent}"
					>
						{glyphFor(env.event.kind)}
					</span>
					{#if 'agent' in env.event.kind}
						<span
							class="w-[92px] shrink-0 truncate text-[11px]"
							style="color: {accent}"
						>
							{(env.event.kind as { agent: string }).agent}
						</span>
					{:else}
						<span class="text-muted-foreground w-[92px] shrink-0 truncate text-[11px]">system</span>
					{/if}
					<span class="min-w-0 flex-1 break-words">
						<span
							class={cn(
								env.event.kind.type === 'system' || env.event.kind.type === 'session_cancelled'
									? 'text-muted-foreground'
									: env.event.kind.type === 'error'
										? 'text-destructive'
										: 'text-foreground'
							)}
						>
							{rowFor(env.event.kind)}
						</span>
					</span>
				</button>
			{/each}
			{#each streamingItems as item (item.key)}
				{@const accent = accentOf(item.agent)}
				<button
					type="button"
					style="--accent: {accent}"
					class={cn(
						'flex w-full items-start gap-2 rounded-sm px-2 py-1 text-left',
						'bg-card/40 animate-pulse'
					)}
				>
					<span class="text-muted-foreground/70 shrink-0 text-[11px]">…</span>
					<span
						class="w-3 shrink-0 text-center text-[11px]"
						style="color: {accent}"
					>
						»
					</span>
					<span class="w-[92px] shrink-0 truncate text-[11px]" style="color: {accent}">
						{item.agent}
					</span>
					<span class="text-foreground/80 min-w-0 flex-1 break-words">{item.text}</span>
				</button>
			{/each}
		</div>
	{/if}

	{#if !stickToBottom && (visible.length > 0 || streamingItems.length > 0)}
		<button
			type="button"
			class="bg-primary text-primary-foreground absolute right-4 bottom-4 rounded-full px-3 py-1 text-xs font-medium shadow-lg"
			onclick={() => {
				stickToBottom = true;
				if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
			}}
		>
			Jump to latest
		</button>
	{/if}
</section>
