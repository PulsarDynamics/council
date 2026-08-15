# AGENTS.md — Council

> **Audience**: any agent (AI or human) working in this repo. Read this first.
> **Source of truth**: this file. When this and a chat message disagree, this wins.
> **Updates**: keep it current. If you change a convention, update this in the same commit.

---

## 0. Working agreement (the "we" rule)

- This is a **collaborative** project. The user (Berk) and the agent work **together** — shared decisions, not top-down delegation. No "as you commanded" energy. Think "pair-programming partner who happens to be an AI."
- Speak directly. If a direction looks wrong, say so once, plainly, with reasoning. If the user insists, follow their lead.
- Match technical depth: the user is fluent in Rust, Svelte, Redis, WebSockets, TOML. Don't over-explain stack basics. Do explain non-obvious tradeoffs.
- Default language: English.

---

## 1. What Council is (and isn't)

**Council is**: a self-contained, multi-agent orchestration system. Rust backend (Axum + Tokio), SvelteKit UI, Redis pub/sub as the message bus. Agents are TOML configs, not code. Live WebSocket observability.

**Council is NOT**:
- A general-purpose agent framework. Don't add plugins/hooks for things unrelated to the Planner → Designer → Implementer pipeline (and their variants).
- A SaaS. No auth, multi-tenant, billing. Single-process, single-user (the operator).
- A distributed system. One orchestrator + N agent processes on the same machine, talking over local Redis.

If a contribution doesn't fit one of these, ask before adding it.

---

## 2. Build & dev

```bash
# one-time
cd ~/Desktop/gitlab-repos/council
echo "OPENAI_API_KEY=sk-..." > .env
docker compose up -d                # redis

# dev (two processes + UI)
cargo run -- serve                  # orchestrator on :8080
cargo run -- agent planner          # in a separate terminal — or use the dev script
cd ui && pnpm dev                   # SvelteKit on :5173
```

Then open <http://localhost:5173>, type a goal, watch the Council work.

**Rule of thumb**: if a workflow needs more than three commands to start, add it to `scripts/dev.sh`. Don't let runbook knowledge rot in chat.

---

## 3. Project layout

```
council/
├── Cargo.toml                      # workspace root
├── crates/
│   ├── council-core/               # shared types: Event, Tool, AgentSpec, wire schema
│   ├── council-orchestrator/       # `serve` subcommand — Axum + WS + agent process manager
│   └── council-agent/              # `agent` subcommand — LLM client + 7 tools + TOML loader
├── agents/                         # starter agent TOML configs
│   ├── planner.toml
│   ├── designer.toml
│   └── implementer.toml
├── examples/agents/                # bonus: researcher, code-reviewer, devops
├── ui/                             # SvelteKit (Svelte 5 + TS strict + Tailwind 4)
├── docs/
│   ├── WIRE_CONTRACT.md            # the 12 event types — full schema
│   ├── AGENT_SCHEMA.md             # TOML schema for agent configs
│   └── DEV.md                      # dev workflow, debugging, troubleshooting
├── scripts/
│   └── dev.sh                      # one-shot dev environment bootstrap
├── docker-compose.yml              # redis (only)
├── .env.example
├── README.md
├── NOTES.md                        # project context dump for cold starts
└── AGENTS.md                       # this file
```

When adding a new top-level file/dir, update this tree.

---

## 4. Code conventions

### Rust
- **Edition 2021+** (current stable). Pin MSRV in `Cargo.toml`.
- **No `unsafe`** unless a comment block justifies it. Default to safe Rust.
- **Errors**: `thiserror` for library errors, `anyhow` only in binary entry points (`main.rs`).
- **Async**: `tokio` everywhere. Don't mix runtimes.
- **Tracing**: `tracing` + `tracing-subscriber`. No `println!` in library code.
- **Public API**: every public item gets a doc comment. Internal items can skip.
- **Lints**: `cargo clippy --all-targets -- -D warnings` must pass before commit.

### Svelte / TS
- **Svelte 5** (runes). `$state`, `$derived`, `$effect` — not the old `let` / `$:` syntax.
- **TypeScript strict** (`"strict": true` in `tsconfig.json`). No `any` outside third-party shims.
- **Tailwind 4**. Utility classes. No `@apply` unless wrapping a design token.
- **Components**: PascalCase file names (`AgentCard.svelte`). One component per file.
- **Stores**: prefer runes over Svelte stores for new code.

### General
- **No dead code**. If you delete a feature, delete the code. Don't comment it out "for later."
- **No magic numbers**. Name them as constants.
- **Comments explain WHY, not WHAT**. If a function needs a comment to explain what it does, rename the function.

---

## 5. Agent authoring

Adding an agent = drop a TOML file in `agents/` (or `examples/agents/`). **No code changes required.**

### TOML schema (sketch — see `docs/AGENT_SCHEMA.md` for full)

```toml
[agent]
name = "designer"          # MUST match filename stem. Case-sensitive!
subscribes = ["plan"]      # channels to listen on
publishes = ["spec"]       # channels to publish to

[model]
provider = "openai"        # openai | openrouter | ollama | azure
name = "gpt-4o"
temperature = 0.3

[prompt]
system = "..."             # the system prompt
template = "..."           # how to render incoming messages

[tools]
allowed = ["read_file", "search_code", "ask_user"]
```

### Gotchas (learned the hard way)

- **Agent names are case-sensitive.** A config file `designer.toml` with `name = "Designer"` will silently fail to match. Keep them in sync.
- **Channel names are case-sensitive** and treated as exact strings by Redis. Convention: lowercase_snake.
- **`delegate_to` is the topology primitive.** Any agent can call any other. Don't hardcode caller/callee pairs in prompts.
- **Tool allow-lists are per-agent.** A tool exists in the binary but the agent won't see it unless it's listed in `[tools].allowed`.

---

## 6. Wire contract (12 event types)

See `docs/WIRE_CONTRACT.md` for the full schema. Short version:

`user_message` · `agent_message` · `agent_thinking` · `tool_call` · `tool_result` · `file_change` · `agent_status` · `llm_call` · `system` · `session_created` · `session_completed` · `error`

**Rules**:
- Every event has a stable `id` (UUID), `session_id`, `timestamp` (RFC3339), and `kind` discriminator.
- Adding a new event type is a breaking change to the wire contract. Bump the contract version in `docs/WIRE_CONTRACT.md` and update the UI's TypeScript types in the same PR.
- The orchestrator never fabricates events. If something happened, it gets a kind. If it didn't, it doesn't.

---

## 7. Testing

- **Unit tests** live next to the code (`#[cfg(test)] mod tests`).
- **Integration tests** in `crates/*/tests/`.
- **E2E test** in `tests/e2e/` — spins up real Redis + orchestrator + one agent, sends a goal, asserts on the event stream.
- **No mocking the LLM.** Either use a real provider (env-driven) or skip the LLM-dependent tests in CI. Mocks drift.
- **`cargo test` and `pnpm test` must pass before commit.**

---

## 8. Git conventions

- **Trunk-based.** `main` is always deployable. Feature branches off `main`, merged back via PR.
- **Commit messages**: imperative mood, ≤ 72 char subject, blank line, body. Use `git commit -m` with the body via `git commit -m "subject" -m "body"` (this repo's `.gitmessage` lives at `~/Desktop/gitlab-repos/.gitmessage` — feel free to copy that style).
- **One concern per commit.** Don't bundle "fix Planner" + "rename UI button" + "bump dep."
- **No force-push to `main`.** Ever.
- **PRs**: small, focused, with a one-line description and a "how I tested" line.

---

## 9. Out-of-scope (for now)

- Tauri desktop wrap — parked. Easy to add later since we're already in Rust.
- Multi-tenant / auth — not in scope.
- Custom LLM providers beyond OpenAI-compatible — not in scope until requested.

---

## 10. When in doubt

1. Read this file.
2. Read `docs/WIRE_CONTRACT.md` and `docs/AGENT_SCHEMA.md`.
3. Read `NOTES.md` for the why behind the design.
4. Then ask.
