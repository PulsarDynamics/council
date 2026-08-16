// Provider registry that mirrors the Rust ProviderRegistry. Built-ins
// (OpenAI Chat, OpenAI Responses, Anthropic) are always present;
// user-added customs are layered on top via localStorage.

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

const STORAGE_KEY = 'council.providers.v1';

function loadCustoms(): ProviderConfig[] {
	if (!browser) return [];
	try {
		const raw = localStorage.getItem(STORAGE_KEY);
		if (!raw) return [];
		const parsed = JSON.parse(raw);
		if (!Array.isArray(parsed)) return [];
		return parsed.filter(
			(p): p is ProviderConfig =>
				typeof p?.name === 'string' &&
				typeof p?.kind === 'string' &&
				typeof p?.baseUrl === 'string' &&
				typeof p?.apiKey === 'string' &&
				typeof p?.defaultModel === 'string'
		);
	} catch {
		return [];
	}
}

function saveCustoms(customs: ProviderConfig[]): void {
	if (!browser) return;
	localStorage.setItem(STORAGE_KEY, JSON.stringify(customs));
}

/** Reactive store of custom providers. Call from components. */
export function getCustoms(): ProviderConfig[] {
	return loadCustoms();
}

export function addCustom(p: Omit<ProviderConfig, 'kind'> & { kind?: ProviderKind }): void {
	const customs = loadCustoms();
	customs.push({
		name: p.name,
		kind: p.kind ?? 'openai_chat',
		baseUrl: p.baseUrl,
		apiKey: p.apiKey,
		defaultModel: p.defaultModel
	});
	saveCustoms(customs);
}

export function removeCustom(name: string): void {
	const customs = loadCustoms().filter((c) => c.name !== name);
	saveCustoms(customs);
}

export function updateCustom(name: string, patch: Partial<ProviderConfig>): void {
	const customs = loadCustoms().map((c) => (c.name === name ? { ...c, ...patch } : c));
	saveCustoms(customs);
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
