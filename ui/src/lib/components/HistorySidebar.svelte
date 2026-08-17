<script lang="ts">
	import { onMount } from 'svelte';
	import { listSessions, type SessionMeta } from '$lib/api';
	import { getSessionEvents, forkSession, type StreamSource } from '$lib/api';
	import EventItem from './EventItem.svelte';
	import type { EventEnvelope } from '$lib/types';

	interface Props {
		open: boolean;
		onClose: () => void;
		onPickSession?: (sessionId: string) => void;
		/**
		 * Which agent to fork into. The default ("planner") is the
		 * entry point agent in the current TOML config; users with
		 * other configurations can pass a different name.
		 */
		defaultForkAgent?: string;
	}
	let {
		open,
		onClose,
		onPickSession,
		defaultForkAgent = 'planner'
	}: Props = $props();

	let sessions: SessionMeta[] = $state([]);
	let loadError = $state('');
	let activeSessionId: string | null = $state(null);
	let activeEvents: EventEnvelope[] = $state([]);
	let loadingEvents = $state(false);

	// Per-session fork state: which session id (if any) is currently
	// dispatching a fork, and the most recent toast (success or error).
	// Kept as a map keyed by session id so concurrent forks of
	// different sessions don't trample each other.
	let forkingIds: Set<string> = $state(new Set());
	let forkToast: { kind: 'ok' | 'err'; message: string } | null = $state(null);

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

	async function forkOne(id: string) {
		if (forkingIds.has(id)) return;
		// Svelte 5: mutate a Set in place by reassigning the property
		// the rune is bound to. A new Set keeps the reactivity simple.
		const next = new Set(forkingIds);
		next.add(id);
		forkingIds = next;
		forkToast = null;
		try {
			const res = await forkSession({
				source_session_id: id,
				agent: defaultForkAgent
			});
			forkToast = { kind: 'ok', message: res.message };
			// The new session is published as SessionCreated on the
			// broadcast channel; the orchestrator's persistence task
			// writes a fresh meta hash and the sidebar will pick it
			// up on the next refresh. Refresh immediately for UX.
			await refresh();
		} catch (e) {
			forkToast = {
				kind: 'err',
				message: e instanceof Error ? e.message : String(e)
			};
		} finally {
			const after = new Set(forkingIds);
			after.delete(id);
			forkingIds = after;
		}
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
			{#if forkToast}
				<div
					class="px-5 py-2 text-xs"
					class:text-emerald-300={forkToast.kind === 'ok'}
					class:text-rose-300={forkToast.kind === 'err'}
				>
					{forkToast.message}
				</div>
			{/if}
			{#if sessions.length === 0 && !loadError}
				<p class="text-base-content/40 px-5 py-6 text-center text-sm">
					No sessions yet. Submit a goal to start one.
				</p>
			{:else}
				<ul class="divide-base-300/40 divide-y">
					{#each sessions as s (s.id)}
						<li class="group flex items-stretch">
							<button
								type="button"
								class="hover:bg-base-200/40 flex-1 px-5 py-3 text-left transition"
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
							<!-- Fork action: separate button so clicking it
							     doesn't toggle the active session. Disabled
							     while a fork is in flight for this row. -->
							<div class="flex items-center pr-3">
								<button
									type="button"
									class="rounded-md border border-base-300/60 px-2 py-1 text-[10px] uppercase tracking-wide opacity-60 transition group-hover:opacity-100 hover:bg-base-200 disabled:cursor-not-allowed disabled:opacity-40"
									disabled={forkingIds.has(s.id)}
									aria-label="Fork session {s.id.slice(0, 8)}"
									title="Fork this session into a new one with the same context"
									onclick={() => forkOne(s.id)}
								>
									{forkingIds.has(s.id) ? 'forking…' : 'fork'}
								</button>
							</div>
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
