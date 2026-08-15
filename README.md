# Council

A self-contained, multi-agent orchestration system. Agents that plan, design, and implement, talk to each other, and stream everything live to a web UI.

**Repo**: <https://github.com/PulsarDynamics/council>

> **Why "Council"?** Agents deliberate, delegate, and contribute. Any agent can call any other via the `delegate_to` tool, so the topology is open — more roundtable than orchestra. Each starter agent is a voice around the table: a planner, a designer, an implementer.

## Stack

- **Backend** — Rust (Axum + Tokio), single binary with `serve` and `agent` subcommands
- **Frontend** — SvelteKit (Svelte 5 + TS strict + Tailwind 4)
- **Message bus** — Redis pub/sub; channels act as workflow edges
- **LLM** — OpenAI-compatible API (OpenAI, Azure, OpenRouter, Ollama, etc.) via env vars
- **Agents** — pure TOML configs in `agents/`; drop a file to add a new voice
- **Observability** — every event streams over WebSocket to the UI

## Starter agents

| Agent       | Subscribes to | Publishes to          | Role                                  |
|-------------|---------------|-----------------------|---------------------------------------|
| Planner     | `goal`        | `plan`, `broadcast`   | Goal → structured plan                |
| Designer    | `plan`        | `spec`, `broadcast`   | Plan → detailed spec                  |
| Implementer | `spec`        | `result`, `broadcast` | Spec → working code                   |

Plus three bonus voices in `examples/agents/`: `researcher`, `code-reviewer`, `devops`.

## Tools (7)

`read_file` · `write_file` · `edit_file` · `list_dir` · `run_command` · `delegate_to` · `ask_user` (Designer also gets `search_code`)

## Quick start

```bash
cd council
echo "OPENAI_API_KEY=sk-..." > .env
docker compose up -d
cargo run -- serve &
cd ui && pnpm dev
```

Open <http://localhost:5173>, type a goal, watch the Council deliberate.

## Layout

```
council/
├── agents/            # starter agent TOML configs
├── examples/agents/   # bonus agent configs
├── ui/                # SvelteKit frontend
├── docs/              # design notes, wire contract, schemas
└── NOTES.md           # full project context dump
```

## See also

- [`NOTES.md`](./NOTES.md) — full project context (plan structure, wire contract, gotchas)
- [`docs/`](./docs/) — wire contract, agent TOML schema, dev workflow
