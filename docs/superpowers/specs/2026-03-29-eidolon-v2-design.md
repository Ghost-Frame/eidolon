# Eidolon v2 Design Spec

## Overview

Eidolon v2 is a conversational terminal agent that runs locally on Windows. It is backed by a free local LLM (Qwen 3 14B via llama.cpp sidecar) and delegates real coding/infrastructure work to Claude Code and Codex CLI sessions. The user talks to Eidolon naturally -- casual chat, memory queries, infrastructure questions, movie recommendations -- and Eidolon only spawns expensive cloud agents when the task genuinely requires them.

**Personality**: Satoru Gojo from Jujutsu Kaisen. Cocky, confident, playful by default. Dead serious when stakes are high. Protective/guardian energy. Not a parody -- a faithful adaptation of the character's voice and attitude.

**Platform**: Windows 11, RTX 5070 (12GB VRAM), terminal TUI only. No web UI. No remote API. Local-first application.

**Language**: Rust. TUI via ratatui. HTTP client for llama-server and Syntheos APIs.

---

## Architecture

### The Conductor Model

Eidolon v2 follows a "conductor" architecture. The local LLM is the conversational brain and routing layer. It handles all direct interaction with the user, calls Syntheos HTTP APIs for memory/infrastructure queries, and spawns Claude Code or Codex sessions only when cloud-tier intelligence is needed.

```
User <-> TUI <-> Conversation Manager <-> Local LLM (llama-server)
                                      |
                                      +-> Syntheos APIs (Engram, Chiasm, Broca, etc.)
                                      +-> OpenSpace Graph Intelligence
                                      +-> Eidolon Brain (Hopfield neural substrate)
                                      +-> Agent Orchestrator -> Claude Code / Codex CLI
```

### Component Map

| Component | Responsibility | Implementation |
|-----------|---------------|----------------|
| TUI | User interface, theming, animations, agent panel rendering | ratatui + crossterm |
| Conversation Manager | Message history, context window, intent routing, personality | Rust, calls llama-server HTTP |
| LLM Sidecar | Local inference | llama-server (llama.cpp HTTP server) |
| Agent Orchestrator | Spawns/monitors/gates Claude Code and Codex sessions | Rust, subprocess management |
| Living Prompt Generator | Builds dynamic CLAUDE.md for spawned sessions | Rust, Engram/OpenSpace queries |
| Gate System | Validates dangerous operations before execution | Rust, enriched with OpenSpace blast radius |
| Brain | Hopfield network pattern storage, dreaming, decay | eidolon-lib (existing Rust crate) |
| Theme Engine | Selectable themes, animations, background patterns | ratatui widgets + Unicode art |

### Sidecar Architecture

The LLM runs as a separate process (llama-server) communicating over localhost HTTP. This was chosen over embedded FFI or a separate daemon for these reasons:

- Independent updates (swap models without recompiling Eidolon)
- Battle-tested HTTP server with OpenAI-compatible API
- Clean process isolation (LLM crash does not crash TUI)
- Negligible localhost HTTP overhead (~0.1ms per request)
- llama-server handles GPU memory management, KV cache, batching

Startup sequence:
1. Eidolon TUI launches
2. Checks if llama-server is running on configured port (default 8080)
3. If not, spawns: `llama-server -m <model_path> -c 8192 -ngl 99 --port 8080`
4. Polls `GET /health` until ready (timeout 30s)
5. Loads system prompt with Gojo personality + tool descriptions
6. Ready for conversation

If llama-server crashes mid-session, Eidolon detects via failed health check, notifies user, and offers to restart.

---

## Tool Layer

All Syntheos services are HTTP APIs rooted at `$ENGRAM_URL` (Engram on Hetzner, port 4200 for production or a dedicated instance for Eidolon). The local LLM calls these directly via HTTP.

### Syntheos Core Services

| Service | Prefix | What Eidolon Uses It For |
|---------|--------|------------------------|
| Engram | `/search`, `/store`, `/recall`, `/context` | Persistent memory. Search before answering questions. Store outcomes after tasks. |
| Chiasm | `/tasks`, `/feed` | Task tracking. Create task on session start, update during, complete on end. Check what other agents are doing. |
| Broca | `/broca/*` | Action logging and natural language infrastructure queries. `/broca/ask` for plain-English questions about what has happened. |
| Axon | `/axon/*` | Event bus. Publish agent.online/offline, task events, deploy events. Subscribe to channels for awareness. |
| Soma | `/soma/*` | Agent registry. Register Eidolon on startup, heartbeat during session, log significant actions. |
| Thymus | `/thymus/*` | Quality scoring. Evaluate routing accuracy, personality consistency. Record metrics (tokens used, sessions spawned). |
| Loom | `/loom/*` | Workflow orchestration. Multi-step pipelines with approval checkpoints (plan -> review -> execute). |
| Cred/credd | Port 4400, `/secret/*` | Encrypted credential vault. Pull API keys, passwords, tokens at runtime. Never hardcode. |

### Graph Intelligence (OpenSpace)

OpenSpace is the graph/structural analysis layer inside Engram. It provides relationship-aware memory retrieval that goes beyond flat keyword search.

**HTTP Endpoints** (`$ENGRAM_URL/structural/*`):

| Endpoint | What It Does | How Eidolon Uses It |
|----------|-------------|-------------------|
| `structural_analyze` | Topology classification of a graph | Understand the shape of a problem domain before routing |
| `structural_detail` | Deep analysis of a specific node | Get full context on a memory/concept |
| `structural_between` | Find connections between two nodes | Answer "how are these related?" |
| `structural_distance` | Shortest path length between nodes | Gauge how far apart two concepts are |
| `structural_trace` | Directed flow trace through graph | Trace dependency chains |
| `structural_impact` | Blast radius of removing/changing a node | Assess risk before infrastructure changes |
| `structural_diff` | Compare two graph states | Detect what changed between snapshots |
| `structural_evolve` | Dry-run a graph patch | Preview what would happen if we made a change |
| `structural_categorize` | Classify nodes by community | Pull clusters of related memories |
| `structural_extract` | Extract a subsystem from the graph | Isolate relevant context for a specific domain |
| `structural_compose` | Merge two graphs | Combine context from multiple domains |
| `structural_memory_graph` | Analyze Engram's own memory links | Build richer living prompts with structural context |

**Built-in Graph Endpoints**:

| Endpoint | What It Does |
|----------|-------------|
| `/graph` | Returns community_id + pagerank_score per node |
| `/communities` | Community detection stats (582+ communities) |
| `/graph/timeline` | Weekly aggregates (node/edge count over time) |

**How Eidolon leverages OpenSpace**:

1. **Context-aware routing**: Before routing a request to Claude/Codex, Eidolon calls `structural_trace` and `structural_impact` to understand what the request touches. "Refactor the auth module" triggers a blast radius check. Results get injected into the living prompt.

2. **Community-based retrieval**: Instead of flat Engram search, Eidolon uses `structural_categorize` and community IDs to pull clusters of related memories. "Tell me about the deploy pipeline" retrieves the entire deploy community, not just keyword matches.

3. **Relationship discovery**: `structural_between` and `structural_distance` let Eidolon answer relationship questions natively without needing Claude.

4. **Living prompt enrichment**: `structural_memory_graph` provides graph structure for richer context sections in prompts sent to Claude -- not just "here are memories" but "here is how they relate."

### Neural Substrate (Eidolon Brain)

The existing eidolon-lib Hopfield network crate is a direct Rust dependency. No HTTP calls -- native library integration.

| Capability | What It Does |
|-----------|-------------|
| Pattern storage | Store interaction patterns as high-dimensional vectors (BRAIN_DIM=512) |
| Association | Find patterns similar to current context (ASSOCIATION_THRESHOLD=0.4) |
| Decay | Unused patterns fade over time (BASE_DECAY_RATE=0.995) |
| Dreaming | Background process that replays and strengthens important patterns |
| Interference detection | Identify when new patterns conflict with existing ones |
| PCA | Dimensionality reduction for visualization and analysis |

The brain stores routing patterns (what kinds of requests need Claude vs local handling), personality calibration data (which responses landed well), and infrastructure interaction patterns. It complements Engram's semantic memory with a neural associative layer.

### Code Agents

| Agent | When Used | Invocation |
|-------|----------|-----------|
| Claude Code | Complex coding, architecture, infrastructure, multi-file changes | `claude -p --output-format stream-json` subprocess |
| Codex CLI | Coding tasks, testing capabilities vs other models, user preference | `codex` subprocess |

Agent selection is recommended by Eidolon based on task analysis but always overridable by the user. Eidolon presents its recommendation with reasoning:

```
Gojo: I'd use Opus for this -- it's architectural work across
multiple services. But if you want to see what Codex can do
with it, say the word.
```

The user can override at any time. Eidolon tracks which agent/model was used for what task type and learns from the outcomes to improve future recommendations.

---

## Conversation Manager

### Personality Engine

Gojo's personality is defined in a structured system prompt loaded into every llama-server request. This is not a one-line "you are Gojo" instruction -- it is a multi-section personality document covering:

**Voice patterns**: Cocky confidence ("You think that's hard? Please."), casual dismissiveness of easy tasks, genuine excitement about interesting problems, protective energy toward the user's infrastructure, playful teasing.

**Mode switching**:
- Default: Playful, cocky, conversational
- Action mode: Focused, strategic, still confident but briefer
- Serious mode: When detecting destructive operations or production risk -- drops the jokes, gets direct
- The transition between modes should feel natural, not mechanical

**Knowledge framing**: Gojo frames his capabilities honestly. He knows he is a local LLM with limited context. When something is beyond him, he says so with Gojo's characteristic bluntness: "That's a job for the real heavyweights. Let me get Claude on it."

**Conversation style**: Natural back-and-forth. Can discuss anime, games, movies, general topics. Not a search engine -- has opinions, makes recommendations, remembers preferences (via Engram). Can be wrong and should admit it casually, not apologetically.

### Intent Router

Every user message is classified into one of three categories. The classification happens as part of the LLM's response generation -- the system prompt instructs the model to output a structured routing block before its conversational response.

| Intent | Behavior | Examples |
|--------|---------|---------|
| **Casual** | LLM responds directly. No external calls. | "What anime should I watch?", "Tell me a joke", "What do you think about Rust vs Go?" |
| **Memory/Query** | LLM calls Syntheos HTTP APIs, synthesizes answer. | "What did we deploy last week?", "What's running on Rocky?", "Show me recent Chiasm tasks" |
| **Action** | LLM formulates plan, presents for approval, spawns agent session. | "Refactor the auth module", "Deploy the new Engram version", "Fix the nginx config on Pangolin" |

The router uses a structured output format enforced by llama.cpp's GBNF grammar:

```json
{
  "intent": "casual|memory|action",
  "confidence": 0.0-1.0,
  "tools_needed": ["engram_search", "broca_ask"],
  "agent_needed": null | "claude" | "codex",
  "reasoning": "brief explanation"
}
```

If the router picks wrong, the user corrects naturally ("no, actually look that up in Engram" or "just use Codex for this") and Eidolon re-routes without friction.

### Context Window Management

The local LLM has limited effective context (8192 tokens initially, potentially 16K). The Conversation Manager maintains context quality through:

**Sliding window**: Recent messages are kept in full. Older messages get summarized.

**Pinned context**: A section at the top of the context window that contains:
- Current Engram context (refreshed at conversation boundaries)
- Active Chiasm tasks
- Recent Axon events
- Server state summary (from Broca)

**Memory loop**: When context approaches capacity:
1. Summarize the oldest messages in the window
2. Store the summary to Engram with conversation metadata
3. Replace the full messages with the summary in the context
4. Continue conversation with freed space

This is manual compaction managed by Eidolon's own code, not dependent on the LLM's capability.

**Session persistence**: When Eidolon exits, the full conversation is stored to Engram. On restart, the user can resume with "where were we?" and Eidolon reconstructs context from stored memories.

---

## Agent Orchestrator

### Session Planning

When the intent router determines a task needs Claude or Codex, Eidolon does not blindly forward the user's message. It builds an optimized session:

1. **Context gathering**: Queries Engram for relevant memories, OpenSpace for graph communities and blast radius, Broca for recent related actions
2. **Agent selection**: Recommends Claude Code (Opus/Sonnet) or Codex based on task complexity, with reasoning shown to user. User can override.
3. **Living prompt generation**: Assembles a dynamic CLAUDE.md/system prompt containing:
   - Relevant Engram memories (filtered by structural analysis, not just keyword match)
   - Server map subset (only servers relevant to the task)
   - Syntheos API references (only services the agent needs)
   - Safety rules and gate constraints
   - Task framing with acceptance criteria
   - OpenSpace graph context (community relationships, dependency chains)

### Living Prompt Template

The living prompt is dynamically generated per session. It contains:

```
# Eidolon Session Context
Generated: {timestamp}
Task: {task_description}

## Relevant Context
{engram_memories -- filtered by OpenSpace structural_extract}

## Server Map
{subset of server map relevant to task}

## Active Work
{chiasm_tasks -- what other agents are currently doing}

## Safety Rules
{gate_rules -- operations that require confirmation}

## Syntheos APIs Available
{api_references -- only services relevant to this task}

## Graph Context
{openspace_community -- structural relationships around the task domain}
```

### Supervised Execution

Spawned Claude/Codex sessions stream output back through Eidolon's TUI. The user sees everything in real-time inside an embedded agent panel.

**Capabilities during active sessions**:
- **Inject instructions**: User tells Eidolon, Eidolon formats and injects into the agent session
- **Kill session**: `Ctrl+K` or "kill it" to terminate a runaway session
- **Gate operations**: Dangerous operations (rm -rf, force push, DROP TABLE, etc.) are caught by the gate system. Eidolon presents them to the user with risk assessment (enriched by OpenSpace blast radius data) before allowing execution
- **Auto-capture**: Session outcomes, key decisions, and errors are automatically stored to Engram. The user does not need to manually record what happened.

### Multi-Agent Orchestration

For complex workflows, Eidolon uses Loom to create multi-step pipelines with approval checkpoints:

```
Example: "Plan a refactor of Engram, let me review, then execute"

Loom Workflow:
  Step 1: [plan]     -- Spawn Claude Opus, generate refactor plan
  Step 2: [review]   -- Present plan to user, wait for approval
  Step 3: [execute]  -- Spawn Claude Sonnet, execute approved plan step by step
  Step 4: [verify]   -- Run tests, check for regressions
  Step 5: [report]   -- Summarize what was done, store to Engram
```

Each step can use a different agent or model. The local LLM manages workflow state and transitions. The user stays in one Eidolon conversation throughout.

---

## LLM Integration

### Runtime

**llama-server** (llama.cpp HTTP server) running as a sidecar process on localhost.

### Model

**Qwen 3 14B Q4_K_M** (~9GB VRAM). On the RTX 5070's 12GB, this leaves ~3GB for KV cache and system overhead.

### Configuration

| Parameter | Value | Notes |
|-----------|-------|-------|
| Context length | 8192 tokens | Start here, bump to 16K if quality holds |
| GPU layers | 99 (all) | Full GPU offload |
| Port | 8080 | Configurable |
| Temperature (casual) | 0.7 | Natural conversation |
| Temperature (routing) | 0.3 | Deterministic tool calls |
| Prompt format | ChatML | Qwen 3 native format |

### API Surface

Eidolon communicates with llama-server via the OpenAI-compatible endpoint:

```
POST http://localhost:8080/v1/chat/completions
```

Request body follows the standard chat completion format with system/user/assistant messages. Streaming is used for real-time token display in the TUI.

### Grammar Constraints

For routing decisions and tool calls, llama.cpp's GBNF grammar system constrains output to valid JSON. This prevents the LLM from generating malformed tool calls or hallucinating invalid API endpoints.

Example grammar for intent routing:
```gbnf
root ::= "{" ws "\"intent\":" ws intent "," ws "\"confidence\":" ws number "," ws "\"tools_needed\":" ws tools "," ws "\"agent_needed\":" ws agent "," ws "\"reasoning\":" ws string "}" ws
intent ::= "\"casual\"" | "\"memory\"" | "\"action\""
agent ::= "null" | "\"claude\"" | "\"codex\""
tools ::= "[" ws (string ("," ws string)*)? "]"
```

---

## TUI Design

### Philosophy

Eidolon's TUI is not a utilitarian terminal window. It is a place the user lives in for hours. It must be visually engaging, fun to look at, and reflect the personality of the agent inside it. Synapse's theme system and animation concepts are the direct inspiration.

The constraint: all rendering is Unicode + ANSI colors + ratatui widgets. Zero GPU usage for rendering. The RTX 5070 stays fully dedicated to the LLM. Terminal rendering is CPU-side and trivial.

### Layout

```
+-------------------------------------------------------------+
| eidolon v2  [infinity-symbol]  gojo mode   [llm: ready] [ctx: 3.2k/8k] [theme: jujutsu] |
+-------------------------------------------------------------+
|                                                             |
|  Gojo: You want me to refactor auth? Please. I could       |
|  do that blindfolded. But let's not be reckless -- here's   |
|  what I found in your memories:                             |
|                                                             |
|  +-- claude session: refactor-auth ---------------------+   |
|  | Reading src/auth/middleware.ts...                     |   |
|  | Found 3 files with circular dependencies...          |   |
|  | Proposing fix: extract shared types to...            |   |
|  +------------------------------------------------------+   |
|                                                             |
|  Gojo: See? Child's play. Want me to let it execute,       |
|  or do you wanna review first?                              |
|                                                             |
+-------------------------------------------------------------+
| > _                                                         |
+-------------------------------------------------------------+
```

**Status bar** (top): LLM health indicator, context usage meter, active theme name, active agent session count. The infinity symbol (Gojo's Limitless technique) animates subtly.

**Chat area** (main, scrollable): Gojo's messages with personality-appropriate formatting. Embedded agent session panels that show streaming Claude/Codex output inline. Agent panels are collapsible.

**Input bar** (bottom): User input. Multi-line with Shift+Enter. Command completion for `/theme`, `/kill`, `/status`, etc.

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| Enter | Send message |
| Shift+Enter | New line in input |
| Ctrl+K | Kill active agent session |
| Ctrl+L | Clear chat display |
| Ctrl+T | Cycle theme |
| Esc | Cancel current LLM generation |
| Up/Down | Scroll chat history |
| Tab | Command completion |

### Theme System

Selectable themes with full color palettes. Each theme defines:

```rust
struct Theme {
    name: String,
    // Core colors
    accent: Color,           // Primary accent (borders, highlights)
    accent_hover: Color,     // Lighter accent variant
    dim: Color,              // Muted/secondary text
    text: Color,             // Primary text
    text_secondary: Color,   // Secondary text
    bg: Color,               // Background
    bg_secondary: Color,     // Panel/card backgrounds
    // Semantic colors
    error: Color,
    success: Color,
    warning: Color,
    // Agent-specific
    tool_call: Color,        // Tool/API call highlighting
    thinking: Color,         // LLM thinking indicator
    gojo_text: Color,        // Gojo's message color
    user_text: Color,        // User's message color
    agent_border: Color,     // Agent session panel border
    // Animation colors
    pulse_start: Color,      // Animation start state
    pulse_end: Color,        // Animation end state
}
```

**Built-in themes** (inspired by Synapse + Pi):

| Theme | Vibe | Accent | Background |
|-------|------|--------|------------|
| **jujutsu** (default) | Cursed energy. Purple/blue with electric highlights. | #7C3AED (purple) | Deep void (#08090c) |
| **limitless** | Gojo's Infinity. Clean white/blue on black. | #38bdf8 (sky blue) | Pure black (#000000) |
| **cyberpunk** | Neon on void. Pi's zanverse theme. | #00f0ff (cyan) | Void (#0a0e17) |
| **hollow** | Domain Expansion. Red/black with amber warnings. | #ff3344 (crimson) | Deep black (#0c0c0c) |
| **synapse** | Warm dark. Synapse's original palette. | #ffaa00 (warm gold) | Dark gray (#08090c) |
| **tokyo** | Tokyo Night. Cool blues. | #7AA2F7 (blue) | #1a1b26 |
| **minimal** | Clean, distraction-free. | #888888 (gray) | #000000 |

Themes are switchable at runtime via `/theme <name>` or `Ctrl+T` to cycle. Stored in config file.

### Animations

All animations are frame-based using ratatui's render loop (typically 30fps). They use only Unicode characters and ANSI color transitions -- no GPU rendering.

**Infinity symbol pulse**: The infinity symbol in the status bar subtly pulses between accent and dim colors on a 2-second cycle. Represents Gojo's Limitless technique and indicates the system is alive.

**Thinking indicator**: When the LLM is generating, an animated spinner with themed colors plays next to "thinking..." text. Uses braille characters for smooth rotation.

**Streaming text**: LLM responses appear with a natural typing cadence, not instant dump. Characters stream in at inference speed (which is already natural for local LLM).

**Agent panel activation**: When a Claude/Codex session spawns, the agent panel border does a brief "power up" animation -- border color sweeps from dim to accent over ~0.5 seconds. On completion, a brief success/failure flash.

**Border breathing**: Subtle 4-second cycle where active panel borders shift between two close color values. Gives the UI a sense of life without being distracting. Inspired by Synapse-GUI's `breathe-border` animation.

**Status transitions**: When LLM state changes (ready -> generating -> tool_call -> ready), the status bar indicator transitions smoothly between colors rather than snapping.

### Background Patterns

Terminal backgrounds using Unicode block characters and styled with theme colors at very low opacity/contrast. These are pre-computed and static per theme -- no runtime rendering cost.

Options per theme:
- **Grid pattern**: Subtle dot grid (like Synapse-GUI's grid-bg) using Unicode dots at very low contrast
- **None**: Clean solid background for minimal themes
- **Scanlines**: Faint horizontal lines for retro/cyberpunk themes

Background pattern is configurable per theme and can be disabled globally.

### Agent Session Panels

When Claude or Codex is active, their output appears in an embedded panel within the chat area:

```
+-- claude opus | refactor-auth | 45s | 2.3k tokens ------+
| Reading src/auth/middleware.ts...                         |
| Found 3 files with circular dependencies:                |
|   - auth/middleware.ts -> auth/types.ts -> auth/utils.ts  |
| Proposing fix: extract shared types to auth/shared.ts    |
+-----------------------------------------------------------+
```

Panel header shows: agent name, model, task label, elapsed time, token usage.
Panel border uses `agent_border` theme color with breathing animation when active.
Panels are collapsible with a keybinding (toggle fold).
Multiple panels can be active simultaneously (multi-agent workflows).

---

## Configuration

### Config File

`~/.config/eidolon/config.toml` on Windows (or platform-appropriate config dir).

```toml
[llm]
model_path = "C:/Users/Zan/models/qwen3-14b-q4_k_m.gguf"
context_length = 8192
port = 8080
gpu_layers = 99

[engram]
url = "http://100.64.0.13:4203"  # Dedicated Eidolon instance, NOT production
api_key_service = "engram"
api_key_name = "api-key-eidolon"

[credd]
url = "http://100.64.0.2:4400"  # Rocky credd
# Agent key pulled from env or credd bootstrap

[agents.claude]
command = "claude"
args = ["-p", "--output-format", "stream-json", "--verbose"]
default_model = "opus"

[agents.codex]
command = "codex"
args = []

[tui]
theme = "jujutsu"
background_pattern = true
animations = true
fps = 30

[brain]
db_path = "~/.local/share/eidolon/brain.db"
dimension = 512
decay_rate = 0.995

[session]
auto_store_to_engram = true
max_context_messages = 50
```

### Engram Instance

Eidolon uses a **dedicated Engram instance** separate from production. This prevents any risk of Eidolon corrupting or polluting production memory while the system is being developed and tuned.

The dedicated instance runs on Hetzner at port 4203 (already configured in the existing Eidolon daemon config). It should have its own database and not sync from production.

---

## Fine-tuning Dataset Structure (Future)

Not built at launch. Eidolon v2 ships with base Qwen 3 14B and a well-crafted system prompt. The architecture collects training data from day one for future fine-tuning.

### What Gets Recorded

With user consent, every interaction is logged to a JSONL file:

- User message and intent classification (casual/memory/action)
- Routing decision and whether user overrode it
- Tool calls made and their results
- Agent delegation decisions (which agent, which model, why)
- Personality responses and user reactions (implicit signal: did user say "just tell me normally"?)

### Dataset Format

```json
{
  "messages": [
    {"role": "system", "content": "<gojo_system_prompt>"},
    {"role": "user", "content": "What did we deploy last week?"},
    {"role": "assistant", "content": "<routing_json>\n<gojo_response>"}
  ],
  "metadata": {
    "intent": "memory",
    "tools_called": ["engram_search", "broca_ask"],
    "user_override": false,
    "timestamp": "2026-04-15T14:30:00Z"
  }
}
```

### When to Fine-tune

After 2-4 weeks of real usage with enough signal to train. LoRA fine-tune on the 14B base targeting:
1. Routing accuracy (correct intent classification)
2. Personality consistency (Gojo's voice)
3. Tool call formatting (valid JSON, correct API endpoints)

The fine-tuned model produces a new GGUF that drops into the same llama-server slot. No architecture changes needed.

---

## Migration from v1

### What Stays

| Component | Current Location | In v2 |
|-----------|-----------------|-------|
| eidolon-lib (Hopfield brain) | Rust crate | Direct dependency of v2 |
| Gate system | eidolon-daemon routes/gate.rs | Moves into Agent Orchestrator |
| Living prompt templates | eidolon-daemon prompt/templates.rs | Enhanced with OpenSpace context |
| Engram integration | HTTP API calls | Same APIs, called by local LLM |
| Config structure | TOML config | Extended with LLM and TUI settings |

### What Changes

| v1 | v2 |
|----|-----|
| HTTP daemon on Rocky | Local TUI application on Windows |
| Task submission API (POST /task) | Conversational interface |
| eidolon-cli (separate binary) | TUI is the interface |
| Remote-only (Rocky) | Local-first (Windows, RTX 5070) |
| Hardcoded agent routing | LLM-driven intent routing |
| No personality | Gojo personality layer |
| Flat Engram search | OpenSpace graph-aware retrieval |

### What's New

- Local LLM integration (llama-server sidecar)
- Gojo personality engine
- Intent routing with structured output
- OpenSpace graph intelligence integration
- Supervised agent delegation with streaming TUI
- Theme system with animations
- Fine-tuning data collection pipeline
- Context window management (manual compaction)
- Multi-agent Loom workflows with approval checkpoints

### Migration Path

v1 daemon on Rocky can continue running independently. v2 is a new application that shares the eidolon-lib crate and Engram APIs. No breaking changes to existing infrastructure. The daemon can optionally be kept as a headless agent orchestrator for server-side tasks, but v2 is the primary user-facing interface.

---

## Open Questions (Resolved)

These were discussed and resolved during design:

| Question | Resolution |
|----------|-----------|
| Ollama or llama.cpp? | llama.cpp (llama-server sidecar). No Ollama. |
| Web UI or TUI? | TUI only. No web apps. Local apps only. |
| Which GPU? | RTX 5070 (12GB VRAM) |
| Which model? | Qwen 3 14B Q4_K_M (~9GB VRAM) |
| Fine-tune now or later? | Later. Collect real data first, base model + system prompt for launch. |
| Production Engram or separate? | Separate dedicated instance (port 4203 on Hetzner) |
| Delegation style? | Supervised. User sees streaming output, Eidolon manages session. |
| Personality? | Satoru Gojo from Jujutsu Kaisen |
| Codex role? | First-class agent, not just simple tasks. User can override agent selection. |
