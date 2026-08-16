// API client. HTTP for goal submission, WebSocket for live events.
//
// Falls back to a mock event stream when the WebSocket can't connect (so the
// UI is previewable without a running orchestrator). The orchestrator itself
// runs at the URL the Vite dev server proxies /api and /ws to.

import type { Event, EventEnvelope } from './types';
import { startMockStream } from './mock';

const ORCHESTRATOR_BASE: string = (import.meta.env.VITE_COUNCIL_API as string | undefined) ?? '';

export interface SubmitGoalResponse {
	session_id: string;
	created_at: string;
}

export async function submitGoal(goal: string): Promise<SubmitGoalResponse> {
	const res = await fetch(`${ORCHESTRATOR_BASE}/api/sessions`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ goal })
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(`submit goal failed: ${res.status} ${text}`);
	}
	return res.json();
}

export interface SwapProviderRequest {
	agent: string;
	provider: string;
	model?: string;
	reason?: string;
}

export interface SwapProviderResponse {
	dispatched: boolean;
	message: string;
}

export async function swapProvider(req: SwapProviderRequest): Promise<SwapProviderResponse> {
	const res = await fetch(`${ORCHESTRATOR_BASE}/api/control/swap`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify(req)
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(`swap failed: ${res.status} ${text}`);
	}
	return res.json();
}

// ---------------- session history ----------------

export interface SessionMeta {
	id: string;
	goal: string;
	created_at: string;
	completed_at?: string;
	status: string;
	event_count: number;
}

export async function listSessions(): Promise<SessionMeta[]> {
	const res = await fetch(`${ORCHESTRATOR_BASE}/api/sessions`);
	if (!res.ok) {
		const text = await res.text();
		throw new Error(`list sessions failed: ${res.status} ${text}`);
	}
	return res.json();
}

export async function getSessionEvents(id: string): Promise<EventEnvelope[]> {
	const res = await fetch(`${ORCHESTRATOR_BASE}/api/sessions/${encodeURIComponent(id)}/events`);
	if (!res.ok) {
		const text = await res.text();
		throw new Error(`get session events failed: ${res.status} ${text}`);
	}
	return res.json();
}

export async function resetAgent(agent: string): Promise<{ dispatched: boolean }> {
	const res = await fetch(`${ORCHESTRATOR_BASE}/api/control/reset`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ agent })
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(`reset failed: ${res.status} ${text}`);
	}
	return res.json();
}

export type StreamSource = 'orchestrator' | 'mock';

export interface StreamHandle {
	/** Whether the stream is backed by the real orchestrator or a mock. */
	source: StreamSource;
	/** Stop the stream and release resources. */
	close(): void;
}

/**
 * Subscribe to live events. Prefers the real WebSocket. If the WebSocket
 * fails to connect (or the orchestrator is down), falls back to a mock
 * stream after one retry so the UI is always demoable. Append `?mock=1`
 * to the URL to force mock mode (used for screenshots and offline preview).
 */
export function subscribeToEvents(
	onEvent: (env: EventEnvelope) => void,
	onSource?: (s: StreamSource) => void
): StreamHandle {
	const forceMock = new URLSearchParams(location.search).get('mock') === '1';
	let stopped = false;
	let ws: WebSocket | null = null;
	let mockStop: (() => void) | null = null;

	function startMock() {
		onSource?.('mock');
		mockStop = startMockStream(onEvent, () => stopped);
	}

	if (forceMock) {
		startMock();
		return {
			get source(): StreamSource {
				return 'mock';
			},
			close() {
				stopped = true;
				mockStop?.();
			}
		};
	}

	function startWs() {
		const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
		const url = `${proto}//${location.host}/ws`;
		try {
			ws = new WebSocket(url);
		} catch {
			startMock();
			return;
		}
		ws.binaryType = 'arraybuffer';
		ws.addEventListener('open', () => onSource?.('orchestrator'));
		ws.addEventListener('message', (e) => {
			const buf = e.data as ArrayBuffer;
			try {
				const env: EventEnvelope = JSON.parse(new TextDecoder().decode(buf));
				onEvent(env);
			} catch (err) {
				console.warn('failed to decode ws event', err);
			}
		});
		ws.addEventListener('close', () => {
			if (stopped) return;
			// Brief retry; if that also fails, drop to mock.
			setTimeout(() => {
				if (stopped) return;
				if (ws && ws.readyState === WebSocket.CLOSED) startMock();
			}, 1500);
		});
		ws.addEventListener('error', () => {
			// The 'close' handler will run too; let it decide.
		});
	}

	startWs();

	return {
		get source(): StreamSource {
			return ws && ws.readyState === WebSocket.OPEN ? 'orchestrator' : 'mock';
		},
		close() {
			stopped = true;
			ws?.close();
			mockStop?.();
		}
	};
}

/** Human-friendly label for an event kind. Used by the UI. */
export function eventKindLabel(event: Event): string {
	const k = event.kind;
	switch (k.type) {
		case 'user_message':
			return 'user';
		case 'agent_message':
			return k.agent;
		case 'agent_message_delta':
			return `${k.agent} (streaming)`;
		case 'agent_thinking':
			return `${k.agent} (thinking)`;
		case 'tool_call':
			return `${k.agent} → ${k.tool}`;
		case 'tool_result':
			return `${k.tool} result`;
		case 'file_change':
			return `${k.kind} ${k.path}`;
		case 'agent_status':
			return `${k.agent}: ${k.status}`;
		case 'llm_call':
			return `${k.agent} llm`;
		case 'system':
			return 'system';
		case 'session_created':
			return 'session';
		case 'session_completed':
			return 'session done';
		case 'error':
			return 'error';
	}
}
