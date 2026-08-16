// Deterministic mock event generator. Used when the WebSocket can't reach
// the orchestrator. Produces a realistic-looking session so the UI is
// previewable offline. Timing is jittered so it doesn't feel mechanical.

import type { Event, EventEnvelope } from './types';

const AGENTS = ['planner', 'designer', 'implementer'] as const;

function uuid(): string {
	// crypto.randomUUID is available in all modern browsers + Node 19+.
	return globalThis.crypto.randomUUID();
}

function nowIso(): string {
	return new Date().toISOString();
}

function env(channel: string, kind: Event['kind']): EventEnvelope {
	return {
		channel,
		event: {
			id: uuid(),
			session_id: '', // set by caller
			kind,
			timestamp: nowIso()
		}
	};
}

function sleep(ms: number, isStopped: () => boolean): Promise<void> {
	return new Promise((res) => {
		const t = setTimeout(res, ms);
		// Best-effort cancellation: we just race against `isStopped` in the loop.
		if (isStopped()) {
			clearTimeout(t);
			res();
		}
	});
}

const SCRIPTED_PLANS = [
	[
		'Tighten the input validation on POST /api/sessions',
		'Move agent dispatch to a background task queue',
		'Add a /healthz endpoint that checks Redis ping'
	],
	['Sketch a CLI for replaying past sessions', 'Persist session state to Redis on every transition']
];

const SCRIPTED_SPECS = [
	[
		'Define the AgentSpec schema migration',
		'Add a `delegate_to` tool that calls the orchestrator with the target agent name',
		'Document the 12-event wire contract in docs/WIRE_CONTRACT.md'
	],
	[
		'Build a SvelteKit stream view that auto-scrolls and renders every event kind',
		'Add a status pill per agent with idle / working / error states'
	]
];

const SCRIPTED_RESULTS = [
	['Implemented `delegate_to` with cycle detection', 'Wired the status pill into the agent card'],
	['Built the SvelteKit stream view, including reconnection logic']
];

export function startMockStream(
	onEvent: (env: EventEnvelope) => void,
	isStopped: () => boolean
): () => void {
	let sessionId = '';
	let cancelled = false;
	const cancel = () => {
		cancelled = true;
	};

	async function run() {
		const jitter = (base: number) => base + Math.random() * 250;
		const send = (channel: string, kind: Event['kind']) => {
			if (cancelled) return;
			const e = env(channel, kind);
			e.event.session_id = sessionId;
			onEvent(e);
		};
		const wait = (ms: number) => sleep(jitter(ms), () => cancelled);

		// SessionCreated
		sessionId = uuid();
		const goal = 'Add a Stripe webhook handler that records failed payments';
		send('broadcast', { type: 'session_created', goal });

		await wait(400);
		send('goal', { type: 'user_message', content: goal });

		// Planner
		await wait(500);
		send('broadcast', { type: 'agent_status', agent: 'planner', status: 'working' });
		await wait(700);
		const plans = SCRIPTED_PLANS[Math.floor(Math.random() * SCRIPTED_PLANS.length)];
		send('plan', {
			type: 'agent_message',
			agent: 'planner',
			content: `Plan:\n${plans.map((p, i) => `  ${i + 1}. ${p}`).join('\n')}`
		});
		send('plan', {
			type: 'llm_call',
			agent: 'planner',
			model: 'gpt-4o',
			prompt_tokens: 412,
			completion_tokens: 86,
			duration_ms: 1320
		});
		await wait(300);
		send('broadcast', { type: 'agent_status', agent: 'planner', status: 'idle' });

		// Designer
		await wait(500);
		send('broadcast', { type: 'agent_status', agent: 'designer', status: 'working' });
		await wait(900);
		const specs = SCRIPTED_SPECS[Math.floor(Math.random() * SCRIPTED_SPECS.length)];
		send('spec', {
			type: 'agent_message',
			agent: 'designer',
			content: `Spec:\n${specs.map((s, i) => `  - ${s}`).join('\n')}`
		});
		send('spec', {
			type: 'tool_call',
			agent: 'designer',
			tool: 'read_file',
			args: { path: 'crates/council-core/src/event.rs' }
		});
		await wait(300);
		send('spec', {
			type: 'tool_result',
			agent: 'designer',
			tool: 'read_file',
			result: { lines: 142 }
		});
		await wait(200);
		send('broadcast', { type: 'agent_status', agent: 'designer', status: 'idle' });

		// Implementer
		await wait(500);
		send('broadcast', { type: 'agent_status', agent: 'implementer', status: 'working' });
		await wait(700);
		send('result', {
			type: 'file_change',
			path: 'crates/council-core/src/event.rs',
			kind: 'modified',
			diff: '@@ -42,6 +42,10 @@\n+    pub channel: String,\n'
		});
		send('result', {
			type: 'agent_message',
			agent: 'implementer',
			content: 'Done — channel field added to EventEnvelope, tests still green.'
		});
		send('result', {
			type: 'llm_call',
			agent: 'implementer',
			model: 'gpt-4o',
			prompt_tokens: 188,
			completion_tokens: 24,
			duration_ms: 640
		});
		await wait(300);
		send('broadcast', { type: 'agent_status', agent: 'implementer', status: 'idle' });

		await wait(400);
		send('broadcast', {
			type: 'session_completed',
			summary: 'Council delivered: 3 plans, 2 specs, 1 file change.'
		});
	}

	void run();

	return cancel;
}
