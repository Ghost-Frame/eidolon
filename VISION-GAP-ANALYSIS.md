# Eidolon Vision Gap Analysis

## Executive Summary
Eidolon has a real neural substrate prototype, a real curation pipeline, and a real Claude Code wrapper, but the end-to-end Guardian behavior described in the design docs is not implemented. The biggest gap is architectural: the daemon, the prompt generator, the gate, and the absorber mostly operate on Engram search results, static rules, and output scraping instead of using the substrate as the live source of understanding.

## Critical Gaps

### Gap 1: Living prompt generation is retrieval plus template assembly, not a brain-written briefing
- **Vision says:** The Guardian prompt must be "a document the brain writes, not a template with variables", driven by brain pattern completion and activated context constellations (`docs/phase3-guardian-design.md:100-113`).
- **Reality:** The daemon prompt path does four plain Engram `/search` calls and feeds the results into a fixed markdown template. There is no brain query, no pattern completion, and no synthesis layer in this path (`eidolon-daemon/src/prompt/generator.rs:39-105`, `eidolon-daemon/src/prompt/templates.rs:13-220`).
- **Impact:** The most visible Guardian promise is missing. Sessions get a curated database dump, not a context document that demonstrates understanding.
- **Fix complexity:** medium

### Gap 2: The action gate is primarily regex and config matching, not brain-grounded validation
- **Vision says:** Outbound actions should be validated by the brain via pattern completion, with `allow`, `block + teach`, or `enrich`, plus confidence thresholds (`docs/phase3-guardian-design.md:68-91`).
- **Reality:** The gate first runs static string checks for destructive commands, reboot, seeding, and protected services (`eidolon-daemon/src/routes/gate.rs:116-256`). Brain lookups only happen for SSH and `systemctl`, and even there they only return enrichment when a top memory is strong enough (`eidolon-daemon/src/routes/gate.rs:305-417`). There is no confidence model, no brain-driven blocking path, and no generalized intent extraction.
- **Impact:** The Guardian is not using learned knowledge as the primary control layer. It is a rule gate with optional memory hints.
- **Fix complexity:** medium

### Gap 3: Session absorption does not feed learnings back into the brain
- **Vision says:** Session absorption should extract learnings, feed them into the brain as new memories, update track records, and update infrastructure understanding (`docs/phase3-guardian-design.md:93-99`).
- **Reality:** `absorb_session` stores a short session summary plus a few keyword-matched lines back into Engram `/store` (`eidolon-daemon/src/absorber.rs:48-141`). It never calls the daemon brain, never invokes `Brain::absorb_new`, and never uses the standalone `/brain/absorb` flow that exists elsewhere in the repo (`eidolon-lib/src/brain.rs:360-408`, `integration/routes.ts:70-88`, `integration/manager.ts:270-290`).
- **Impact:** The core feedback loop is broken. The Guardian can narrate a session after the fact, but it does not make the brain smarter from that session.
- **Fix complexity:** medium

### Gap 4: Agent track records and correction learning are effectively unimplemented
- **Vision says:** The orchestrator should choose agents based on historical performance and maintain success and correction rates per task type (`docs/phase3-guardian-design.md:40-48`, `docs/phase3-guardian-design.md:95-99`, `docs/phase3-guardian-design.md:267-275`).
- **Reality:** Sessions have a `corrections` field, but it is initialized to `0`, serialized, and persisted without any code that increments it during gate activity (`eidolon-daemon/src/session.rs:46-61`, `eidolon-daemon/src/session.rs:65-83`, `eidolon-daemon/src/session.rs:239-253`). Task submission simply picks the requested agent or defaults to `claude-code`; there is no performance-based selection (`eidolon-daemon/src/routes/tasks.rs:39-54`).
- **Impact:** The "gets smarter over time" claim is not true at the orchestrator level. Agent choice and correction statistics do not learn.
- **Fix complexity:** small

### Gap 5: Multi-agent orchestration is promised, but only Claude Code actually runs
- **Vision says:** The registry should support Claude Code, OpenCode, Synapse v2, Pi, and third-party agents with thin adapters (`docs/phase3-guardian-design.md:45-48`, `docs/phase3-guardian-design.md:236-259`).
- **Reality:** `run_agent` only handles `"claude-code"` and fails everything else as unknown (`eidolon-daemon/src/agents/registry.rs:31-63`). The example config only defines `[agents.claude-code]` (`config/config.example.toml:90-94`).
- **Impact:** The orchestrator is not really an orchestrator yet. It is a single-adapter wrapper with a registry-shaped config.
- **Fix complexity:** medium

### Gap 6: The daemon does not support the advertised Rust/C++ backend selection model
- **Vision says:** The neural substrate is a Rust or C++ binary using the same JSON-over-stdio protocol, and the backend is selectable by config (`README.md:100-101`, `docs/phase3-guardian-design.md:162-168`).
- **Reality:** The daemon hardcodes an in-process Rust `Brain` and initializes it directly from SQLite (`eidolon-daemon/src/main.rs:63-70`). `BrainConfig` has only `db_path`, `data_dir`, and `dream_interval_secs`; there is no backend selector (`eidolon-daemon/src/config.rs:19-39`). A backend-switching JSON-over-stdio manager exists only in the separate TypeScript integration layer (`integration/manager.ts:1-26`, `integration/manager.ts:89-170`).
- **Impact:** The README overstates what the shipped daemon can do. C++ parity exists as a standalone integration path, not as part of the Guardian service.
- **Fix complexity:** medium

## Partial Implementations

### Partial 1: The neural substrate has recall mechanics, but it still returns ranked memories instead of understood answers
- **Vision says:** Querying should be pattern completion, not retrieval, and Branch B/Branch C should either return understanding-grounded memory constellations or synthesized answers (`docs/design.md:52-54`, `docs/design.md:122-139`, `docs/design.md:191-203`).
- **Current state:** The substrate has a real Hopfield-style store, graph spread, contradiction resolution, decay, and optional evolution weighting (`eidolon-lib/src/substrate.rs:94-160`, `eidolon-lib/src/brain.rs:200-355`).
- **What's missing:** `Brain::query` does not call `HopfieldSubstrate::complete`; it seeds from `retrieve`, spreads over the graph, and returns top activated memories. There is no answer synthesis, no explanation generation, and no "that is wrong, here is why" output layer in the daemon (`eidolon-lib/src/brain.rs:221-355`, `eidolon-lib/src/substrate.rs:123-160`).
- **Fix complexity:** large

### Partial 2: Dreaming exists, but it is a bounded heuristic sweep, not continuous consolidation woven into the Guardian loop
- **Vision says:** Dreaming should be a continuous background consolidation process that replays, strengthens, resolves interference, and merges redundant patterns (`docs/design.md:60-62`).
- **Current state:** Dreaming does meaningful work: replay boosts, redundant merge, prune, edge discovery, and contradiction cleanup (`eidolon-lib/src/dreaming.rs:74-113`, `eidolon-lib/src/dreaming.rs:118-213`, `eidolon-lib/src/dreaming.rs:218-399`).
- **What's missing:** In the daemon, it runs on a fixed timer from `dream_interval_secs`, not as a broader idle-aware or behavior-coupled consolidation loop (`eidolon-daemon/src/main.rs:114-132`). It also does not feed Guardian-specific corrections, prompt outcomes, or session absorption outputs back into the dream cycle.
- **Fix complexity:** medium

### Partial 3: Instincts and curation exist, but they are isolated from the Guardian's runtime reasoning path
- **Vision says:** The system should ship with instincts and rely on a curated corpus as the substrate's clean food (`docs/design.md:68-78`, `docs/design.md:145-183`).
- **Current state:** There is a synthetic instincts corpus with ghost memories and contradiction examples (`eidolon-lib/src/instincts.rs:1-18`, `eidolon-lib/src/instincts.rs:126-220`), and there is a real curation pipeline that filters noise, deduplicates via SimHash, writes `brain.db`, and seeds edges (`integration/curate.ts:1-46`, `integration/curate.ts:115-290`).
- **What's missing:** The Guardian does not use these assets to generate its living prompt or gate decisions in a closed loop. Prompt generation still goes to Engram `/search`, and the absorber still writes raw summaries back to Engram instead of curated substrate ingestion (`eidolon-daemon/src/prompt/generator.rs:75-105`, `eidolon-daemon/src/absorber.rs:48-141`).
- **Fix complexity:** medium

## Working As Intended

- The substrate can absorb memories into a projected pattern space, seed graph edges, and run interference-aware recall over graph spread (`eidolon-lib/src/absorb.rs:23-83`, `eidolon-lib/src/brain.rs:200-355`).
- Dream-cycle mechanics are real, not stubbed. Replay, merge, prune, discovery, and contradiction cleanup all mutate substrate state (`eidolon-lib/src/dreaming.rs:74-113`, `eidolon-lib/src/dreaming.rs:216-399`).
- The curated corpus pipeline is real and useful. It filters noise, deduplicates, writes `brain.db`, and seeds contradiction/association edges from `memory_links` (`integration/curate.ts:150-290`).
- The Claude Code wrapper is real. It creates a session directory, injects `CLAUDE.md`, installs a pre-tool gate hook, streams output, and enforces timeouts (`eidolon-daemon/src/agents/claude_code.rs:97-317`).
- The separate TypeScript integration layer really can spawn either the Rust or C++ brain subprocess over JSON-over-stdio (`integration/manager.ts:18-26`, `integration/manager.ts:89-170`, `integration/manager.ts:253-290`).

## Architectural Issues

### 1. The system has two brains with no closed-loop synchronization
The daemon loads an in-process Rust `Brain` from `brain.db` (`eidolon-daemon/src/main.rs:63-70`), but prompt generation and absorption still talk to Engram HTTP endpoints (`eidolon-daemon/src/prompt/generator.rs:39-105`, `eidolon-daemon/src/absorber.rs:62-141`). That means the Guardian's runtime substrate, the prompt context, and the stored learnings can diverge.

### 2. The feedback loop stops at narration
Blocked actions and session outcomes are narrated into Engram, but not reabsorbed into the live substrate. Even the optional evolution path records feedback on gate use, but the daemon never calls a training step (`eidolon-daemon/src/routes/gate.rs:137-145`, `eidolon-daemon/src/routes/gate.rs:160-167`, `eidolon-lib/src/brain.rs:519-527`).

### 3. Session absorption is currently double-triggered
`run_claude_code` spawns `absorb_session` on timeout and normal exit (`eidolon-daemon/src/agents/claude_code.rs:277-282`, `eidolon-daemon/src/agents/claude_code.rs:307-311`), and `run_agent` calls `absorb_session` again after `run_claude_code` returns (`eidolon-daemon/src/agents/registry.rs:66-80`). That can duplicate summaries and discovery memories.

### 4. Important Guardian metrics exist as fields, not behavior
`corrections` and `engram_stores` are persisted session metadata (`eidolon-daemon/src/session.rs:57-61`, `eidolon-daemon/src/session.rs:239-253`), but only `engram_stores` is actively updated in the gate (`eidolon-daemon/src/routes/gate.rs:76-94`). The correction system has schema without instrumentation.

## Recommended Priority Order

1. Make the daemon brain the actual system of record for Guardian decisions. Prompt generation, gate learning, and absorption all need to query and update the same substrate.
2. Replace template-only prompt generation with a substrate-driven assembly path. At minimum, use brain query results instead of raw `/search`, then add synthesis on top.
3. Rebuild session absorption into structured learning. Extract concrete facts, corrections, and outcomes, then call substrate absorb directly instead of only writing summaries to Engram.
4. Convert the gate from regex-first to brain-first for supported action classes. Keep static kill switches for catastrophic commands, but route SSH, service, deploy, and git intent through substrate-backed validation.
5. Implement correction accounting and agent track records, then use them for default agent selection.
6. Remove duplicate absorption triggers and define one ownership path for session finalization.
7. Unify backend selection. Either bring the daemon onto the JSON-over-stdio backend abstraction or narrow the README/design claims so they match the shipped service.
