<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import AgentRail from '$lib/components/AgentRail.svelte';
	import Composer from '$lib/components/Composer.svelte';
	import EventStream from '$lib/components/EventStream.svelte';
	import FlowGraph from '$lib/components/FlowGraph.svelte';
	import HistorySidebar from '$lib/components/HistorySidebar.svelte';
	import Inspector from '$lib/components/Inspector.svelte';
	import SettingsPanel from '$lib/components/SettingsPanel.svelte';
	import SwapProviderModal from '$lib/components/SwapProviderModal.svelte';
	import TopNav from '$lib/components/TopNav.svelte';
	import { agentsWithAccent } from '$lib/agentColors';
	import {
		cancelSession,
		submitGoal,
		subscribeToEvents,
		type StreamSource
	} from '$lib/api';
	import type { AgentLifecycle, EventEnvelope } from '$lib/types';

	/*
	 * Council console — 3-column layout, ported from the Council
	 * Chamber-1 visual rework.
	 *
	 *   ┌────────────────────────────────────────────────────────────────┐
	 *   │  COUNCIL · live · N events                       [ console … ] │  ← top bar
	 *   ├──────────┬───────────────────────────────────┬─────────────────┤
	 *   │          │  bus  ●── goal ─ plan ─ spec ─ …   │                 │
	 *   │ Agents   ├───────────────────────────────────┤   Inspector     │
	 *   │  ● plan  │                                   │                 │
	 *   │  ● desi  │       [event stream]              │                 │
	 *   │  ● impl  │                                   │                 │
	 *   │          │                                   │                 │
	 *   ├──────────┴───────────────────────────────────┴─────────────────┤
	 *   │   [textarea: state a goal…]  [suggestions…]  [×] [↺] [convene →] │  ← composer
	 *   └────────────────────────────────────────────────────────────────┘
	 *
	 * Wires up to the real orchestrator over WebSocket; cancel / history /
	 * settings remain modal-style so we don't have to carve out full
	 * route pages for them in this pass.
	 */

	let events: EventEnvelope[] = $state([]);
	let activeSessionId: string | null = $state(null);
	let activeGoal: string = $state('');
	let streamSource: StreamSource | 'connecting' = $state('connecting');

	// Per-agent lifecycle + accumulated token counts. Token totals are
	// derived from `llm_call` events; the rail shows the latest status
	// (which the agent publishes via `agent_status`).
	const agentStatus: Record<string, AgentLifecycle> = $state(
		Object.fromEntries(agentsWithAccent.map((a) => [a.id, 'idle' as AgentLifecycle]))
	);

	// Selection state (drives the Inspector).
	let selectedEventId: string | null = $state(null);
	const selectedEvent = $derived(
		selectedEventId
			? events.find((e) => e.event.id === selectedEventId) ?? null
			: null
	);

	// Filters.
	let agentFilter: string[] = $state([]);
	let channelFilter: string[] = $state([]);

	// Per-channel traffic timestamps (ms) for the FlowGraph's hot-pulse.
	let channelTraffic: Record<string, number> = $state({});

	// A session is "busy" from SessionCreated until SessionCompleted /
	// SessionCancelled. The Composer disables goal input while busy.
	const sessionTerminated = $derived.by(() => {
		const set = new Set<string>();
		for (const env of events) {
			const k = env.event.kind;
			if (
				env.event.session_id === activeSessionId &&
				(k.type === 'session_completed' || k.type === 'session_cancelled')
			) {
				set.add(env.event.session_id);
			}
		}
		return set;
	});

	let composerGoal: string = $state('');
	let cancelling = $state(false);
	let settingsOpen = $state(false);
	let swapTarget: string | null = $state(null);
	let historyOpen = $state(false);

	let streamHandle: ReturnType<typeof subscribeToEvents> | null = null;

	function trackEvent(env: EventEnvelope) {
		events = [...events, env];
		const k = env.event.kind;

		if (k.type === 'agent_status') {
			agentStatus[k.agent.toLowerCase()] = k.status;
		} else if (k.type === 'llm_call') {
			// We don't currently have per-tool channel routing on the wire
			// (everything goes via "broadcast"). Tick the broadcast pill
			// on the FlowGraph so it lights up whenever the LLM runs.
			channelTraffic = { ...channelTraffic, broadcast: Date.now() };
		} else if (k.type === 'session_created') {
			if (!activeSessionId || env.event.session_id === activeSessionId) {
				activeSessionId = env.event.session_id;
				activeGoal = k.goal;
			}
		} else if (k.type === 'session_completed' || k.type === 'session_cancelled') {
			// keep activeSessionId so the stream stays visible; the
			// Composer uses sessionTerminated to hide the cancel button.
		} else if (k.type === 'agent_message_delta') {
			channelTraffic = { ...channelTraffic, broadcast: Date.now() };
		}

		if (!activeSessionId && env.event.session_id) {
			activeSessionId = env.event.session_id;
		}
	}

	onMount(() => {
		streamHandle = subscribeToEvents(
			(env) => trackEvent(env),
			(s) => (streamSource = s)
		);
	});

	onDestroy(() => {
		streamHandle?.close();
	});

	async function handleSubmit() {
		const goal = composerGoal.trim();
		if (!goal) return;
		if (streamSource === 'mock') {
			activeSessionId = null;
			events = [];
			composerGoal = '';
			return;
		}
		try {
			const res = await submitGoal(goal);
			activeSessionId = res.session_id;
			activeGoal = goal;
			composerGoal = '';
		} catch (err) {
			console.error('submit failed', err);
			alert(`Submit failed: ${err instanceof Error ? err.message : String(err)}`);
		}
	}

	async function handleCancel() {
		if (!activeSessionId || cancelling) return;
		if (streamSource === 'mock') {
			activeSessionId = null;
			events = [];
			return;
		}
		cancelling = true;
		try {
			await cancelSession({ session_id: activeSessionId, reason: 'user cancelled' });
		} catch (err) {
			console.error('cancel failed', err);
			alert(`Cancel failed: ${err instanceof Error ? err.message : String(err)}`);
		} finally {
			cancelling = false;
		}
	}

	function handleNewSession() {
		activeSessionId = null;
		events = [];
		activeGoal = '';
		selectedEventId = null;
		composerGoal = '';
	}

	function toggleAgent(id: string) {
		agentFilter = agentFilter.includes(id)
			? agentFilter.filter((x) => x !== id)
			: [...agentFilter, id];
	}

	function toggleChannel(c: string) {
		channelFilter = channelFilter.includes(c)
			? channelFilter.filter((x) => x !== c)
			: [...channelFilter, c];
	}

	function openSettings() {
		settingsOpen = true;
	}
	function openHistory() {
		historyOpen = true;
	}
	function handleNav(target: 'console' | 'history' | 'settings') {
		if (target === 'history') openHistory();
		else if (target === 'settings') openSettings();
		// 'console' is the current page — no-op.
	}

	// Status word at the top of the header.
	const statusLabel = $derived.by(() => {
		if (!activeSessionId) return 'standby';
		if (sessionTerminated.has(activeSessionId)) return 'adjourned';
		return 'deliberating';
	});

	const isLive = $derived(
		!!activeSessionId && !sessionTerminated.has(activeSessionId)
	);

	const sourceLabel = $derived.by(() => {
		if (streamSource === 'orchestrator') return 'live';
		if (streamSource === 'mock') return 'mock stream';
		return 'connecting…';
	});
</script>

<SettingsPanel open={settingsOpen} onClose={() => (settingsOpen = false)} />
<SwapProviderModal
	open={swapTarget !== null}
	agent={swapTarget}
	onClose={() => (swapTarget = null)}
/>
<HistorySidebar
	open={historyOpen}
	onClose={() => (historyOpen = false)}
	onPickSession={(id) => {
		activeSessionId = id;
		historyOpen = false;
	}}
/>

<header
	class="border-border bg-background/95 sticky top-0 z-20 flex items-center gap-4 border-b px-4 py-3 backdrop-blur"
>
	<div class="flex items-center gap-2">
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
	</div>
	<span class="text-muted-foreground hidden font-mono text-[11px] sm:inline">
		multi-agent orchestration · {sourceLabel}
	</span>

	<div class="ml-auto flex items-center gap-3 font-mono text-[11px]">
		<span
			class="size-1.5 rounded-full {isLive
				? 'bg-primary animate-pulse'
				: 'bg-muted-foreground/50'}"
		></span>
		<span class="text-muted-foreground">{statusLabel}</span>
		<span class="text-muted-foreground">{events.length} events</span>
		<TopNav active="console" onNavigate={handleNav} />
	</div>
</header>

<main class="flex h-[calc(100vh-3.5rem)] flex-col">
	<div class="grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-[240px_minmax(0,1fr)_300px]">
		<aside class="border-border hidden min-h-0 border-r lg:block">
			<AgentRail
				status={agentStatus}
				filter={agentFilter}
				onToggle={toggleAgent}
			/>
		</aside>

		<section class="flex min-h-0 flex-col">
			<FlowGraph
				traffic={channelTraffic}
				active={channelFilter}
				onToggle={toggleChannel}
			/>
			<EventStream
				{events}
				{activeSessionId}
				{selectedEventId}
				{agentFilter}
				{channelFilter}
				onSelect={(id) => (selectedEventId = id)}
			/>
		</section>

		<aside class="border-border hidden min-h-0 border-l lg:block">
			<Inspector event={selectedEvent} goal={activeGoal} />
		</aside>
	</div>

	<Composer
		busy={!!activeSessionId && !sessionTerminated.has(activeSessionId)}
		active={!!activeSessionId}
		{cancelling}
		goal={composerGoal}
		onGoalChange={(g) => (composerGoal = g)}
		onSubmit={handleSubmit}
		onCancel={handleCancel}
		onNewSession={handleNewSession}
	/>
</main>
