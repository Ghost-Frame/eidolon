# Eidolon

A neural brain for AI agents. Understands memories instead of searching them.

[![License](https://img.shields.io/badge/license-Elastic%202.0-orange)](LICENSE)
[![Rust](https://img.shields.io/badge/built%20with-Rust-orange)](https://www.rust-lang.org/)

---

## The Problem

AI agents forget everything between sessions. Memory systems store documents and search them with cosine similarity. An agent searching "where does Engram run" gets ten results from different points in time and has to guess which is current. The information exists. The understanding does not.

Cosine similarity finds what matches the query. It does not know what is true, what is stale, or what contradicts something else in the same store.

---

## What Eidolon Does

Memories become activation patterns in a neural space, not rows in a database. Associations form through connection weights that strengthen with use and decay with neglect.

When two facts conflict, they compete. The pattern backed by more recent, more frequently reinforced memories wins. The stale pattern fades. It is not deleted. It becomes harder to reach, the way a half-remembered thing is harder to reach.

When an agent sends a query, the brain does pattern completion. The query activates a partial pattern; the network fills in the strongest connected associations and returns a synthesized answer grounded in specific memories.

**Concrete example:**

> Agent asserts: "Engram runs on Windows."
> Brain responds: "No. Engram moved to the production server on March 20th. The previous instance was decommissioned. [sources: 3 memories documenting the migration]"

The brain corrects the agent using maintained temporal understanding, not a search result.

### What this enables

- **Recall, not retrieval.** Queries activate patterns and complete them. The answer comes from the network's state, not from ranked documents.
- **Contradiction resolution.** Conflicting memories compete. The network converges on the stronger, more current pattern.
- **Natural decay.** Unused associations fade. Patterns that never get reinforced become unreachable over time.
- **Dreaming.** Offline consolidation replays patterns, strengthens important connections, and resolves interference during idle periods.
- **Instincts.** New instances ship with pre-trained neural wiring for how to think. What to think about comes from operator data.
- **Evolution.** Feedback reshapes connection weights. The brain adjusts what it emphasizes based on what turns out to be right or wrong.
- **The Guardian.** A persistent daemon that spawns agents with living context from the brain, intercepts every action through a gate, blocks mistakes, and absorbs session learnings back.
- **Activity fan-out.** Agents report activity to one endpoint. Eidolon distributes to task tracking, event bus, action logging, quality evaluation, agent registry, memory storage, and the neural brain.

---

## Architecture

```
+---------------------------------------+
|           Terminal UI (TUI)           |
|         eidolon-tui (Windows)         |
|                                       |
|  Local LLM (llama-server / GPU)       |
|  Parallel routing + chat              |
|  DaemonClient (HTTP + WebSocket)      |
+---------------------------------------+
           |  HTTP / WS
           v
+----------------------------------------------------------+
|                     Guardian Daemon                      |
|               eidolon-daemon (Rust / axum)               |
|                                                          |
|  HTTP :7700    Living Prompt     Action Gate             |
|  /task         Generator         /gate/check             |
|  /sessions     /prompt/generate  allow / block / enrich  |
|  /activity     Engram context                            |
|  /brain/*      + neural recall                           |
|                                                          |
|  Agent Registry    Session Absorber    Agent Wrapper     |
|  claude-code       learnings -> brain  spawn + intercept |
+----------------------------------------------------------+
           |                          |
           v                          v
+--------------------+    +---------------------+
|   Neural Substrate |    |  Engram + Syntheos  |
|   eidolon-lib      |    |                     |
|   (Rust)           |    |  Memory storage     |
|                    |    |  Fact decomposition |
|  Hopfield store    |    |  Task tracking      |
|  Activation graph  |    |  Event bus          |
|  Interference      |    |  Action logging     |
|  Decay             |    |  Agent registry     |
|                    |    |  Quality evaluation |
|                    |    +---------------------+
|  Dreaming          |
|  Instincts         |
|  Evolution         |
+--------------------+
           |
           v
+--------------------+
|  SQLite brain.db   |
+--------------------+
```

**Neural Substrate** (`eidolon-lib`): Hopfield-based associative store, weighted activation graph, interference resolution, natural decay, offline dreaming, instinct pre-training, feedback-driven evolution.

**Brain Binary** (`eidolon`): Standalone executable for direct neural brain operations -- pattern completion, dreaming cycles, instinct generation, and brain diagnostics outside the daemon.

**Guardian Daemon** (`eidolon-daemon`): Persistent service at `:7700`. Manages agent sessions, generates living prompts from brain state, runs the action gate on every outbound command, absorbs session learnings back into the brain. Unified `/activity` endpoint handles fan-out to all Syntheos services.

**Terminal UI** (`eidolon-tui`): One frontend for the daemon. Interactive TUI with a local LLM sidecar (llama-server on GPU) for routing and casual chat. Agent spawning, gate checks, brain queries, and session management all go through the daemon. Not required -- any agent that hits the daemon's API gets the same intelligence layer.

**CLI** (`eidolon-cli`): Submit tasks and query status from the command line.

---

## Integrating with Agents

The daemon is the core. The TUI is one frontend. Any agent that talks to the daemon's HTTP API gets the same intelligence layer -- living context, gate checks, brain recall, session absorption. A cloud-hosted agent (Claude Code, Cursor, etc.) running through the daemon gets the same capabilities as the local TUI.

### Claude Code Hooks

Full integration uses four hook points. Place these in `.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [{
          "type": "command",
          "command": "bash ~/.claude/hooks/session-start-engram.sh",
          "timeout": 30,
          "statusMessage": "Loading context from Eidolon brain..."
        }]
      }
    ],
    "Stop": [
      {
        "hooks": [{
          "type": "command",
          "command": "bash ~/.claude/hooks/session-end.sh",
          "timeout": 15
        }]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{
          "type": "command",
          "command": "bash /path/to/eidolon/scripts/eidolon-gate.sh",
          "timeout": 10
        }]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{
          "type": "command",
          "command": "bash ~/.claude/hooks/eidolon-activity.sh",
          "timeout": 10,
          "async": true
        }]
      }
    ]
  }
}
```

**What each hook does:**

| Hook | Lifecycle | Purpose |
|------|-----------|---------|
| SessionStart | Agent session begins | Calls `/prompt/generate` to inject brain-aware context. Registers with Soma via `/activity`. Falls back to direct Engram search if daemon is unreachable. |
| Stop | Agent session ends | Calls `/gate/complete` to enforce Engram store requirement. Reports `agent.offline` via `/activity` for fan-out. |
| PreToolUse (Bash) | Before every shell command | Sends command to `/gate/check`. Returns `allow`, `block`, or `enrich` (allow with added context). |
| PostToolUse (Bash) | After every shell command | Reports significant actions (git push, ssh, deploys, service management) to `/activity`. Async, never blocks. |

All hooks fail open. If the daemon is unreachable, commands proceed normally. A dead gate is better than a dead agent.

### The Gate

The gate script (`scripts/eidolon-gate.sh`) reads hook input from stdin, posts it to `/gate/check`, and interprets the response. Blocked actions exit with code 2 and print the reason to stderr. Enriched actions print added context to stderr and allow execution.

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

## Getting Started

### Prerequisites

- Rust 1.75+
- [Engram](https://codeberg.org/GhostFrame/engram) running and accessible

### Build

```bash
cargo build --release --workspace
```

For static binaries (cross-distro deployment):

```bash
cargo build --release --target x86_64-unknown-linux-musl -p eidolon-daemon
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

[agents.claude-code]
command = "claude"
args = ["-p", "--output-format", "stream-json"]
models = ["opus", "sonnet", "haiku"]
default_model = "sonnet"
```

### Development Setup

```bash
# Enable the pre-commit hook (blocks commits containing private infrastructure details)
git config core.hooksPath .githooks
```

### Run

```bash
export EIDOLON_API_KEY=your-key

# Start the daemon
./target/release/eidolon-daemon

# Submit a task
./target/release/eidolon-cli "your task here"
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
      prompt/           # Living prompt generator and templates
      routes/           # HTTP routes (activity, gate, brain, sessions, tasks, audit)
      absorber.rs       # Session absorption back into brain
      session.rs        # Session lifecycle management
    tests/              # Security pentest suite (72 tests)
  eidolon-tui/          # Terminal UI with local LLM + daemon integration
  eidolon-cli/          # CLI client
  config/               # Example configuration
  scripts/              # Gate hook script, benchmarks
  docs/                 # Design specs
  data/                 # Instinct pre-training data
```

---

## License

[Elastic License 2.0](LICENSE)

---

Neural substrate designed from scratch. No fine-tuned LLMs, no vector databases, no RAG pipelines. Hopfield networks extended with weighted graphs, interference resolution, and continuous online learning.

---

Support: **support@syntheos.dev** · Security: **security@syntheos.dev** · [Security Policy](SECURITY.md)
