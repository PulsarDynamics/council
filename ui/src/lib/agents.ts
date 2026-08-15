// Starter agent roster. Kept in sync with the TOML configs in /agents/.
// Add a new agent here when you add a new TOML so the UI surfaces it.

export interface AgentSummary {
	name: string;
	role: string;
	subscribes: string[];
	publishes: string[];
}

export const agents: AgentSummary[] = [
	{
		name: 'Planner',
		role: 'Goal → structured plan',
		subscribes: ['goal'],
		publishes: ['plan', 'broadcast']
	},
	{
		name: 'Designer',
		role: 'Plan → detailed spec (data models, API, UI/UX)',
		subscribes: ['plan'],
		publishes: ['spec', 'broadcast']
	},
	{
		name: 'Implementer',
		role: 'Spec → working code',
		subscribes: ['spec'],
		publishes: ['result', 'broadcast']
	}
];
