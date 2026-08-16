<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import Header from '$lib/components/Header.svelte';
	import AgentCard from '$lib/components/AgentCard.svelte';
	import GoalInput from '$lib/components/GoalInput.svelte';
	import EventStream from '$lib/components/EventStream.svelte';
	import SettingsPanel from '$lib/components/SettingsPanel.svelte';
	import { agents as starterAgents } from '$lib/agents';
	import { submitGoal, subscribeToEvents, type StreamSource } from '$lib/api';
	import type { AgentLifecycle, EventEnvelope } from '$lib/types';

	let events: EventEnvelope[] = $state([]);
	let activeSessionId: string | null = $state(null);
	let streamSource: StreamSource | 'connecting' = $state('connecting');

	// Track the latest status per agent (derived from agent_status events).
	const agentStatus: Record<string, AgentLifecycle> = $state(
		Object.fromEntries(starterAgents.map((a) => [a.name.toLowerCase(), 'idle' as AgentLifecycle]))
	);

	let streamHandle: ReturnType<typeof subscribeToEvents> | null = null;

	function handleEvent(env: EventEnvelope) {
		events = [...events, env];
		const k = env.event.kind;
		if (k.type === 'agent_status') {
			agentStatus[k.agent.toLowerCase()] = k.status;
		}
		// In mock mode, adopt the first event's session_id as the active one
		// so the stream filter doesn't hide everything.
		if (!activeSessionId && env.event.session_id) {
			activeSessionId = env.event.session_id;
		}
	}

	onMount(() => {
		streamHandle = subscribeToEvents(
			(env) => handleEvent(env),
			(s) => (streamSource = s)
		);
	});

	onDestroy(() => {
		streamHandle?.close();
	});

	async function handleSubmit(goal: string) {
		// In mock mode, the form is decorative — the mock stream runs on its own.
		if (streamSource === 'mock') {
			// Clear any previous session so a new one starts.
			activeSessionId = null;
			events = [];
			// The first event from the new mock run will set activeSessionId.
			return;
		}
		try {
			const res = await submitGoal(goal);
			activeSessionId = res.session_id;
		} catch (err) {
			console.error('submit failed', err);
			alert(`Submit failed: ${err instanceof Error ? err.message : String(err)}`);
		}
	}

	function clearSession() {
		activeSessionId = null;
	}

	let settingsOpen = $state(false);
</script>

<Header
	stream={streamSource}
	sessionId={activeSessionId}
	onOpenSettings={() => (settingsOpen = true)}
/>

<SettingsPanel open={settingsOpen} onClose={() => (settingsOpen = false)} />

<main class="mx-auto flex h-[calc(100vh-3.5rem)] max-w-7xl gap-4 px-4 py-4">
	<aside class="hidden w-64 shrink-0 flex-col gap-3 lg:flex">
		<h2 class="text-base-content/70 text-xs font-semibold tracking-wide uppercase">Voices</h2>
		{#each starterAgents as agent (agent.name)}
			<AgentCard {agent} status={agentStatus[agent.name.toLowerCase()] ?? 'idle'} />
		{/each}
		<div class="text-base-content/40 mt-auto text-[11px] leading-relaxed">
			Each voice subscribes to channels and publishes results. The orchestrator routes events over
			Redis pub/sub.
		</div>
	</aside>

	<section class="flex min-w-0 flex-1 flex-col gap-3">
		<div class="flex items-center justify-between gap-2">
			<GoalInput onSubmit={handleSubmit} />
			{#if activeSessionId}
				<button
					type="button"
					class="text-base-content/50 hover:text-base-content/80 self-start rounded-md px-2 py-1 text-xs underline"
					onclick={clearSession}
				>
					new session
				</button>
			{/if}
		</div>
		<EventStream {events} {activeSessionId} />
	</section>
</main>
