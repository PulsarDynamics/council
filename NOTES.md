# Council — Project Notes

Full context dump for cold-starting a fresh session on this project.

## What it is
A self-contained, multi-agent orchestration system in **Rust + Svelte** with **Redis** as the message bus. Agents that plan, design, and implement based on plans/docs, talk to each other, and you can watch everything live in the UI.

Project name: **Council** (previously informally "Agent Orchestra"). Directory: `~/Desktop/gitlab-repos/council/`.

## Stack & key decisions
- **Backend**: Rust (Axum + Tokio), single binary with `serve` and `agent` subcommands
- **Frontend**: SvelteKit (Svelte 5 + TS strict + Tailwind 4)
- **Message bus**: Redis pub/sub — channels act as workflow edges
- **LLM**: OpenAI-compatible API (works with OpenAI, Azure, OpenRouter, Ollama, etc.) — driven by env vars
- **Agents** are pure **TOML configs** → drop a new file in `agents/` to add one
- **`delegate_to` tool** lets agents call other agents → arbitrary workflow topologies
- **Live observability**: every event (user msg, agent msg, tool call, file change, LLM call, agent status) streams over WebSocket to the UI

## Three starter agents (in `agents/`)

| Agent       | Subscribes to | Publishes to          | Role                                  |
|-------------|---------------|-----------------------|---------------------------------------|
| Planner     | `goal`        | `plan`, `broadcast`   | Goal → structured plan                |
| Designer    | `plan`        | `spec`, `broadcast`   | Plan → detailed spec (data models, API, UI/UX) |
| Implementer | `spec`        | `result`, `broadcast` | Spec → working code                   |

**7 tools**: `read_file`, `write_file`, `edit_file`, `list_dir`, `run_command`, `delegate_to`, `ask_user` (Designer also gets `search_code`).

## Plan structure — 5 tasks, 3 cycles

| # | Module                                          | Cycle | Status                |
|---|-------------------------------------------------|-------|-----------------------|
| 1 | Scaffold + shared core types + Redis bus        | 1     | In progress           |
| 2 | SvelteKit UI (8 components, WS, mock mode)      | 1     | ✅ **Done**           |
| 3 | Orchestrator server (Axum + WS + agent process manager) | 2 | Blocked on #1         |
| 4 | Agent binary (LLM + 7 tools + 3 agents)         | 2     | Blocked on #1         |
| 5 | E2E test + dev script + 4 docs + 3 bonus example agents | 3 | Blocked on #2–#4    |

First plan launch failed (case-sensitive agent names: `coder` vs `Coder`); relaunched as `plan_45b66e65` and is running clean.

## Event wire contract (12 types)

`user_message` · `agent_message` · `agent_thinking` · `tool_call` · `tool_result` · `file_change` · `agent_status` · `llm_call` · `system` · `session_created` · `session_completed` · `error`

## Bonus agents in `examples/agents/`

`researcher`, `code-reviewer`, `devops` — drop-in configs for non-software-dev workflows.

## Side questions

- **Tauri for desktop wrap** — recommended, native fit since you're already in Rust (~15MB vs Electron's 150MB, direct Rust↔Rust into the orchestrator). Offered to add as a follow-up task.
- **MiniMax Code integration** — I don't have those docs, pointed you to the IDE's AI/provider settings.

## Next steps after build lands

```bash
cd ~/Desktop/gitlab-repos/council
echo "OPENAI_API_KEY=sk-..." > .env
docker compose up -d
cargo run -- serve &
cd ui && pnpm dev
```

Open `http://localhost:5173`, type a goal, watch Planner → Designer → Implementer work it live.

---

## When to refer back

- Adding new agents → check the `agents/` TOML schema + wire contract
- Wiring up new event types → check the 12-type contract above
- Spawning a fresh session → paste this dump back as context
- Talking about the project → call it **Council**
