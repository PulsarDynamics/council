// Wire-contract types. Mirror of crates/council-core/src/event.rs.
// Keep this file in sync with the Rust enum — they MUST agree on discriminator
// strings (snake_case) and required fields. See AGENTS.md §6.

export type EventId = string; // UUID v4
export type SessionId = string; // UUID v4

export type FileChangeKind = 'created' | 'modified' | 'deleted';
export type AgentLifecycle = 'started' | 'idle' | 'working' | 'stopped' | 'error';
export type SessionStatus = 'pending' | 'running' | 'completed' | 'failed';

export type EventKind =
	| { type: 'user_message'; content: string }
	| { type: 'agent_message'; agent: string; content: string }
	| { type: 'agent_thinking'; agent: string; content: string }
	| { type: 'tool_call'; agent: string; tool: string; args: unknown }
	| {
			type: 'tool_result';
			agent: string;
			tool: string;
			result: unknown;
			error?: string;
	  }
	| { type: 'file_change'; path: string; kind: FileChangeKind; diff?: string }
	| { type: 'agent_status'; agent: string; status: AgentLifecycle }
	| {
			type: 'llm_call';
			agent: string;
			model: string;
			prompt_tokens: number;
			completion_tokens: number;
			duration_ms: number;
	  }
	| { type: 'system'; message: string }
	| { type: 'session_created'; goal: string }
	| { type: 'session_completed'; summary: string }
	| { type: 'error'; source: string; message: string };

export interface Event {
	id: EventId;
	session_id: SessionId;
	kind: EventKind;
	timestamp: string; // ISO 8601
}

export interface Session {
	id: SessionId;
	goal: string;
	status: SessionStatus;
	created_at: string;
	completed_at?: string;
}
