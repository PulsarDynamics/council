<script lang="ts">
	import {
		BUILT_INS,
		deleteProvider,
		envVarsFor,
		fetchProviders,
		kindLabel,
		upsertProvider,
		type ProviderConfig,
		type ProviderKind,
		type ProvidersView
	} from '$lib/providers';

	interface Props {
		open: boolean;
		onClose: () => void;
	}
	let { open, onClose }: Props = $props();

	let view: ProvidersView | null = $state(null);
	let loadError = $state('');
	let saving = $state(false);

	// New-provider form
	let formName = $state('');
	let formKind: ProviderKind = $state('openai_chat');
	let formBaseUrl = $state('https://api.openai.com/v1');
	let formApiKey = $state('');
	let formModel = $state('gpt-4o');
	let formError = $state('');

	$effect(() => {
		if (open) {
			refresh();
		}
	});

	async function refresh() {
		loadError = '';
		try {
			view = await fetchProviders();
		} catch (e) {
			loadError = e instanceof Error ? e.message : String(e);
			view = null;
		}
	}

	function customsList(): ProviderConfig[] {
		if (!view) return [];
		return Object.values(view.providers).sort((a, b) => a.name.localeCompare(b.name));
	}

	async function addNew(event: Event) {
		event.preventDefault();
		formError = '';
		const name = formName.trim();
		if (!name) {
			formError = 'Name is required';
			return;
		}
		if (!/^[a-z0-9_-]+$/i.test(name)) {
			formError = 'Name must be letters, digits, _, or -';
			return;
		}
		const p: ProviderConfig = {
			name,
			kind: formKind,
			baseUrl: formBaseUrl.trim() || 'https://api.openai.com/v1',
			apiKey: formApiKey.trim(),
			defaultModel: formModel.trim() || 'gpt-4o'
		};
		saving = true;
		try {
			await upsertProvider(p);
			formName = '';
			formApiKey = '';
			await refresh();
		} catch (e) {
			formError = e instanceof Error ? e.message : String(e);
		} finally {
			saving = false;
		}
	}

	async function del(name: string) {
		saving = true;
		try {
			await deleteProvider(name);
			await refresh();
		} catch (e) {
			loadError = e instanceof Error ? e.message : String(e);
		} finally {
			saving = false;
		}
	}
</script>

{#if open}
	<button
		type="button"
		aria-label="Close settings"
		class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"
		onclick={onClose}
	></button>

	<aside
		class="bg-base-100 border-base-300/60 fixed top-0 right-0 z-50 flex h-full w-full max-w-xl flex-col border-l shadow-2xl"
	>
		<header class="border-base-300/60 flex items-center justify-between border-b px-5 py-3">
			<div>
				<h2 class="text-base font-semibold">Settings — LLM Providers</h2>
				<p class="text-base-content/50 text-xs">
					{#if view}
						persisted to <code class="bg-base-300/50 rounded px-1 font-mono text-[10px]">{view.path}</code>
					{:else}
						loading…
					{/if}
				</p>
			</div>
			<button
				type="button"
				class="hover:bg-base-300/60 rounded-md px-2 py-1 text-xs"
				onclick={onClose}>close</button
			>
		</header>

		<div class="flex-1 space-y-6 overflow-y-auto px-5 py-4">
			{#if loadError}
				<p class="text-rose-400 text-xs">{loadError}</p>
			{/if}

			<section>
				<h3 class="text-base-content/70 mb-2 text-xs font-semibold tracking-wide uppercase">
					Built-in
				</h3>
				<ul class="space-y-2">
					{#each BUILT_INS as b (b.name)}
						<li
							class="border-base-300/40 bg-base-200/30 flex items-start justify-between gap-3 rounded-md border p-3"
						>
							<div class="min-w-0 flex-1">
								<div class="text-sm font-medium">
									{b.name}
									<span class="text-base-content/50 text-xs font-normal">— {b.label}</span>
								</div>
								<div class="text-base-content/50 mt-0.5 font-mono text-[11px]">
									{b.defaultBaseUrl} · {b.defaultModel}
								</div>
							</div>
							<span
								class="bg-emerald-500/10 text-emerald-300 rounded px-2 py-0.5 text-[10px] font-semibold tracking-wide uppercase"
								>ready</span
							>
						</li>
					{/each}
				</ul>
			</section>

			<section>
				<h3 class="text-base-content/70 mb-2 text-xs font-semibold tracking-wide uppercase">
					Custom ({customsList().length})
				</h3>
				{#if !view}
					<p class="text-base-content/40 text-xs">Loading…</p>
				{:else if customsList().length === 0}
					<p class="text-base-content/40 text-xs">None yet. Add one below.</p>
				{:else}
					<ul class="space-y-2">
						{#each customsList() as c (c.name)}
							<li class="border-base-300/40 bg-base-200/30 space-y-2 rounded-md border p-3">
								<div class="flex items-start justify-between gap-2">
									<div class="min-w-0 flex-1">
										<div class="text-sm font-medium">{c.name}</div>
										<div class="text-base-content/50 text-[11px]">{kindLabel(c.kind)}</div>
									</div>
									<button
										type="button"
										disabled={saving}
										class="text-rose-400 hover:text-rose-300 text-xs underline disabled:opacity-50"
										onclick={() => del(c.name)}>remove</button
									>
								</div>
								<div class="text-base-content/40 font-mono text-[10px]">
									{c.baseUrl} · {c.defaultModel}
								</div>
								<details class="text-base-content/50 text-[10px]">
									<summary class="cursor-pointer">env-var equivalent</summary>
									<pre
										class="bg-base-300/30 mt-1 overflow-x-auto rounded p-2 text-[10px]"
									>{envVarsFor(c).join('\n')}</pre>
								</details>
							</li>
						{/each}
					</ul>
				{/if}
			</section>

			<section>
				<h3 class="text-base-content/70 mb-2 text-xs font-semibold tracking-wide uppercase">
					Add custom provider
				</h3>
				<form onsubmit={addNew} class="space-y-2">
					<input
						class="border-base-300/60 bg-base-100/60 focus:border-primary/60 w-full rounded border px-2 py-1.5 font-mono text-sm focus:outline-none"
						placeholder="provider name (e.g. 'groq', 'local-llama')"
						bind:value={formName}
					/>
					<select
						class="border-base-300/60 bg-base-100/60 w-full rounded border px-2 py-1.5 text-sm"
						bind:value={formKind}
					>
						<option value="openai_chat">OpenAI Chat Completions</option>
						<option value="openai_responses">OpenAI Responses</option>
						<option value="anthropic_messages">Anthropic Messages</option>
						<option value="custom">Custom (OpenAI-compatible)</option>
					</select>
					<input
						class="border-base-300/60 bg-base-100/60 w-full rounded border px-2 py-1.5 font-mono text-sm"
						placeholder="base url"
						bind:value={formBaseUrl}
					/>
					<input
						class="border-base-300/60 bg-base-100/60 w-full rounded border px-2 py-1.5 font-mono text-sm"
						type="password"
						placeholder="api key (kept in providers.toml)"
						bind:value={formApiKey}
					/>
					<input
						class="border-base-300/60 bg-base-100/60 w-full rounded border px-2 py-1.5 font-mono text-sm"
						placeholder="default model"
						bind:value={formModel}
					/>
					{#if formError}
						<p class="text-rose-400 text-xs">{formError}</p>
					{/if}
					<button
						type="submit"
						disabled={saving}
						class="bg-primary text-primary-content hover:bg-primary/90 w-full rounded-md px-3 py-1.5 text-sm font-semibold disabled:opacity-50"
						>Add provider</button
					>
				</form>
				<p class="text-base-content/40 mt-2 text-[11px] leading-relaxed">
					Provider configs are persisted to <code class="bg-base-300/50 rounded px-1 font-mono"
						>providers.toml</code
					> and read by every Council process. To swap an agent to a
					custom mid-flight, open the agent card's "swap provider"
					menu.
				</p>
			</section>
		</div>
	</aside>
{/if}
