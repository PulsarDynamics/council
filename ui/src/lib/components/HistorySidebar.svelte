<script lang="ts">
	import { onMount } from 'svelte';
	import { listSessions, type SessionMeta } from '$lib/api';
	import { getSessionEvents, type StreamSource } from '$lib/api';
	import EventItem from './EventItem.svelte';
	import type { EventEnvelope } from '$lib/types';

	interface Props {
		open: boolean;
		onClose: () => void;
		onPickSession?: (sessionId: string) => void;
	}
	let { open, onClose, onPickSession }: Props = $props();

	let sessions: SessionMeta[] = $state([]);
	let loadError = $state('');
	let activeSessionId: string | null = $state(null);
	let activeEvents: EventEnvelope[] = $state([]);
	let loadingEvents = $state(false);

	async function refresh() {
		loadError = '';
		try {
			sessions = await listSessions();
		} catch (e) {
			loadError = e instanceof Error ? e.message : String(e);
			sessions = [];
		}
	}

	async function pickSession(id: string) {
		activeSessionId = id;
		loadingEvents = true;
		try {
			activeEvents = await getSessionEvents(id);
		} catch (e) {
			loadError = e instanceof Error ? e.message : String(e);
			activeEvents = [];
		} finally {
			loadingEvents = false;
		}
		onPickSession?.(id);
	}

	function relativeTime(iso: string): string {
		try {
			const d = new Date(iso);
			const diff = (Date.now() - d.getTime()) / 1000;
			if (diff < 60) return `${Math.floor(diff)}s ago`;
			if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
			if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
			return d.toLocaleDateString();
		} catch {
			return iso;
		}
	}

	onMount(() => {
		if (open) refresh();
	});

	$effect(() => {
		if (open) refresh();
	});
</script>

{#if open}
	<button
		type="button"
		aria-label="Close history"
		class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"
		onclick={onClose}
	></button>

	<aside
		class="bg-base-100 border-base-300/60 fixed top-0 right-0 z-50 flex h-full w-full max-w-2xl flex-col border-l shadow-2xl"
	>
		<header class="border-base-300/60 flex items-center justify-between border-b px-5 py-3">
			<div>
				<h2 class="text-base font-semibold">History</h2>
				<p class="text-base-content/50 text-xs">last 50 sessions · 24h TTL</p>
			</div>
			<div class="flex items-center gap-2">
				<button
					type="button"
					class="hover:bg-base-300/60 rounded-md px-2 py-1 text-xs"
					onclick={refresh}>refresh</button
				>
				<button
					type="button"
					class="hover:bg-base-300/60 rounded-md px-2 py-1 text-xs"
					onclick={onClose}>close</button
				>
			</div>
		</header>

		<div class="flex-1 overflow-y-auto">
			{#if loadError}
				<p class="text-rose-400 px-5 py-3 text-xs">{loadError}</p>
			{/if}
			{#if sessions.length === 0 && !loadError}
				<p class="text-base-content/40 px-5 py-6 text-center text-sm">
					No sessions yet. Submit a goal to start one.
				</p>
			{:else}
				<ul class="divide-base-300/40 divide-y">
					{#each sessions as s (s.id)}
						<li>
							<button
								type="button"
								class="hover:bg-base-200/40 w-full px-5 py-3 text-left transition"
								class:bg-base-200={activeSessionId === s.id}
								onclick={() => pickSession(s.id)}
							>
								<div class="flex items-baseline justify-between gap-2">
									<div class="text-sm font-medium">
										{s.goal.length > 80 ? s.goal.slice(0, 80) + '…' : s.goal}
									</div>
									<div class="text-base-content/40 text-[10px]">
										{relativeTime(s.created_at)}
									</div>
								</div>
								<div class="text-base-content/50 mt-1 flex items-center gap-2 text-[11px]">
									<span
										class="rounded px-1.5 py-0.5 font-mono text-[10px]"
										class:bg-emerald-500={s.status === 'completed'}
										class:text-emerald-200={s.status === 'completed'}
										class:bg-zinc-500={s.status !== 'completed'}
										class:text-zinc-200={s.status !== 'completed'}
									>
										{s.status}
									</span>
									<span>{s.event_count} events</span>
									<span class="font-mono">{s.id.slice(0, 8)}</span>
								</div>
							</button>
						</li>
					{/each}
				</ul>

				{#if activeSessionId && (loadingEvents || activeEvents.length > 0)}
					<div class="border-base-300/60 border-t p-3">
						<h3 class="text-base-content/70 mb-2 text-xs font-semibold tracking-wide uppercase">
							Events for {activeSessionId.slice(0, 8)}
						</h3>
						{#if loadingEvents}
							<p class="text-base-content/50 text-xs">loading…</p>
						{:else}
							<div class="flex max-h-96 flex-col gap-2 overflow-y-auto">
								{#each activeEvents as env (env.event.id)}
									<EventItem envelope={env} />
								{/each}
							</div>
						{/if}
					</div>
				{/if}
			{/if}
		</div>
	</aside>
{/if}
