<script lang="ts">
	import {
		BUILT_INS,
		addCustom,
		envVarsFor,
		getCustoms,
		kindLabel,
		removeCustom,
		updateCustom,
		type ProviderConfig,
		type ProviderKind
	} from '$lib/providers';

	interface Props {
		open: boolean;
		onClose: () => void;
	}
	let { open, onClose }: Props = $props();

	let customs: ProviderConfig[] = $state(getCustoms());

	// New-provider form
	let formName = $state('');
	let formKind: ProviderKind = $state('openai_chat');
	let formBaseUrl = $state('https://api.openai.com/v1');
	let formApiKey = $state('');
	let formModel = $state('gpt-4o');
	let formError = $state('');

	function addNew(event: Event) {
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
		if (BUILT_INS.some((b) => b.name === name) || customs.some((c) => c.name === name)) {
			formError = `"${name}" already exists`;
			return;
		}
		const p: ProviderConfig = {
			name,
			kind: formKind,
			baseUrl: formBaseUrl.trim() || 'https://api.openai.com/v1',
			apiKey: formApiKey.trim(),
			defaultModel: formModel.trim() || 'gpt-4o'
		};
		addCustom(p);
		customs = getCustoms();
		formName = '';
		formApiKey = '';
		formError = '';
	}

	function del(name: string) {
		removeCustom(name);
		customs = getCustoms();
	}

	function updateField(name: string, key: keyof ProviderConfig, value: string) {
		updateCustom(name, { [key]: value } as Partial<ProviderConfig>);
		customs = getCustoms();
	}
</script>

{#if open}
	<!-- backdrop -->
	<button
		type="button"
		aria-label="Close settings"
		class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"
		onclick={onClose}
	></button>

	<!-- panel -->
	<aside
		class="bg-base-100 border-base-300/60 fixed top-0 right-0 z-50 flex h-full w-full max-w-xl flex-col border-l shadow-2xl"
	>
		<header class="border-base-300/60 flex items-center justify-between border-b px-5 py-3">
			<div>
				<h2 class="text-base font-semibold">Settings — LLM Providers</h2>
				<p class="text-base-content/50 text-xs">
					Built-ins always available. Customs persist in localStorage; agents
					read them on next start.
				</p>
			</div>
			<button
				type="button"
				class="hover:bg-base-300/60 rounded-md px-2 py-1 text-xs"
				onclick={onClose}>close</button
			>
		</header>

		<div class="flex-1 space-y-6 overflow-y-auto px-5 py-4">
			<!-- Built-ins -->
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
							<span class="bg-emerald-500/10 text-emerald-300 rounded px-2 py-0.5 text-[10px] font-semibold tracking-wide uppercase"
								>ready</span
							>
						</li>
					{/each}
				</ul>
			</section>

			<!-- Customs -->
			<section>
				<h3 class="text-base-content/70 mb-2 text-xs font-semibold tracking-wide uppercase">
					Custom ({customs.length})
				</h3>
				{#if customs.length === 0}
					<p class="text-base-content/40 text-xs">None yet. Add one below.</p>
				{:else}
					<ul class="space-y-2">
						{#each customs as c (c.name)}
							<li
								class="border-base-300/40 bg-base-200/30 space-y-2 rounded-md border p-3"
							>
								<div class="flex items-start justify-between gap-2">
									<div class="min-w-0 flex-1">
										<div class="text-sm font-medium">{c.name}</div>
										<div class="text-base-content/50 text-[11px]">{kindLabel(c.kind)}</div>
									</div>
									<button
										type="button"
										class="text-rose-400 hover:text-rose-300 text-xs underline"
										onclick={() => del(c.name)}>remove</button
									>
								</div>
								<div class="grid grid-cols-1 gap-1.5">
									<input
										class="border-base-300/60 bg-base-100/60 rounded border px-2 py-1 font-mono text-xs"
										placeholder="https://api.example.com/v1"
										value={c.baseUrl}
										oninput={(e) => updateField(c.name, 'baseUrl', e.currentTarget.value)}
									/>
									<input
										class="border-base-300/60 bg-base-100/60 rounded border px-2 py-1 font-mono text-xs"
									type="password"
										placeholder="api key"
										value={c.apiKey}
										oninput={(e) => updateField(c.name, 'apiKey', e.currentTarget.value)}
									/>
									<input
										class="border-base-300/60 bg-base-100/60 rounded border px-2 py-1 font-mono text-xs"
										placeholder="default model"
										value={c.defaultModel}
										oninput={(e) => updateField(c.name, 'defaultModel', e.currentTarget.value)}
									/>
								</div>
								<details class="text-base-content/50 text-[10px]">
									<summary class="cursor-pointer">env-var equivalent</summary>
									<pre class="bg-base-300/30 mt-1 overflow-x-auto rounded p-2 text-[10px]">{envVarsFor(c).join('\n')}</pre>
								</details>
							</li>
						{/each}
					</ul>
				{/if}
			</section>

			<!-- Add form -->
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
						placeholder="api key (kept in your browser only)"
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
						class="bg-primary text-primary-content hover:bg-primary/90 w-full rounded-md px-3 py-1.5 text-sm font-semibold"
						>Add provider</button
					>
				</form>
				<p class="text-base-content/40 mt-2 text-[11px] leading-relaxed">
					The agent loads providers from env on each start. After saving a
					custom provider here, restart the agent (or re-run
					<code class="bg-base-300/50 rounded px-1">scripts/dev.sh</code>) and
					set the matching env vars — or the agent's TOML <code class="bg-base-300/50 rounded px-1"
						>model.provider</code
					> can name this provider directly.
				</p>
			</section>
		</div>
	</aside>
{/if}
