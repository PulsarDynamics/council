<script lang="ts">
	import type { AgentSummary } from '$lib/agents';
	import type { AgentLifecycle } from '$lib/types';
	import StatusPill from './StatusPill.svelte';

	interface Props {
		agent: AgentSummary;
		status: AgentLifecycle;
	}
	let { agent, status }: Props = $props();

	const accent: Record<string, string> = {
		planner: 'border-l-sky-400',
		designer: 'border-l-violet-400',
		implementer: 'border-l-emerald-400'
	};
</script>

<article
	class="border-base-300/60 bg-base-200/40 hover:border-base-300 rounded-md border border-l-2 p-3 transition {accent[
		agent.name.toLowerCase()
	] ?? 'border-l-zinc-400'}"
>
	<header class="flex items-center justify-between gap-2">
		<h3 class="text-sm font-semibold">{agent.name}</h3>
		<StatusPill {status} />
	</header>
	<p class="text-base-content/60 mt-1 text-xs leading-relaxed">{agent.role}</p>
	<div class="text-base-content/50 mt-2 flex flex-wrap gap-1 font-mono text-[10px]">
		<span class="bg-base-300/50 rounded px-1.5 py-0.5">in: {agent.subscribes.join(', ')}</span>
		<span class="bg-base-300/50 rounded px-1.5 py-0.5">out: {agent.publishes.join(', ')}</span>
	</div>
</article>
