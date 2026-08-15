<script lang="ts">
	import { agents } from '$lib/agents';

	let goal = $state('');
	let submitting = $state(false);

	function submit(event: SubmitEvent) {
		event.preventDefault();
		// Scaffold: the real submit will POST to /sessions on the orchestrator,
		// which will fan out a `goal` event on Redis to all subscribed agents.
		submitting = true;
		console.info('goal submitted (scaffold — no orchestrator wired yet):', goal);
		setTimeout(() => (submitting = false), 400);
	}
</script>

<main class="mx-auto flex min-h-screen max-w-3xl flex-col gap-10 px-6 py-16">
	<header class="space-y-2">
		<h1 class="text-4xl font-semibold tracking-tight">Council</h1>
		<p class="text-base-content/70 text-lg">
			A roundtable of agents that plan, design, and implement. Set a goal — the Council
			deliberates.
		</p>
	</header>

	<form onsubmit={submit} class="flex flex-col gap-3">
		<label for="goal" class="text-sm font-medium">Goal</label>
		<textarea
			id="goal"
			bind:value={goal}
			rows="4"
			placeholder="e.g. add a Stripe webhook handler that records failed payments in the audit log"
			class="border-base-300 focus:border-primary focus:ring-primary rounded-md border bg-transparent px-3 py-2 text-sm focus:ring-1 focus:outline-none"
		></textarea>
		<button
			type="submit"
			disabled={!goal.trim() || submitting}
			class="bg-primary text-primary-content hover:bg-primary/90 self-start rounded-md px-4 py-2 text-sm font-medium transition disabled:cursor-not-allowed disabled:opacity-50"
		>
			{submitting ? 'Submitting…' : 'Send to Council'}
		</button>
	</form>

	<section class="space-y-3">
		<h2 class="text-sm font-semibold tracking-wide uppercase">Voices around the table</h2>
		<ul class="divide-base-300 border-base-300 divide-y rounded-md border">
			{#each agents as agent (agent.name)}
				<li class="flex items-center justify-between px-4 py-3 text-sm">
					<div>
						<div class="font-medium">{agent.name}</div>
						<div class="text-base-content/60 text-xs">{agent.role}</div>
					</div>
					<div class="text-base-content/60 font-mono text-xs">
						subscribes: {agent.subscribes.join(', ')}
					</div>
				</li>
			{/each}
		</ul>
	</section>

	<footer class="text-base-content/50 mt-auto text-xs">
		Scaffold build — live event stream and WebSocket land in cycle 2.
	</footer>
</main>
