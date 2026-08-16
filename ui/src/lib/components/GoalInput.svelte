<script lang="ts">
	interface Props {
		onSubmit: (goal: string) => void | Promise<void>;
		disabled?: boolean;
	}
	let { onSubmit, disabled = false }: Props = $props();

	let goal = $state('');
	let submitting = $state(false);

	async function submit(event?: Event) {
		event?.preventDefault();
		const trimmed = goal.trim();
		if (!trimmed || submitting) return;
		submitting = true;
		try {
			await onSubmit(trimmed);
			goal = '';
		} finally {
			submitting = false;
		}
	}

	function onKeydown(event: KeyboardEvent) {
		if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
			event.preventDefault();
			void submit();
		}
	}
</script>

<form
	onsubmit={submit}
	class="border-base-300/60 bg-base-200/30 flex flex-col gap-2 rounded-md border p-3"
>
	<label for="goal" class="text-base-content/70 text-xs font-medium tracking-wide uppercase">
		Goal
		<span class="text-base-content/40 ml-2 normal-case opacity-70">⌘⏎ to send</span>
	</label>
	<textarea
		id="goal"
		bind:value={goal}
		onkeydown={onKeydown}
		rows="3"
		placeholder="e.g. add a Stripe webhook handler that records failed payments in the audit log"
		class="border-base-300/60 bg-base-100/60 focus:border-primary/60 focus:ring-primary/30 resize-y rounded-md border px-3 py-2 font-sans text-sm focus:ring-1 focus:outline-none"
		{disabled}></textarea>
	<div class="flex items-center justify-between">
		<p class="text-base-content/40 text-[11px]">
			Sessions, plans, and code edits will stream below in real time.
		</p>
		<button
			type="submit"
			disabled={!goal.trim() || submitting || disabled}
			class="bg-primary text-primary-content hover:bg-primary/90 rounded-md px-3 py-1.5 text-xs font-semibold transition disabled:cursor-not-allowed disabled:opacity-40"
		>
			{submitting ? 'Sending…' : 'Send to Council'}
		</button>
	</div>
</form>
