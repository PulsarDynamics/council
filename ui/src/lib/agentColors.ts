// Per-agent accent resolver. Returns the CSS color (or `var(--agent-*)`
// token) for a given agent name, matching the `--color-agent-*` and
// `--agent-*` definitions in `routes/layout.css`.
//
// Adding a new agent?  Add the agent to `lib/agents.ts` with an `accent`
// field (e.g. `"var(--agent-researcher)"`) AND add the corresponding
// `--color-agent-researcher` token in layout.css.

import { agents, type AgentSummary } from './agents';

export interface AgentWithAccent extends AgentSummary {
	accent: string;
	id: string; // lowercased name; matches our `agent_status` event `agent` field
}

const accentByName: Record<string, string> = {
	planner: 'var(--agent-planner)',
	designer: 'var(--agent-designer)',
	implementer: 'var(--agent-implementer)',
	// Fallback for unrecognized agents / the "council" pseudo-agent.
	// Council Chamber-1 uses a muted color here.
};

export function accentOf(agent: string | null | undefined): string {
	if (!agent) return 'var(--agent-council)';
	const key = agent.toLowerCase();
	return accentByName[key] ?? 'var(--agent-council)';
}

/// The same agent list as `agents` but with `id` and `accent` fields
/// populated for the UI. Use this anywhere that needs both name +
/// color (AgentRail, FlowGraph per-agent pills, Inspector, etc.).
export const agentsWithAccent: AgentWithAccent[] = agents.map((a) => ({
	...a,
	id: a.name.toLowerCase(),
	accent: accentOf(a.name),
}));
