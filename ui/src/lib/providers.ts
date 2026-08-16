// Provider registry that mirrors the Rust ProviderRegistry. Built-ins
// (OpenAI Chat, OpenAI Responses, Anthropic) are always available;
// user-added customs are read from the orchestrator's providers.toml
// (via GET /api/providers) and upserted via POST /api/providers.

import { browser } from '$app/environment';

export type ProviderKind = 'openai_chat' | 'openai_responses' | 'anthropic_messages' | 'custom';

export interface ProviderConfig {
	name: string;
	kind: ProviderKind;
	baseUrl: string;
	apiKey: string;
	defaultModel: string;
}

export interface BuiltIn {
	name: string;
	kind: ProviderKind;
	defaultBaseUrl: string;
	defaultModel: string;
	label: string;
}

export const BUILT_INS: BuiltIn[] = [
	{
		name: 'openai',
		kind: 'openai_chat',
		defaultBaseUrl: 'https://api.openai.com/v1',
		defaultModel: 'gpt-4o',
		label: 'OpenAI (Chat Completions)'
	},
	{
		name: 'openai-responses',
		kind: 'openai_responses',
		defaultBaseUrl: 'https://api.openai.com/v1',
		defaultModel: 'gpt-4o',
		label: 'OpenAI (Responses API)'
	},
	{
		name: 'anthropic',
		kind: 'anthropic_messages',
		defaultBaseUrl: 'https://api.anthropic.com/v1',
		defaultModel: 'claude-sonnet-4-5',
		label: 'Anthropic (Messages API)'
	}
];

const ORCHESTRATOR_BASE: string =
	(import.meta.env.VITE_COUNCIL_API as string | undefined) ?? '';

export interface ProvidersView {
	path: string;
	providers: Record<string, ProviderConfig>;
}

/** Fetch the providers file from the orchestrator. */
export async function fetchProviders(): Promise<ProvidersView> {
	const res = await fetch(`${ORCHESTRATOR_BASE}/api/providers`);
	if (!res.ok) {
		const text = await res.text();
		throw new Error(`fetch providers failed: ${res.status} ${text}`);
	}
	// The API returns snake_case-ish keys (`base_url`, `api_key`,
	// `default_model`); normalise to camelCase for the UI.
	const raw = (await res.json()) as {
		path: string;
		providers: Record<
			string,
			{ kind: ProviderKind; base_url: string; api_key: string; default_model: string }
		>;
	};
	const out: Record<string, ProviderConfig> = {};
	for (const [name, e] of Object.entries(raw.providers ?? {})) {
		out[name] = {
			name,
			kind: e.kind,
			baseUrl: e.base_url,
			apiKey: e.api_key,
			defaultModel: e.default_model
		};
	}
	return { path: raw.path, providers: out };
}

/** Upsert a provider. Writes to providers.toml via the orchestrator. */
export async function upsertProvider(p: ProviderConfig): Promise<{ path: string }> {
	const res = await fetch(`${ORCHESTRATOR_BASE}/api/providers`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({
			name: p.name,
			kind: p.kind,
			base_url: p.baseUrl,
			api_key: p.apiKey,
			default_model: p.defaultModel
		})
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(`upsert failed: ${res.status} ${text}`);
	}
	return res.json();
}

/** Remove a provider. */
export async function deleteProvider(name: string): Promise<void> {
	const res = await fetch(`${ORCHESTRATOR_BASE}/api/providers/${encodeURIComponent(name)}`, {
		method: 'DELETE'
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(`delete failed: ${res.status} ${text}`);
	}
}

export function kindLabel(kind: ProviderKind): string {
	switch (kind) {
		case 'openai_chat':
			return 'OpenAI Chat Completions';
		case 'openai_responses':
			return 'OpenAI Responses';
		case 'anthropic_messages':
			return 'Anthropic Messages';
		case 'custom':
			return 'Custom (OpenAI-compatible)';
	}
}

export function envVarsFor(p: ProviderConfig): string[] {
	const upper = p.name.toUpperCase();
	return [
		`COUNCIL_PROVIDER_${upper}_KIND=${p.kind}`,
		`COUNCIL_PROVIDER_${upper}_BASE_URL=${p.baseUrl}`,
		`COUNCIL_PROVIDER_${upper}_API_KEY=${p.apiKey || '<set your key>'}`,
		`COUNCIL_PROVIDER_${upper}_MODEL=${p.defaultModel}`
	];
}
