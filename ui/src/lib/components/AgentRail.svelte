<script lang="ts">
	import { accentOf, agentsWithAccent, type AgentWithAccent } from '$lib/agentColors';
	import { cn } from '$lib/cn';
	import type { AgentLifecycle } from '$lib/types';

	interface Props {
		/** Map of agent id -> lifecycle status from the wire. */
		status: Record<string, AgentLifecycle>;
		/** Filter set: empty = show all. */
		filter: string[];
		onToggle?: (id: string) => void;
	}
	let { status, filter, onToggle }: Props = $props();

	const STATUS_LABEL: Record<AgentLifecycle, string> = {
		idle: 'idle',
		working: 'working',
		error: 'error',
		stopped: 'stopped',
		started: 'idle'
	};

	function liveOf(s: AgentLifecycle | undefined): boolean {
		return s === 'working' || s === 'started';
	}

	// Friendly display string per agent (role) — short, fits one line.
	function roleFor(a: AgentWithAccent): string {
		return a.role;
	}
</script>

<div class="flex h-full flex-col overflow-y-auto">
	<header
		class="sticky top-0 z-10 flex items-center justify-between border-b border-border bg-background/95 px-4 py-3 backdrop-blur"
	>
		<span class="font-mono text-[11px] tracking-[0.18em] text-muted-foreground uppercase">
			Agents
		</span>
		<span class="font-mono text-[11px] text-muted-foreground">
			{agentsWithAccent.length}
		</span>
	</header>

	<ul>
		{#each agentsWithAccent as agent (agent.id)}
			{@const st = status[agent.id] ?? 'idle'}
			{@const muted = filter.length > 0 && !filter.includes(agent.id)}
			{@const live = liveOf(st)}
			{@const accent = agent.accent}
			<li>
				<button
					type="button"
					onclick={() => onToggle?.(agent.id)}
					style="--accent: {accent}"
					class={cn(
						'group w-full border-b border-border px-4 py-3 text-left transition-colors hover:bg-card',
						muted && 'opacity-40',
						filter.includes(agent.id) && 'bg-card'
					)}
				>
					<div class="flex items-center gap-2">
						<span
							class={cn(
								'size-2 shrink-0 rounded-full',
								live ? 'animate-pulse' : st === 'error' ? 'bg-destructive' : 'bg-muted-foreground/30'
							)}
							style="background-color: {live ? accent : ''}"
						></span>
						<span class="font-mono text-[13px] font-medium" style="color: {accent}">
							{agent.name}
						</span>
					</div>
					<p class="text-muted-foreground mt-1 pl-4 text-[11px] leading-tight">
						{roleFor(agent)}
					</p>
					<div class="mt-2 flex flex-wrap items-center gap-1 pl-4 font-mono text-[10px] text-muted-foreground">
						{#each agent.subscribes as s (s)}
							<span class="bg-muted rounded-sm px-1 py-px">↓{s}</span>
						{/each}
						{#each agent.publishes as p (p)}
							<span class="bg-muted rounded-sm px-1 py-px">↑{p}</span>
						{/each}
					</div>
					<div class="mt-2 flex items-center justify-between pl-4 font-mono text-[10px]">
						<span
							class={cn(
								'tracking-wide',
								st === 'idle' || st === 'stopped' || st === 'started'
									? 'text-muted-foreground'
									: st === 'error'
										? 'text-destructive'
										: 'text-foreground'
							)}
						>
							{STATUS_LABEL[st] ?? st}
						</span>
					</div>
				</button>
			</li>
		{/each}
	</ul>

	<footer class="text-muted-foreground mt-auto px-4 py-3 font-mono text-[10px] leading-relaxed">
		click an agent to filter the stream
	</footer>
</div>
