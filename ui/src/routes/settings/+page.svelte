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

	/*
	 * /settings — full-page LLM provider management.
	 *
	 *   ┌────────────────────────────────────────────────────────────────┐
	 *   │  built-in providers (always available)                        │
	 *   │  custom providers (CRUD; persisted to providers.toml)         │
	 *   │  add new provider form                                        │
	 *   └────────────────────────────────────────────────────────────────┘
	 *
	 * Full-page LLM provider management. Replaces the old
	 * SettingsPanel modal — generous vertical real estate, no
	 * overlay/scrim, file-path chip shown in the page header.
	 */

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
		refresh();
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

<svelte:head>
	<title>Settings · Council</title>
</svelte:head>

<main class="mx-auto h-[calc(100vh-3.5rem)] max-w-3xl overflow-y-auto px-6 py-8">
	<header class="mb-6 flex items-start justify-between gap-4">
		<div>
			<h2 class="font-mono text-[12px] font-semibold tracking-[0.18em] uppercase">
				LLM Providers
			</h2>
			<p class="text-muted-foreground mt-1 font-mono text-[11px] leading-relaxed">
				Built-in providers are always available. Custom providers are persisted to
				<code class="bg-muted rounded px-1 font-mono text-[10px]">providers.toml</code>
				and read by every Council process. To swap an agent to a custom provider mid-flight, open
				the agent card's "swap provider" menu on the console.
			</p>
		</div>
		{#if view}
			<code
				class="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 font-mono text-[10px]"
			>
				{view.path}
			</code>
		{/if}
	</header>

	{#if loadError}
		<p class="text-destructive mb-4 font-mono text-[11px]">{loadError}</p>
	{/if}

	<section class="mb-8">
		<h3
			class="text-muted-foreground mb-3 font-mono text-[11px] font-semibold tracking-[0.18em] uppercase"
		>
			Built-in ({BUILT_INS.length})
		</h3>
		<ul class="space-y-2">
			{#each BUILT_INS as b (b.name)}
				<li
					class="border-border bg-card/40 flex items-start justify-between gap-3 rounded-md border p-3"
				>
					<div class="min-w-0 flex-1">
						<div class="font-mono text-[12px] font-medium">{b.name}</div>
						<div class="text-muted-foreground mt-0.5 font-mono text-[10px]">
							{b.label}
						</div>
						<div class="text-muted-foreground/70 mt-0.5 font-mono text-[10px]">
							{b.defaultBaseUrl} · {b.defaultModel}
						</div>
					</div>
					<span
						class="bg-emerald-500/10 text-emerald-400 rounded px-2 py-0.5 font-mono text-[10px] font-semibold tracking-wide uppercase"
					>
						ready
					</span>
				</li>
			{/each}
		</ul>
	</section>

	<section class="mb-8">
		<h3
			class="text-muted-foreground mb-3 font-mono text-[11px] font-semibold tracking-[0.18em] uppercase"
		>
			Custom ({customsList().length})
		</h3>
		{#if !view}
			<p class="text-muted-foreground font-mono text-[11px]">Loading…</p>
		{:else if customsList().length === 0}
			<p class="text-muted-foreground font-mono text-[11px]">None yet. Add one below.</p>
		{:else}
			<ul class="space-y-2">
				{#each customsList() as c (c.name)}
					<li class="border-border bg-card/40 space-y-2 rounded-md border p-3">
						<div class="flex items-start justify-between gap-2">
							<div class="min-w-0 flex-1">
								<div class="font-mono text-[12px] font-medium">{c.name}</div>
								<div class="text-muted-foreground mt-0.5 font-mono text-[10px]">
									{kindLabel(c.kind)}
								</div>
							</div>
							<button
								type="button"
								disabled={saving}
								class="text-destructive hover:text-destructive/80 font-mono text-[11px] underline disabled:opacity-50"
								onclick={() => del(c.name)}>remove</button
							>
						</div>
						<div class="text-muted-foreground/70 font-mono text-[10px]">
							{c.baseUrl} · {c.defaultModel}
						</div>
						<details class="text-muted-foreground font-mono text-[10px]">
							<summary class="cursor-pointer">env-var equivalent</summary>
							<pre
								class="bg-muted/60 mt-1 overflow-x-auto rounded p-2 font-mono text-[10px]">{envVarsFor(c).join('\n')}</pre>
						</details>
					</li>
				{/each}
			</ul>
		{/if}
	</section>

	<section class="mb-8">
		<h3
			class="text-muted-foreground mb-3 font-mono text-[11px] font-semibold tracking-[0.18em] uppercase"
		>
			Add custom provider
		</h3>
		<form onsubmit={addNew} class="space-y-2">
			<input
				class="border-border bg-input/40 focus:border-primary/60 w-full rounded-md border px-2 py-1.5 font-mono text-[12px] focus:outline-none"
				placeholder="provider name (e.g. 'groq', 'local-llama')"
				bind:value={formName}
			/>
			<select
				class="border-border bg-input/40 w-full rounded-md border px-2 py-1.5 text-[12px]"
				bind:value={formKind}
			>
				<option value="openai_chat">OpenAI Chat Completions</option>
				<option value="openai_responses">OpenAI Responses</option>
				<option value="anthropic_messages">Anthropic Messages</option>
				<option value="custom">Custom (OpenAI-compatible)</option>
			</select>
			<input
				class="border-border bg-input/40 w-full rounded-md border px-2 py-1.5 font-mono text-[12px]"
				placeholder="base url"
				bind:value={formBaseUrl}
			/>
			<input
				class="border-border bg-input/40 w-full rounded-md border px-2 py-1.5 font-mono text-[12px]"
				type="password"
				placeholder="api key (kept in providers.toml)"
				bind:value={formApiKey}
			/>
			<input
				class="border-border bg-input/40 w-full rounded-md border px-2 py-1.5 font-mono text-[12px]"
				placeholder="default model"
				bind:value={formModel}
			/>
			{#if formError}
				<p class="text-destructive font-mono text-[11px]">{formError}</p>
			{/if}
			<button
				type="submit"
				disabled={saving}
				class="bg-primary text-primary-foreground hover:bg-primary/90 w-full rounded-md px-3 py-1.5 font-mono text-[12px] font-semibold disabled:opacity-50"
			>
				Add provider
			</button>
		</form>
	</section>
</main>
