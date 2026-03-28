# Eidolon Phase 3: The Guardian -- Design Spec

> Eidolon becomes the orchestrator, wrapper, and living prompt generator for all AI agents. Agents no longer run unsupervised. Eidolon spawns them, briefs them, guards them, teaches them, and absorbs their experience.

**Date:** 2026-03-27
**Status:** Design approved
**Depends on:** Eidolon Phase 1 (neural substrate) + Phase 2 (oracle, dreaming, instincts, evolution)
**Location:** Eidolon service on Rocky (primary), deployable anywhere Engram runs

---

## The Problem

Agents are stupid. Not because the models are bad, but because every session starts from zero. An agent with access to Engram still SSH's to the wrong server six times because it doesn't understand the information it retrieves. Agents don't use available tools (Soma, Chiasm, Broca) because they don't know they exist. The same conversation happens three times in one day because no session knows what previous sessions did.

Storing information in Engram doesn't help if agents can't use it. Exposing MCP tools doesn't help if agents don't know when to call them. Writing detailed AGENTS.md files doesn't help if agents ignore the instructions.

The root cause: agents run unsupervised with incomplete knowledge and no accountability.

---

## The Solution

Agents don't run unsupervised anymore. Eidolon runs them.

Eidolon is a persistent service with three layers:

1. **Orchestrator** -- receives tasks, picks (or is told) which agent to use, manages session lifecycle
2. **Wrapper** -- spawns agents inside a controlled environment, intercepts every action, blocks mistakes, teaches corrections
3. **Living Prompt** -- generates a dynamic, tailored system prompt for each session from the brain's understanding of the task

---

## Architecture

### Layer 1: Orchestrator

The orchestrator is the entry point. All tasks come through it, regardless of which client (CLI, Telegram, Discord, egui) sends them.

**Task intake:**
- Receives: task description + optional agent preference + optional context
- If agent not specified: Eidolon picks based on task analysis and historical performance. User can override at any time.
- Agent overrides are feedback -- if user keeps overriding Sonnet to Opus for infra tasks, Eidolon learns to stop picking Sonnet for those.

**Agent registry:**
- Available agents: Claude Code (Opus/Sonnet/Haiku), OpenCode, Synapse v2 (Ion/ionsh), Pi (future)
- Each agent has: spawn command, capabilities, supported models, track record (success rate, correction rate per task type)
- Third-party agents (Gemini, GPT) can be registered by other users

**Session management:**
- Tracks all active sessions: which agent, what task, how long, current state
- Handles completion: absorbs session learnings into the brain
- Handles failure: logs what went wrong, feeds back into brain for future avoidance
- Can kill a session if it's going off the rails

### Layer 2: Wrapper

Every agent runs inside a wrapper. The wrapper controls the agent's entire lifecycle.

**Spawning:**
1. Eidolon generates the living prompt (Layer 3)
2. Wrapper spawns the agent process with the prompt injected
3. For Claude Code: starts with custom CLAUDE.md containing the living prompt
4. For OpenCode: injects via AGENTS.md
5. For Synapse v2: passes context through Ion's session init
6. Each agent harness gets a thin adapter that knows how to inject context and intercept actions

**Action gate:**
Every outbound action passes through the gate before execution:

- SSH commands: check target host, port, user, key against brain knowledge
- File operations on remote servers: check paths exist, permissions are right
- Service management: check restart ordering, safety constraints
- Git operations: check remote, branch, destructive flags
- API calls: check endpoints, auth, correct service
- Deployment actions: check target, method, file layout

**Check flow:**
1. Agent wants to execute action
2. Wrapper extracts intent from the action
3. Brain does pattern completion: given this intent, what does Eidolon know?
4. Three outcomes:
   - **Allow** -- brain confirms the action makes sense. Proceed.
   - **Block + teach** -- brain detects a problem. Stop the action. Return: what's wrong, what the correct action is, why. Absorb the correction into the brain.
   - **Enrich** -- action is correct but incomplete. Brain adds missing context (right SSH key, right port, etc).

**Confidence threshold:**
- High confidence (>0.8) that action is wrong: block
- Medium confidence (0.4-0.8): warn but allow
- Low confidence (<0.4): allow silently
- Brain doesn't know anything about this action: allow (don't block what you don't understand)

**Session absorption:**
When a session ends:
1. Extract key learnings: what was done, what was discovered, what went wrong, what was corrected
2. Feed all learnings into the brain as new memories
3. Update agent track record (did it need corrections? how many? what types?)
4. Update the brain's understanding of infrastructure state if anything changed

### Layer 3: Living Prompt Generator

The living prompt is a document the brain writes, not a template with variables.

**Assembly pipeline:**

1. **Task analysis** -- the brain pattern-completes on the task description. What entities, servers, services, history, and constraints are relevant?

2. **Context gathering** from the activated constellation:
   - Current state of relevant systems (IPs, paths, deployment methods, what's running where)
   - Recent changes and migrations
   - Active constraints and safety rules
   - Known agent mistakes on similar tasks
   - Available tools and when to use each one (Soma, Chiasm, Broca, Engram MCP tools)
   - What other agents have been working on recently (from Chiasm)

3. **Prompt synthesis** -- the oracle (Branch C) generates a coherent briefing document from the gathered context. Reads like a briefing from someone who knows, not a dump of database rows.

4. **Ongoing updates** -- as the session progresses, the wrapper monitors what the agent is doing. If the agent shifts to a new topic, Eidolon pushes a context update with relevant knowledge for the new direction.

**Example generated prompt:**

```
You are working on: Deploy Engram to Hetzner

## Current State
- Engram production: production (10.0.0.2:4200), SSH as deploy, key ~/.ssh/id_ed25519
- Deployed via SCP from Rocky staging, NOT a git repo on Hetzner
- Files at ~/engram/ on production
- Last deployment: 2026-03-25 (GUI rebuild + nerve center API key fix)
- Rocky (127.0.0.1) is staging/backup, NOT production

## How to Deploy
- Build/test on Rocky first
- SCP files to production: scp -i ~/.ssh/id_ed25519 [files] deploy@10.0.0.2:~/engram/
- Restart process on production after deploy

## Constraints
- SSH key: ~/.ssh/id_ed25519 (always, all servers)
- DO NOT reboot OVH VPS (LUKS vault locks)
- CrowdSec everywhere, NEVER fail2ban
- DO NOT assign passwords, ask the operator

## Your Tools
- Register with Chiasm on session start (POST /chiasm/tasks)
- Log significant actions to Broca (POST /broca/log)
- Store discoveries to Engram (POST /store)
- Query Engram before guessing at ANYTHING

## Recent Issues on Similar Tasks
- Previous agents repeatedly SSH'd to wrong IPs (10.0.0.1, 10.0.0.1).
  The correct IP is 10.0.0.2.
- Previous agents assumed Engram is a git repo on Hetzner. It is not.
- Previous agents didn't know how Engram starts on production. Check Engram for the start command.
```

---

## Eidolon Service Architecture

Eidolon runs as a persistent daemon with the following components:

### Core
- Neural substrate (Rust or C++ binary, same as Phase 1/2)
- Oracle (LLM synthesis via Engram's callLLM)
- Curated corpus (brain.db)
- Dreaming (background consolidation during idle)
- Instincts (base weights for new instances)
- Evolution (learned weights from feedback)

### Service Layer
- **Task API** -- REST + WebSocket endpoint for all clients
  - POST /task -- submit a task (with optional agent, model preferences)
  - GET /task/:id -- task status, progress
  - WS /task/:id/stream -- live output streaming
  - POST /task/:id/override -- change agent/model mid-session
  - POST /task/:id/kill -- terminate session
  - GET /sessions -- list active sessions
  - GET /brain/stats -- brain health, dream stats, evolution stats

- **Agent spawner** -- manages agent processes
  - Spawns agents with injected living prompts
  - Routes agent stdio through the action gate
  - Handles agent crash/timeout

- **Action gate** -- the guardian
  - Parses outbound actions from agent output
  - Queries brain for validation
  - Returns allow/block/enrich decisions
  - Logs all decisions for learning

- **Prompt engine** -- generates living prompts
  - Queries brain for task-relevant context
  - Uses oracle to synthesize briefing documents
  - Pushes live updates during sessions

- **Absorber** -- post-session learning
  - Extracts learnings from completed sessions
  - Feeds back into brain
  - Updates agent track records

### Client Adapters (all thin)

**CLI:**
- `eidolon "task description"` -- default agent
- `eidolon --agent claude-opus "task description"` -- specific agent
- `eidolon --agent opencode --model copilot "task description"` -- agent + model
- Interactive mode: shows live agent output, allows intervention
- `eidolon status` -- list active sessions
- `eidolon brain` -- brain stats

**Telegram bot:**
- `/task deploy Engram to Hetzner` -- default agent
- `/opus deploy Engram to Hetzner` -- specific model
- Sends progress updates as messages
- Reply to override or provide input
- `/status` -- active sessions
- `/brain` -- brain summary

**Discord bot:**
- Same command pattern as Telegram
- Thread per task for clean conversation
- Role-based access if other people use it

**egui desktop app:**
- Dashboard: active sessions, brain state, recent corrections
- Task submission with agent/model picker
- Live session viewer (agent output streaming)
- Brain visualizer (activation patterns, connection graph)
- Correction history and evolution progress
- Session replay (see what an agent did and what Eidolon corrected)

---

## Agent Adapters

Each agent type needs a thin adapter that knows how to:
1. Inject the living prompt
2. Intercept outbound actions
3. Capture session output for absorption

### Claude Code Adapter
- Inject prompt via: write dynamic CLAUDE.md to a temp project dir, run claude with --project flag pointing there
- Intercept actions via: Claude Code hooks (PreToolUse for Bash, SSH, etc.)
- Capture output: parse claude's stdout/stderr

### OpenCode Adapter
- Inject prompt via: write dynamic AGENTS.md to project dir
- Intercept actions via: hook into OpenCode's tool execution (may need OpenCode plugin or wrapper)
- Capture output: parse stdout

### Synapse v2 (Ion/ionsh) Adapter
- Inject prompt via: Ion's session context API
- Intercept actions via: Ion's action pipeline
- Capture output: Ion's session log

### Generic Adapter (for others)
- Inject prompt via: environment variable or stdin preamble
- Intercept actions via: LD_PRELOAD or ptrace-based syscall interception (aggressive) or command wrapper scripts (simple)
- Capture output: stdout/stderr

---

## The Feedback Loop

Every interaction makes Eidolon smarter:

1. **Agent mistakes** -- blocked actions become strong training signals. The brain learns "SSH to 10.0.0.1 for Engram is wrong" with high confidence.

2. **User overrides** -- when you switch from Sonnet to Opus, Eidolon learns task-type-to-model preferences.

3. **Session outcomes** -- did the task succeed? Did it need corrections? How many? This builds agent track records.

4. **Repeated questions** -- if agents keep asking about the same thing across sessions, Eidolon elevates that knowledge to the living prompt template. It becomes part of every session's briefing.

5. **Corrections propagate** -- a correction in one session immediately affects all future sessions. The brain learns once, all agents benefit.

---

## Implementation Language

- **Eidolon service core:** Rust (same binary as the neural substrate, extended with the service layer)
- **Client adapters:** TypeScript for Claude Code/OpenCode hooks, Rust for CLI, whatever each bot framework needs for Telegram/Discord
- **egui app:** Rust (native desktop, talks to Eidolon API)
- **No Python anywhere**

---

## What Success Looks Like

- You message the Telegram bot "deploy Engram to Hetzner." Agent completes it first try. No wrong IPs. No wrong paths. No confused fumbling.
- A new agent session about Engram already knows the full deployment history, current state, and every mistake previous agents made.
- You never have the same conversation twice. Eidolon remembers and teaches.
- Agents use Soma, Chiasm, Broca without being told, because the living prompt tells them how and when.
- The brain gets noticeably smarter over weeks. Corrections decrease. Sessions get faster. Agents need less hand-holding.
- You open the egui app and see your entire infrastructure's state as Eidolon understands it -- a living, evolving map of everything you've built.
