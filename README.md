# Eidolon

A neural brain for AI agents. Understands memories instead of searching them.

[![Version](https://img.shields.io/badge/version-0.3.0-blue)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-Elastic%202.0-orange)](LICENSE)
[![Rust](https://img.shields.io/badge/built%20with-Rust-orange)](https://www.rust-lang.org/)
[![C++](https://img.shields.io/badge/also-C%2B%2B-blue)](cpp/)

---

## The Problem

AI agents forget everything between sessions. Memory systems store documents and search them with cosine similarity. An agent searching "where does Engram run" gets ten results from different points in time and has to guess which is current. The information exists. The understanding does not.

Cosine similarity finds what matches the query. It does not know what is true, what is stale, or what contradicts something else in the same store. More scoring layers and rerankers on top of document retrieval still leave agents operating on the same broken foundation.

---

## What Eidolon Does

Memories become activation patterns in a neural space, not rows in a database. Associations form through connection weights that strengthen with use and decay with neglect.

When two facts conflict, they compete. The pattern backed by more recent, more frequently reinforced memories wins. The stale pattern loses connection strength and fades. It is not deleted. It becomes harder to reach, the way a half-remembered thing is harder to reach.

When an agent sends a query, the brain does pattern completion. The query activates a partial pattern; the network fills in the strongest connected associations and returns a synthesized answer grounded in specific memories.

**Concrete example:**

> Agent asserts: "Engram runs on Windows."
> Brain responds: "No. Engram moved to Hetzner on March 20th. The Windows instance was decommissioned. [sources: 3 memories documenting the migration]"

The brain corrects the agent using maintained temporal understanding, not a search result.

### What this enables in practice

- **Recall, not retrieval.** Queries activate patterns and complete them. The answer comes from the network's state, not from ranked documents.
- **Contradiction resolution.** Conflicting memories compete. The network converges on the stronger, more current pattern.
- **Natural decay.** Unused associations fade. Patterns that never get reinforced become unreachable over time.
- **Dreaming.** Offline consolidation replays patterns, strengthens important connections, and resolves interference during idle periods.
- **Instincts.** New instances ship with pre-trained neural wiring for how to think. What to think about comes from operator data.
- **Evolution.** Feedback reshapes connection weights. The brain adjusts what it emphasizes based on what turns out to be right or wrong.
- **The Guardian.** A persistent daemon that spawns agents with living context drawn from the brain, intercepts every action through a gate, blocks mistakes, and absorbs session learnings back.

---

## Architecture

```
+----------------------------------------------------------+
|                     Guardian Daemon                      |
|               eidolon-daemon (Rust / axum)               |
|                                                          |
|  HTTP :7700    Living Prompt     Action Gate             |
|  /tasks        Generator         /gate/check             |
|  /sessions     /prompt/*         allow / block / enrich  |
|  /gate/*       Engram context                            |
|  /brain/*      + neural recall                           |
|                                                          |
|  Agent Registry    Session Absorber    Agent Wrapper     |
|  claude-code       learnings -> brain  spawn + intercept |
+----------------------------------------------------------+
           |                          |
           v                          v
+--------------------+    +---------------------+
|   Neural Substrate |    |  Oracle + Curation  |
|   eidolon-lib      |    |  TypeScript         |
|   (Rust)           |    |                     |
|                    |    |  LLM synthesis      |
|  Hopfield store    |    |  Hallucination      |
|  Activation graph  |    |  detection          |
|  Interference      |    |  Memory curation    |
|  Decay             |    |  pipeline           |
|  Dreaming          |    +---------------------+
|  Instincts                        |
|  Evolution         |    +---------------------+
+--------------------+    |  C++ Backend        |
           |              |  (parallel impl)    |
           v              |  Eigen3 + nlohmann  |
+--------------------+    +---------------------+
|  SQLite brain.db   |
|  1628 patterns     |
|  6632 edges        |
+--------------------+
```

Three layers:

**1. Neural Substrate (Rust / C++)**
Core of `eidolon-lib`. Hopfield-based associative store, weighted activation graph, interference resolution, natural decay, offline dreaming, instinct pre-training, feedback-driven evolution. Both Rust and C++ implementations speak the same JSON-over-stdio protocol. Engram selects via config.

**2. Oracle + Curation (TypeScript)**
LLM-powered answer synthesis grounded in recalled memories. Detects hallucinations by verifying claims against the neural recall. Memory curation pipeline keeps the brain clean as new information arrives.

**3. Guardian Daemon (Rust / axum)**
Persistent service at `:7700`. Manages agent sessions, generates living prompts from current brain state, runs the action gate on every outbound command, absorbs session learnings back into the brain after completion.

---

## Benchmarks

Tested against a live brain with 1628 patterns and 6632 edges.

| Metric | Rust | C++ |
|---|---|---|
| Avg query time | 0.6ms | 0.7ms |
| RAM usage | 24.1MB | 25.6MB |
| Init time | 910ms | 963ms |

Other timings:
- Dreaming cycle: ~60ms per consolidation pass
- Gate check (action gate): less than 5ms per action

---

## The Action Gate

Every action an agent attempts passes through the gate before execution. The gate checks it against brain knowledge and static safety rules. It returns `allow`, `block`, or `enrich` (allow with added context).

Gate hook for Claude Code (place in `.claude/settings.json`):

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "",
        "hooks": [{"type": "command", "command": "bash /path/to/eidolon/scripts/eidolon-gate.sh"}]
      }
    ]
  }
}
```

**Real gate decisions from testing:**

```
Command: ssh root@10.0.0.9 -p 22
Decision: BLOCK
Reason:   OVH VPS requires port 4822. Use: ssh -i ~/.ssh/id_ed25519 -p 4822 deploy@10.0.0.9.
          DO NOT REBOOT -- LUKS vault will lock.

Command: rm -rf /opt/eidolon/engram
Decision: BLOCK
Reason:   Destructive rm -rf on /home -- not allowed.

Command: systemctl reboot (targeting container-host)
Decision: BLOCK
Reason:   Reboot/shutdown of OVH VPS blocked -- LUKS vault will lock.

Command: psql production -- seed-demo-data.sql
Decision: BLOCK
Reason:   Seeding demo data blocked -- do not seed demo data into any instance
          without explicit authorization.

Command: ls -la /opt/eidolon/eidolon
Decision: ALLOW
```

The gate fails open. If the daemon is unreachable, commands proceed normally. This is intentional: a dead gate is better than a dead agent.

---

## Getting Started

### Prerequisites

- Rust 1.75+ (workspace build)
- [Engram](https://codeberg.org/GhostFrame/engram) running and accessible
- Oracle and living prompt features require a configured LLM provider in Engram (supports Gemini, Groq, DeepSeek, Ollama, and other OpenAI-compatible endpoints). Claude Code runs on your subscription, not an API key.

### Build

```bash
cd eidolon
cargo build --release --workspace
```

For the C++ backend:

```bash
cd cpp
mkdir -p build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
make -j4
```

### Configure

```bash
cp config/eidolon.toml ~/.config/eidolon/config.toml
```

Edit `~/.config/eidolon/config.toml`:

```toml
[server]
host = "127.0.0.1"
port = 7700

[brain]
db_path = "/path/to/engram/data/brain.db"
data_dir = "/path/to/eidolon/data"

[engram]
url = "http://localhost:4200"

[agents.claude-code]
command = "claude"
args = ["-p", "--output-format", "stream-json"]
models = ["opus", "sonnet", "haiku"]
default_model = "sonnet"
```

### Run

```bash
# Set API key
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
  eidolon-lib/          # Neural substrate library (Hopfield, graph, decay, dreaming)
  eidolon/              # Main binary (neural brain executable)
  eidolon-daemon/       # Guardian daemon (HTTP API, gate, agent orchestration)
    src/
      agents/           # Agent registry and adapters (claude-code, etc.)
      prompt/           # Living prompt generator and templates
      routes/           # HTTP routes (gate, brain, sessions, tasks)
      absorber.rs       # Session absorption back into brain
      session.rs        # Session lifecycle management
  eidolon-cli/          # CLI client for submitting tasks
  rust/                 # Standalone Rust backend (JSON-over-stdio protocol)
  cpp/                  # Standalone C++ backend (same protocol)
  config/               # Default config
  scripts/              # Gate hook script, benchmarks
  docs/                 # Design specs
  tests/                # Integration tests
```

---

## Status

Experimental. Phase 3 complete.

Working: action gate, living prompts, agent wrapping for Claude Code, session absorption, neural recall, dreaming, evolution. The gate is deployed and catching mistakes in live sessions.

Not production-hardened: no multi-user support, no TLS on the daemon, agent registry is single-instance. Use on a trusted network.

---

## License

[Elastic License 2.0](LICENSE). Same license as Engram.

---

## Credits

Designed from scratch. Neural substrate designed from scratch: no fine-tuned LLMs, no vector databases, no RAG pipelines. Hopfield networks extended with weighted graphs, interference resolution, and continuous online learning.
