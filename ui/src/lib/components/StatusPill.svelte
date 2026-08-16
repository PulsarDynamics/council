<script lang="ts">
	import type { AgentLifecycle } from '$lib/types';

	type Status = AgentLifecycle | 'unknown' | 'connected' | 'disconnected' | 'mock';

	interface Props {
		status: Status;
		label?: string;
	}
	let { status, label }: Props = $props();

	const colors: Record<Status, string> = {
		started: 'bg-sky-500/10 text-sky-300 border-sky-500/30',
		idle: 'bg-zinc-500/10 text-zinc-300 border-zinc-500/30',
		working: 'bg-emerald-500/10 text-emerald-300 border-emerald-500/30',
		stopped: 'bg-zinc-500/10 text-zinc-400 border-zinc-500/30',
		error: 'bg-rose-500/10 text-rose-300 border-rose-500/30',
		unknown: 'bg-zinc-500/10 text-zinc-400 border-zinc-500/30',
		connected: 'bg-emerald-500/10 text-emerald-300 border-emerald-500/30',
		disconnected: 'bg-rose-500/10 text-rose-300 border-rose-500/30',
		mock: 'bg-amber-500/10 text-amber-300 border-amber-500/30'
	};
	const dot: Record<Status, string> = {
		started: 'bg-sky-400',
		idle: 'bg-zinc-400',
		working: 'bg-emerald-400',
		stopped: 'bg-zinc-500',
		error: 'bg-rose-400',
		unknown: 'bg-zinc-500',
		connected: 'bg-emerald-400',
		disconnected: 'bg-rose-400',
		mock: 'bg-amber-400'
	};
</script>

<span
	class="inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs font-medium {colors[
		status
	]}"
>
	<span class="inline-block size-1.5 rounded-full {dot[status]}"></span>
	{label ?? status}
</span>
