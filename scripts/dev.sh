#!/usr/bin/env bash
# scripts/dev.sh — one-shot dev environment for Council.
# Starts Redis, the orchestrator, the planner agent, and the SvelteKit UI.
# Each runs in its own process. Ctrl-C in this terminal kills the orchestrator
# and planner; Redis and the UI are left running (Ctrl-C twice to kill the UI).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# --- 0. sanity ---
command -v docker >/dev/null 2>&1 || { echo "docker is required"; exit 1; }
command -v cargo  >/dev/null 2>&1 || { echo "cargo is required";  exit 1; }
command -v pnpm   >/dev/null 2>&1 || { echo "pnpm is required";   exit 1; }

# --- 1. .env ---
if [[ ! -f .env ]]; then
  echo "No .env found. Copying .env.example -> .env — fill in OPENAI_API_KEY before continuing."
  cp .env.example .env
  echo "Edit .env then re-run scripts/dev.sh."
  exit 1
fi
# shellcheck disable=SC1091
set -a; source .env; set +a

# --- 2. Redis ---
if ! docker ps --format '{{.Names}}' | grep -q '^council-redis$'; then
  echo "Starting redis (docker compose)..."
  docker compose up -d redis
fi

# --- 3. Build (cached if nothing changed) ---
echo "Building workspace (cargo build --workspace)..."
cargo build --workspace

# --- 4. Launch orchestrator + planner agent in background ---
echo "Starting orchestrator (logs: /tmp/council-orchestrator.log)..."
RUST_LOG="${RUST_LOG:-info}" \
  ./target/debug/council-orchestrator serve --bind "${COUNCIL_BIND:-0.0.0.0:8080}" \
  > /tmp/council-orchestrator.log 2>&1 &
echo $! > /tmp/council-orchestrator.pid

echo "Starting planner agent (logs: /tmp/council-agent-planner.log)..."
RUST_LOG="${RUST_LOG:-info}" \
  ./target/debug/council-agent run --config agents/planner.toml \
  > /tmp/council-agent-planner.log 2>&1 &
echo $! > /tmp/council-agent-planner.pid

cleanup() {
  echo "Stopping background processes..."
  [[ -f /tmp/council-orchestrator.pid ]] && kill "$(cat /tmp/council-orchestrator.pid)" 2>/dev/null || true
  [[ -f /tmp/council-agent-planner.pid ]] && kill "$(cat /tmp/council-agent-planner.pid)" 2>/dev/null || true
  rm -f /tmp/council-orchestrator.pid /tmp/council-agent-planner.pid
}
trap cleanup EXIT INT TERM

echo
echo "Council running."
echo "  Orchestrator:  http://${COUNCIL_BIND:-0.0.0.0:8080}"
echo "  Planner agent: PID $(cat /tmp/council-agent-planner.pid)"
echo
echo "Now starting SvelteKit UI in the foreground. Ctrl-C here to stop everything."
echo

# --- 5. UI in foreground ---
cd "$REPO_ROOT/ui"
pnpm dev
