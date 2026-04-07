# Eidolon

Neural brain for AI agents. Understands memories instead of searching them. Captures new ones without being asked.

[![License](https://img.shields.io/badge/license-Elastic%202.0-orange)](LICENSE)
[![Rust](https://img.shields.io/badge/built%20with-Rust-orange)](https://www.rust-lang.org/)

---

## The Problem

AI agents forget everything between sessions. Memory systems store documents and search them with cosine similarity. An agent searching "where does Engram run" gets ten results from different points in time and has to guess which is current. The information exists. The understanding does not.

Cosine similarity finds what matches the query. It does not know what is true, what is stale, or what contradicts something else in the same store.

---

## What Eidolon Does

Memories become activation patterns in a neural space, not rows in a database. Associations form through connection weights that strengthen with use and decay with neglect.

When two facts conflict, they compete. The pattern backed by more recent, more frequently reinforced memories wins. The stale pattern fades. It becomes harder to reach, the way a half-remembered thing does.

When an agent sends a query, the brain does pattern completion. The query activates a partial pattern; the network fills in the strongest connected associations and returns a synthesized answer grounded in specific memories.

**Concrete example:**

> Agent asserts: "Engram runs on Windows."
> Brain responds: "No. Engram moved to the production server on March 20th. The previous instance was decommissioned. [sources: 3 memories documenting the migration]"

The brain corrects the agent using temporal understanding maintained across thousands of memory updates, not a ranked search result.

### Capabilities

- **Recall, not retrieval.** Queries activate patterns and complete them. The answer comes from the network's state, not from ranked documents.
- **Contradiction resolution.** Conflicting memories compete. The network converges on the stronger, more current pattern.
- **Natural decay.** Unused associations fade. Patterns that never get reinforced become unreachable over time.
- **Dreaming.** Offline consolidation replays patterns, strengthens important connections, and resolves interference during idle periods.
- **Growth.** Post-dream reflection produces observations that accumulate in Engram and get injected into living prompts via `/growth/materialize`.
- **Instincts.** New instances ship with pre-trained neural wiring for how to think. What to think about comes from operator data.
- **Evolution.** Feedback reshapes connection weights. The brain adjusts what it emphasizes based on what turns out to be right or wrong.
- **Action gate.** Every outbound command passes through a safety gate. Dangerous operations get blocked or enriched with warnings before execution.
- **Activity fan-out.** Agents report activity to one endpoint. Eidolon distributes to task tracking, event bus, action logging, quality evaluation, agent registry, memory storage, and the neural brain.

---

## Architecture

```
+----------------------------------------------------------+
|                     Eidolon Daemon                       |
|               eidolon-daemon (Rust / axum)               |
|                                                          |
|  Action Gate         Activity Fan-out     Growth         |
|  /gate/check         /activity            /growth/*      |
|  block / enrich      7-service fan-out    reflect +      |
|                                           materialize    |
|  Brain API           Prompt Generator     Sessions       |
|  /brain/query        /prompt/generate     /task, /stream |
|  /brain/dream        Engram + neural                     |
+----------------------------------------------------------+
           |                          |
           v                          v
+--------------------+    +---------------------+
|   Neural Substrate |    |  Engram             |
|   eidolon-lib      |    |                     |
|   (Rust)           |    |  Memory storage     |
|                    |    |  Hybrid search      |
|  Hopfield store    |    |  FSRS-6 decay       |
|  Activation graph  |    |  Knowledge graph    |
|  Interference      |    |  Personality engine |
|  Decay             |    +---------------------+
|  Dreaming          |
|  Instincts         |    +---------------------+
|  Evolution         |    |  Ollama (local LLM) |
+--------------------+    |  Classification     |
           |              +---------------------+
           v
+--------------------+
|  SQLite brain.db   |
+--------------------+
```

**Neural Substrate** (`eidolon-lib`): Hopfield-based associative store, weighted activation graph, interference resolution, natural decay, offline dreaming, instinct pre-training, feedback-driven evolution.

**Brain Binary** (`eidolon`): Standalone executable for direct neural brain operations. Pattern completion, dreaming cycles, instinct generation, and brain diagnostics outside the daemon.

**Guardian Daemon** (`eidolon-daemon`): Persistent service at `:7700`. Action gate, activity fan-out, prompt generation, growth reflection.

**Terminal UI** (`eidolon-tui`): WIP. Interactive TUI with a local LLM sidecar (llama-server on GPU). See [Terminal UI](#terminal-ui-wip) below.

**CLI** (`eidolon-cli`): Submit tasks and query status from the command line.

---

## Action Gate

The gate script (`scripts/eidolon-gate.sh`) intercepts shell commands via Claude Code's `PreToolUse` hook. Each command goes to `/gate/check`, which returns `allow`, `block`, or `enrich`. Blocked commands exit with code 2 and print the reason. Enriched commands add context and allow execution.

All hooks fail open. If the daemon is unreachable, commands proceed normally.

---

## Activity Endpoint

Agents report activity to `POST /activity` with one call. Eidolon fans out to:

- **Chiasm** (task tracking): creates or updates tasks per agent/project
- **Axon** (event bus): publishes events to appropriate channels
- **Broca** (action log): logs significant actions
- **Engram** (memory): stores completions and errors for cross-agent visibility
- **Soma** (agent registry): updates agent heartbeats and status
- **Thymus** (quality evaluation): records quality metrics from agent outcomes
- **Brain** (neural substrate): absorbs everything as activation patterns

```bash
curl -s http://localhost:7700/activity \
  -X POST -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"agent":"claude-code","action":"task.completed","summary":"Deployed v2","project":"myapp"}'
```

All fan-out is best-effort. Individual service failures are logged but do not fail the request.

---

## Growth System

After each dream cycle, there is a configurable chance that the daemon reflects on the results. An LLM reads the dream context alongside prior observations and produces one new observation (or nothing if nothing is notable). Observations accumulate in Engram and get injected into living prompts via `/growth/materialize`.

Other services in the ecosystem can use the same endpoints to reflect on their own activity.

### Endpoints

**POST /growth/reflect**: Send recent activity context, receive an LLM-generated observation.

**GET /growth/observations**: Fetch raw growth observations from Engram. Filter by service, limit, or timestamp.

**GET /growth/materialize**: Returns accumulated observations as formatted plain text, suitable for injection into a living prompt. Truncates at a configurable byte cap.

### Configuration

```toml
[growth]
enabled = true
reflection_chance = 0.20
```

---

## Terminal UI (WIP)

A Ratatui terminal interface that pairs a local LLM with the Eidolon daemon. Runs on Windows with a GPU-accelerated llama-server sidecar for fast local inference. The daemon provides brain queries, gate checks, and session management over HTTP and WebSocket.

Current state:

- Split-panel layout with local LLM chat on the left and a Claude session panel on the right
- Claude sessions stream output through the daemon's WebSocket endpoint
- Gate approval flow surfaces permission requests inline and lets you approve or deny from the TUI
- Stream-json output from Claude gets parsed into readable display lines
- Word-wrap-aware scrolling with a scrollbar that tracks actual row positions
- Growth reflections fire after each exchange
- Conversation exchanges auto-store to Engram

What remains: stability, connection resilience, and making it feel like a real daily-driver terminal rather than a prototype.

---

## Getting Started

### Prerequisites

- Rust 1.75+
- [Engram](https://github.com/Ghost-Frame/engram) running and accessible

### Build

```bash
cargo build --release -p eidolon-daemon
```

### Configure

```bash
cp config/config.example.toml ~/.config/eidolon/config.toml
```

Edit `~/.config/eidolon/config.toml`:

```toml
[server]
host = "127.0.0.1"
port = 7700

[brain]
db_path = "/path/to/brain.db"
data_dir = "/path/to/eidolon/data"

[engram]
url = "http://localhost:4200"
```

### Development Setup

```bash
# Enable the pre-commit hook (blocks commits containing private infrastructure details)
git config core.hooksPath .githooks
```

### Run

```bash
export EIDOLON_API_KEY=your-key

./target/release/eidolon-daemon
```

---

## Project Structure

```
eidolon/
  eidolon-lib/          # Neural substrate (Hopfield, graph, decay, dreaming, evolution)
  eidolon/              # Main binary (neural brain executable)
  eidolon-daemon/       # Guardian daemon (HTTP API, gate, agent orchestration)
    src/
      agents/           # Agent registry and adapters (claude-code)
      embedding/        # Pluggable embedding providers (Engram, OpenAI, HTTP)
      prompt/           # Living prompt generator and templates
      routes/           # HTTP routes (activity, gate, brain, sessions, tasks, audit, growth)
      absorber.rs       # Session absorption back into brain
      session.rs        # Session lifecycle management
    tests/              # Security pentest suite (113 tests)
  eidolon-tui/          # Terminal UI with local LLM and daemon integration
  eidolon-cli/          # CLI client
  config/               # Example configuration
  scripts/              # Gate hook script, benchmarks
  data/                 # Instinct pre-training data
```

---

## License

[Elastic License 2.0](LICENSE)

---

Neural substrate designed from scratch. No fine-tuned LLMs, no vector databases, no RAG pipelines. Hopfield networks extended with weighted graphs, interference resolution, and continuous online learning.

---

Support: **support@syntheos.dev** · Security: **security@syntheos.dev** · [Security Policy](SECURITY.md)
