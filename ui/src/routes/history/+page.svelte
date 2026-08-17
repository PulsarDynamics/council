<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import {
		forkSession,
		getSessionEvents,
		listSessions,
		type SessionMeta
	} from '$lib/api';
	import EventItem from '$lib/components/EventItem.svelte';
	import { cn } from '$lib/cn';
	import type { EventEnvelope } from '$lib/types';

	/*
	 * /history — full-page session history.
	 *
	 *   ┌────────────────────────────────────────────────────────────────┐
	 *   │   50 sessions                  │   events for {id}            │
	 *   │   ┌──────────────────────────┐ │   ┌────────────────────────┐ │
	 *   │   │  goal   12m   completed  │ │   │  EventItem             │ │
	 *   │   │  goal   1h    running    │ │   │  EventItem             │ │
	 *   │   └──────────────────────────┘ │   │  …                     │ │
	 *   │                                │   └────────────────────────┘ │
	 *   │                                │   [view in console] [fork]   │
	 *   └────────────────────────────────────────────────────────────────┘
	 *
	 * Click a row to load its events on the right. "View in console"
	 * navigates to /?session=<id> so the live stream picks it up.
	 */

	let sessions: SessionMeta[] = $state([]);
	let loadError = $state('');
	let activeSessionId: string | null = $state(null);
	let activeEvents: EventEnvelope[] = $state([]);
	let loadingEvents = $state(false);

	// Per-session fork state.
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
		// Reflect the active session in the URL so the page is
		// deep-linkable / refreshable.
		const url = new URL(page.url);
		url.searchParams.set('session', id);
		goto(url.pathname + url.search, { replaceState: true, noScroll: true });
	}

	async function forkOne(id: string) {
		if (forkingIds.has(id)) return;
		const next = new Set(forkingIds);
		next.add(id);
		forkingIds = next;
		forkToast = null;
		try {
			const res = await forkSession({ source_session_id: id, agent: 'planner' });
			forkToast = { kind: 'ok', message: res.message };
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

	function viewInConsole(id: string) {
		goto(`/?session=${encodeURIComponent(id)}`);
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
		refresh().then(() => {
			// If the URL has ?session=<id>, pre-load that session's events.
			const target = page.url.searchParams.get('session');
			if (target && sessions.some((s) => s.id === target)) {
				pickSession(target);
			}
		});
	});
</script>

<svelte:head>
	<title>History · Council</title>
</svelte:head>

<main class="flex h-[calc(100vh-3.5rem)] flex-col">
	<div class="grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-[360px_minmax(0,1fr)]">
		<!-- Left: session list -->
		<aside class="border-border flex min-h-0 flex-col border-r">
			<header
				class="border-border bg-background/95 sticky top-0 z-10 flex items-center justify-between border-b px-4 py-3 backdrop-blur"
			>
				<div>
					<h2 class="font-mono text-[12px] font-semibold tracking-[0.18em] uppercase">
						History
					</h2>
					<p class="text-muted-foreground mt-0.5 font-mono text-[10px]">
						last 50 sessions · 24h TTL
					</p>
				</div>
				<button
					type="button"
					class="hover:bg-card text-muted-foreground rounded-md px-2 py-1 font-mono text-[11px] hover:text-foreground"
					onclick={refresh}>refresh</button
				>
			</header>

			<div class="flex-1 overflow-y-auto">
				{#if loadError}
					<p class="text-destructive px-4 py-3 font-mono text-[11px]">{loadError}</p>
				{/if}
				{#if forkToast}
					<div
						class="px-4 py-2 font-mono text-[11px]"
						class:text-emerald-400={forkToast.kind === 'ok'}
						class:text-destructive={forkToast.kind === 'err'}
					>
						{forkToast.message}
					</div>
				{/if}
				{#if sessions.length === 0 && !loadError}
					<p class="text-muted-foreground px-4 py-6 text-center font-mono text-[12px]">
						No sessions yet. Submit a goal from the console to start one.
					</p>
				{:else}
					<ul>
						{#each sessions as s (s.id)}
							{@const active = activeSessionId === s.id}
							<li
								class={cn(
									'group border-b border-border transition-colors',
									active && 'bg-card'
								)}
							>
								<button
									type="button"
									class="hover:bg-card flex w-full items-stretch text-left"
									onclick={() => pickSession(s.id)}
								>
									<div class="min-w-0 flex-1 px-4 py-3">
										<div class="flex items-baseline justify-between gap-2">
											<div class="truncate font-mono text-[12px] font-medium">
												{s.goal.length > 60 ? s.goal.slice(0, 60) + '…' : s.goal}
											</div>
											<div class="text-muted-foreground shrink-0 font-mono text-[10px]">
												{relativeTime(s.created_at)}
											</div>
										</div>
										<div
											class="text-muted-foreground mt-1 flex items-center gap-2 font-mono text-[10px]"
										>
											<span
												class="rounded px-1.5 py-0.5"
												class:bg-emerald-500={s.status === 'completed'}
												class:text-emerald-200={s.status === 'completed'}
												class:bg-muted={s.status !== 'completed'}
												class:text-muted-foreground={s.status !== 'completed'}
											>
												{s.status}
											</span>
											<span>{s.event_count} events</span>
											<span class="text-muted-foreground/60">{s.id.slice(0, 8)}</span>
										</div>
									</div>
								</button>
								<div class="flex items-center justify-end gap-1 px-4 pb-2">
									<button
										type="button"
										class="text-muted-foreground hover:text-foreground rounded-md border border-border px-2 py-0.5 font-mono text-[10px] uppercase tracking-wide opacity-60 transition group-hover:opacity-100 disabled:opacity-40"
										disabled={forkingIds.has(s.id)}
										title="Fork this session into a new one with the same context"
										onclick={(e) => {
											e.stopPropagation();
											forkOne(s.id);
										}}
									>
										{forkingIds.has(s.id) ? 'forking…' : 'fork'}
									</button>
									<button
										type="button"
										class="bg-primary text-primary-foreground hover:bg-primary/90 rounded-md px-2 py-0.5 font-mono text-[10px] uppercase tracking-wide opacity-60 transition group-hover:opacity-100"
										title="Open this session in the live console"
										onclick={(e) => {
											e.stopPropagation();
											viewInConsole(s.id);
										}}
									>
										open →
									</button>
								</div>
							</li>
						{/each}
					</ul>
				{/if}
			</div>
		</aside>

		<!-- Right: events for selected session -->
		<section class="flex min-h-0 flex-col">
			<header
				class="border-border bg-background/95 sticky top-0 z-10 flex items-center justify-between border-b px-4 py-3 backdrop-blur"
			>
				<div class="min-w-0">
					<h2 class="font-mono text-[12px] font-semibold tracking-[0.18em] uppercase">
						{activeSessionId ? 'Events' : 'No session selected'}
					</h2>
					{#if activeSessionId}
						<p class="text-muted-foreground mt-0.5 truncate font-mono text-[10px]">
							{activeSessionId}
						</p>
					{/if}
				</div>
				{#if activeSessionId}
					<button
						type="button"
						class="bg-primary text-primary-foreground hover:bg-primary/90 rounded-md px-3 py-1 font-mono text-[11px] font-semibold"
						onclick={() => viewInConsole(activeSessionId!)}
					>
						view in console →
					</button>
				{/if}
			</header>

			<div class="flex-1 overflow-y-auto p-4">
				{#if !activeSessionId}
					<p class="text-muted-foreground py-12 text-center font-mono text-[12px]">
						Pick a session on the left to inspect its events.
					</p>
				{:else if loadingEvents}
					<p class="text-muted-foreground py-12 text-center font-mono text-[12px]">
						loading…
					</p>
				{:else if activeEvents.length === 0}
					<p class="text-muted-foreground py-12 text-center font-mono text-[12px]">
						No events recorded for this session.
					</p>
				{:else}
					<div class="flex flex-col gap-2">
						{#each activeEvents as env (env.event.id)}
							<EventItem envelope={env} />
						{/each}
					</div>
				{/if}
			</div>
		</section>
	</div>
</main>
