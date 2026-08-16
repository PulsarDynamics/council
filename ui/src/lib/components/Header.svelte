<script lang="ts">
	import type { StreamSource } from '$lib/api';
	import StatusPill from './StatusPill.svelte';

	interface Props {
		stream: StreamSource | 'connecting';
		sessionId: string | null;
	}
	let { stream, sessionId }: Props = $props();
</script>

<header
	class="border-base-300/60 bg-base-100/60 flex items-center justify-between border-b px-6 py-3"
>
	<div class="flex items-center gap-3">
		<div class="flex items-center gap-2">
			<svg
				viewBox="0 0 24 24"
				class="text-primary size-5"
				fill="none"
				stroke="currentColor"
				stroke-width="2"
			>
				<circle cx="12" cy="5" r="2" />
				<circle cx="5" cy="18" r="2" />
				<circle cx="19" cy="18" r="2" />
				<path d="M12 7 L 6 16 M 12 7 L 18 16" />
			</svg>
			<h1 class="text-base font-semibold tracking-tight">Council</h1>
		</div>
		<span class="text-base-content/40 text-xs">a roundtable of agents</span>
	</div>
	<div class="flex items-center gap-2 text-xs">
		{#if sessionId}
			<span class="text-base-content/50 font-mono">{sessionId.slice(0, 8)}</span>
		{/if}
		{#if stream === 'orchestrator'}
			<StatusPill status="connected" label="orchestrator" />
		{:else if stream === 'mock'}
			<StatusPill status="mock" label="mock stream" />
		{:else}
			<StatusPill status="disconnected" label="connecting…" />
		{/if}
	</div>
</header>
