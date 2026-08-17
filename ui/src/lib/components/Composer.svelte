<script lang="ts">
	import { cn } from '$lib/cn';

	/**
	 * Bottom composer. Two roles:
	 *   1. Goal input: textarea + "convene" button + suggestion chips.
	 *   2. (When there's an active session) cancel + new-session controls.
	 *
	 * Ported from Council Chamber-1's Composer.tsx; reworked to talk to
	 * our real orchestrator (no pause/resume wired in our wire yet, so
	 * only cancel + reset are exposed).
	 */
	interface Props {
		busy: boolean;
		active: boolean;
		cancelling?: boolean;
		goal: string;
		onGoalChange: (g: string) => void;
		onSubmit: () => void;
		onCancel?: () => void;
		onNewSession?: () => void;
	}
	let {
		busy,
		active,
		cancelling = false,
		goal,
		onGoalChange,
		onSubmit,
		onCancel,
		onNewSession
	}: Props = $props();

	const SUGGESTIONS = [
		'Build a habit tracker with streaks',
		'Add an audit log to the admin panel',
		'Design an onboarding flow for teams'
	];

	function submit() {
		if (!goal.trim() || busy) return;
		onSubmit();
	}

	function onKey(e: KeyboardEvent) {
		if (e.key === 'Enter' && !e.shiftKey) {
			e.preventDefault();
			submit();
		}
	}
</script>

<div class="border-border bg-background border-t">
	<div class="flex items-end gap-3 px-4 py-3">
		<div class="min-w-0 flex-1">
			<textarea
				value={goal}
				oninput={(e) => onGoalChange((e.target as HTMLTextAreaElement).value)}
				onkeydown={onKey}
				rows={2}
				disabled={busy}
				placeholder="State a goal for the council…"
				class="border-border bg-card text-foreground placeholder:text-muted-foreground focus:border-primary w-full resize-none rounded-sm border px-3 py-2 font-mono text-[13px] leading-relaxed outline-none disabled:opacity-50"
			></textarea>
			{#if !busy}
				<div class="mt-2 flex flex-wrap gap-2">
					{#each SUGGESTIONS as s (s)}
						<button
							type="button"
							onclick={() => onGoalChange(s)}
							class="border-border text-muted-foreground hover:border-foreground/40 hover:text-foreground rounded-sm border px-2 py-1 font-mono text-[10.5px] transition-colors"
						>
							{s}
						</button>
					{/each}
				</div>
			{/if}
		</div>

		<div class="flex shrink-0 items-center gap-2 pb-1">
			{#if active}
				<button
					type="button"
					onclick={onCancel}
					disabled={cancelling}
					class="border-border text-muted-foreground hover:text-foreground flex size-9 items-center justify-center rounded-sm border transition-colors disabled:opacity-50"
					aria-label="Cancel session"
				>
					{#if cancelling}
						<span class="font-mono text-[10px]">…</span>
					{:else}
						<span class="font-mono text-[11px]">×</span>
					{/if}
				</button>
				<button
					type="button"
					onclick={onNewSession}
					class="border-border text-muted-foreground hover:text-foreground flex size-9 items-center justify-center rounded-sm border transition-colors"
					aria-label="New session"
				>
					<span class="font-mono text-[12px]">↺</span>
				</button>
			{/if}
			<button
				type="button"
				onclick={submit}
				disabled={busy || !goal.trim()}
				class={cn(
					'bg-primary text-primary-foreground flex h-9 items-center gap-2 rounded-sm px-4 font-mono text-[12px] tracking-wide transition-opacity',
					(busy || !goal.trim()) && 'opacity-40'
				)}
			>
				convene
				<span class="font-mono text-[12px]">→</span>
			</button>
		</div>
	</div>
</div>
