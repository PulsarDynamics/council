<script lang="ts">
	import { BUILT_INS, fetchProviders, kindLabel, type ProviderConfig } from '$lib/providers';
	import { swapProvider } from '$lib/api';

	interface Props {
		open: boolean;
		agent: string | null;
		onClose: () => void;
	}
	let { open, agent, onClose }: Props = $props();

	let provider: string = $state('openai');
	let model: string = $state('');
	let reason: string = $state('');
	let submitting = $state(false);
	let errorMsg = $state('');

	let customs: ProviderConfig[] = $state([]);

	$effect(() => {
		if (open) {
			fetchProviders()
				.then((v) => {
					customs = Object.values(v.providers);
				})
				.catch(() => {
					customs = [];
				});
		}
	});

	const allProviders: Array<{ name: string; kind: string; label: string; defaultModel: string }> = $derived(
		[
			...BUILT_INS.map((b) => ({
				name: b.name,
				kind: b.kind,
				label: b.label,
				defaultModel: b.defaultModel
			})),
			...customs.map((c) => ({
				name: c.name,
				kind: c.kind,
				label: `${c.name} (custom)`,
				defaultModel: c.defaultModel
			}))
		]
	);

	// Pre-fill model when provider changes.
	$effect(() => {
		if (!open) return;
		const p = allProviders.find((x) => x.name === provider);
		if (p && !model) {
			model = p.defaultModel;
		}
	});

	async function submit(event: Event) {
		event.preventDefault();
		if (!agent) return;
		errorMsg = '';
		submitting = true;
		try {
			await swapProvider({
				agent,
				provider,
				model: model.trim() || undefined,
				reason: reason.trim() || undefined
			});
			onClose();
			reason = '';
		} catch (e) {
			errorMsg = e instanceof Error ? e.message : String(e);
		} finally {
			submitting = false;
		}
	}
</script>

{#if open && agent}
	<button
		type="button"
		aria-label="Close swap dialog"
		class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"
		onclick={onClose}
	></button>

	<div
		class="bg-base-100 border-base-300/60 fixed top-1/2 left-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border p-5 shadow-2xl"
		role="dialog"
		aria-modal="true"
	>
		<header class="mb-3">
			<h2 class="text-base font-semibold">Swap provider — {agent}</h2>
			<p class="text-base-content/50 mt-1 text-xs leading-relaxed">
				The agent will summarize the session so far with the current LLM,
				attach the files it touched, and resume with the new one. The
				deliberation continues without dropping the goal.
			</p>
		</header>

		<form onsubmit={submit} class="space-y-3">
			<label class="block">
				<span class="text-base-content/70 mb-1 block text-xs font-medium">Provider</span>
				<select
					class="border-base-300/60 bg-base-100/60 w-full rounded-md border px-2 py-1.5 text-sm"
					bind:value={provider}
				>
					{#each allProviders as p (p.name)}
						<option value={p.name}>{p.label}</option>
					{/each}
				</select>
			</label>

			<label class="block">
				<span class="text-base-content/70 mb-1 block text-xs font-medium">Model</span>
				<input
					class="border-base-300/60 bg-base-100/60 w-full rounded-md border px-2 py-1.5 font-mono text-sm"
					placeholder="leave blank for the provider default"
					bind:value={model}
				/>
			</label>

			<label class="block">
				<span class="text-base-content/70 mb-1 block text-xs font-medium">
					Reason <span class="text-base-content/40 font-normal">(optional)</span>
				</span>
				<input
					class="border-base-300/60 bg-base-100/60 w-full rounded-md border px-2 py-1.5 text-sm"
					placeholder="e.g. cost, latency, stuck in a loop"
					bind:value={reason}
				/>
			</label>

			{#if errorMsg}
				<p class="text-rose-400 text-xs">{errorMsg}</p>
			{/if}

			<div class="flex items-center justify-end gap-2 pt-2">
				<button
					type="button"
					class="hover:bg-base-300/60 rounded-md px-3 py-1.5 text-sm"
					onclick={onClose}>cancel</button
				>
				<button
					type="submit"
					disabled={submitting}
					class="bg-primary text-primary-content hover:bg-primary/90 rounded-md px-3 py-1.5 text-sm font-semibold disabled:opacity-50"
					>{submitting ? 'Swapping…' : 'Swap & continue'}</button
				>
			</div>
		</form>
	</div>
{/if}
