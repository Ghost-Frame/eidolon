# Post-Fix Audit Results

## Fix Verification

### Gap 1: Prompt Generation
- Status: PARTIALLY CLOSED
- Evidence: The prompt path now embeds the task text and queries the in-process brain instead of calling Engram `/search` (`eidolon-daemon/src/prompt/generator.rs:47-88`). The rendered prompt also exposes activation strength and contradiction output (`eidolon-daemon/src/prompt/templates.rs:4-34`).
- Issues found:
  - The output is still a fixed template, not a briefing synthesized by the oracle as required by `docs/phase3-guardian-design.md:102-118`. `build_living_prompt` renders hard-coded sections plus static tool/safety tables (`eidolon-daemon/src/prompt/templates.rs:36-115`).
  - Context gathering is still canned: one task query, one fixed infrastructure query, one fixed failure query (`eidolon-daemon/src/prompt/generator.rs:96-104`). It does not derive a richer constellation of relevant entities, recent Chiasm activity, or tool timing from the task as described in `docs/phase3-guardian-design.md:106-118`.
  - The prompt can be internally inconsistent because it performs three independent brain queries with separate lock windows; dream cycles or absorption can mutate the brain between them (`eidolon-daemon/src/prompt/generator.rs:97-104`, `eidolon-daemon/src/main.rs:119-139`, `eidolon-daemon/src/absorber.rs:64-66`).

### Gap 2: Action Gate
- Status: PARTIALLY CLOSED
- Evidence: `gate_check` now runs a generic brain-backed validator for Bash commands before the old SSH/systemctl helpers (`eidolon-daemon/src/routes/gate.rs:224-285`). That validator embeds the command text, queries the brain, and can emit `block` or `enrich` (`eidolon-daemon/src/routes/gate.rs:13-84`).
- Issues found:
  - This is still not the confidence-threshold design in `docs/phase3-guardian-design.md:78-91`. The logic is heuristic string matching over recalled memory text, not intent extraction plus calibrated confidence (`eidolon-daemon/src/routes/gate.rs:41-68`, `86-99`).
  - Any recalled memory above `0.6` activation causes an immediate `enrich` return (`eidolon-daemon/src/routes/gate.rs:60-68`, `262-283`). That short-circuits the more precise SSH and `systemctl` logic below, so a vague memory can suppress the exact server-port or restart-order enrichment this route already had (`eidolon-daemon/src/routes/gate.rs:287-334`).
  - The new brain path only runs for `tool_name == "Bash"` (`eidolon-daemon/src/routes/gate.rs:224-225`), so the gate is still not generalized across outbound action types the design calls out (`docs/phase3-guardian-design.md:71-76`).

### Gap 3: Session Absorption
- Status: PARTIALLY CLOSED
- Evidence: The absorber now builds `BrainMemory` values and calls `Brain::absorb_new()` for session summaries, blocked actions, and discovery lines (`eidolon-daemon/src/absorber.rs:17-67`, `115-206`).
- Issues found:
  - The new memories are not persisted to `brain_memories`. `Brain::absorb_new()` only persists newly created edges; it never inserts the absorbed memory row itself (`eidolon-lib/src/brain.rs:392-408`). On restart, `Brain::init()` reloads only what is in `brain_memories` (`eidolon-lib/src/persistence.rs:6-47`), so post-session learning is ephemeral.
  - The extraction step is still output scraping, not structured learning. It builds one generic summary plus keyword-matched lines (`eidolon-daemon/src/absorber.rs:101-206`) instead of updating infrastructure state, track records, and correction types as required by `docs/phase3-guardian-design.md:93-99`.
  - If `/embed` fails, the absorber silently skips brain learning and only logs a warning (`eidolon-daemon/src/absorber.rs:25-35`).

### Gap 4: Correction Tracking
- Status: PARTIALLY CLOSED
- Evidence: Static gate blocks now increment `session.corrections` (`eidolon-daemon/src/routes/gate.rs:205-216`), and brain-driven blocks do the same (`eidolon-daemon/src/routes/gate.rs:227-237`).
- Issues found:
  - The orchestrator still does not maintain any per-agent or per-task-type track record. Session data stores the raw `corrections` count, but there is no aggregation layer and no learned selection path (`eidolon-daemon/src/session.rs:46-61`, `239-257`).
  - Task submission still either accepts the requested agent or defaults to `claude-code`; there is no performance-based selection (`eidolon-daemon/src/routes/tasks.rs:39-54`), so the learning loop described in `docs/phase3-guardian-design.md:40-48` is still absent.

### Double-Absorption Bug
- Status: CLOSED
- Evidence: `run_claude_code()` no longer spawns `absorb_session()` on exit or timeout (`eidolon-daemon/src/agents/claude_code.rs:250-305`). `run_agent()` now owns final absorption in one place after the adapter returns or on adapter error (`eidolon-daemon/src/agents/registry.rs:32-80`).
- Issues found: I do not see a remaining duplicate-absorption path in the current daemon flow.

### Evolution Feedback
- Status: PARTIALLY CLOSED
- Evidence: The gate now records evolution feedback when brain memories contribute to a block or enrichment (`eidolon-daemon/src/routes/gate.rs:239-247`, `265-272`, `299-307`, `322-329`). The daemon also trains evolution after each dream cycle (`eidolon-daemon/src/main.rs:132-137`).
- Issues found:
  - All of this is behind the optional `evolution` feature (`eidolon-daemon/Cargo.toml:14-16`, `eidolon-lib/Cargo.toml:22-23`). In the default build, those paths compile out. The `cargo test -p eidolon-daemon --tests` attempt emitted unused-`memory_ids` warnings at `eidolon-daemon/src/routes/gate.rs:226`, `289`, and `316`, which is exactly what happens when the `#[cfg(feature = "evolution")]` blocks are absent.
  - Training still depends on the periodic dream timer (`eidolon-daemon/src/main.rs:117-139`), so gate feedback does not affect query behavior until a dream cycle runs.

## Regressions

### Lock Contention
- Prompt generation, gate validation, absorption, and dream cycles all serialize through the same `state.brain` mutex (`eidolon-daemon/src/prompt/generator.rs:65-66`, `eidolon-daemon/src/routes/gate.rs:37-38`, `eidolon-daemon/src/absorber.rs:64-66`, `eidolon-daemon/src/main.rs:124-137`). I do not see a deadlock path, but this creates a real latency bottleneck under concurrent sessions.

### Error Propagation
- `embed_text()` returns `None` on any `/embed` failure without preserving status or body (`eidolon-daemon/src/lib.rs:36-55`).
- Prompt generation degrades to empty sections and still claims the context reflects the brain's current understanding (`eidolon-daemon/src/prompt/generator.rs:58-61`, `eidolon-daemon/src/prompt/templates.rs:71-80`).
- Gate validation silently disappears when embedding fails, which means the route falls back to static checks or plain allow (`eidolon-daemon/src/routes/gate.rs:25-35`, `224-285`).
- Session absorption quietly skips brain learning on the same failure mode (`eidolon-daemon/src/absorber.rs:25-35`).

### Memory Growth
- `Brain::absorb_new()` always appends to `self.memories` if the generated ID is new (`eidolon-lib/src/brain.rs:360-409`). There is no per-session cap or deduplication beyond ID uniqueness.
- Dreaming may prune later, but only on the timer-driven dream loop (`eidolon-daemon/src/main.rs:117-139`). In a long-lived daemon, absorbed session summaries and discovery lines will accumulate in memory for the life of the process.
- Because those memories are not written back to `brain_memories`, the daemon gets the worst of both worlds: in-process growth during uptime, then total loss of the learned state after restart (`eidolon-lib/src/brain.rs:392-408`, `eidolon-lib/src/persistence.rs:6-47`, `76-120`).

### Consistency
- The living prompt is assembled from three separate brain queries (`eidolon-daemon/src/prompt/generator.rs:96-104`). If absorption or dreaming runs between them, the task, infrastructure, and failure sections can come from different effective brain states.

### Test Coverage
- I found no tests covering the new brain-backed prompt path, the new generic gate validator, or the new absorber path. The daemon tests still target static helpers such as `check_dangerous_patterns()` and `parse_ssh_target()` (`eidolon-daemon/tests/pentest_gate_bypass.rs`, `eidolon-daemon/tests/pentest_ssrf.rs`).
- I could not complete `cargo test -p eidolon-daemon --tests` or `cargo test -p eidolon-lib` in this sandbox because the MSVC linker `link.exe` is unavailable. That prevents a full execution-level verification run and makes the missing targeted tests more important.

## Remaining Gaps Re-evaluation

### Gap 5: Multi-agent orchestration
- Still open. The registry still only dispatches `claude-code` and fails everything else (`eidolon-daemon/src/agents/registry.rs:31-63`). Nothing in this fix changed the single-adapter reality described in the previous audit.

### Gap 6: Backend selection
- Still open. The daemon still constructs an in-process Rust `Brain` directly (`eidolon-daemon/src/main.rs:63-70`), and `BrainConfig` still has no backend selector (`eidolon-daemon/src/config.rs:19-39`). README claims about config-selectable Rust/C++ parity remain overstated (`README.md:100-107`).

### Partial 1: Ranked memories vs understood answers
- Still open. `Brain::query()` still returns ranked activated memories plus contradiction pairs (`eidolon-lib/src/brain.rs:200-358`). This fix makes the daemon consume that result more directly, but it does not add the Branch C synthesis layer described in `docs/design.md:131-141`.

### Partial 2: Dreaming behavior-coupling
- Slightly improved, still open. Evolution training now runs after dream cycles (`eidolon-daemon/src/main.rs:132-137`), but dreaming itself is still timer-based, not idle-aware or session-coupled (`eidolon-daemon/src/main.rs:117-139`, `docs/design.md:60-74`).

### Partial 3: Instincts and curation in runtime reasoning
- Improved, still open. Prompt generation, gate validation, and absorption now hit the in-process brain, so runtime behavior is more connected to the curated corpus and instincts than before. But the prompt is still template-driven, and the absorber now injects raw session summaries directly into the live brain without passing through the curation model described in `docs/design.md:145-168`.

### Architectural Issue 1: Two brains with no sync
- Improved, not solved. Core runtime decisions now consult the in-process brain, which is better than the old Engram `/search` dependency. But embeddings still depend on Engram `/embed`, the absorber still mirrors data into Engram `/store`, and the newly absorbed brain memories are not persisted back to `brain.db` (`eidolon-daemon/src/lib.rs:36-55`, `eidolon-daemon/src/absorber.rs:118-205`, `eidolon-lib/src/brain.rs:392-408`).

### Architectural Issue 2: Feedback loop stops at narration
- Improved, not solved. The daemon now records gate feedback and absorbs some session output into the live brain. But the default build does not enable evolution, absorbed memories vanish on restart, and agent selection still ignores historical outcomes.

## New Issues

### High: Generic brain enrichment bypasses the precise SSH and service guards
- `gate_check()` returns immediately on a generic brain `enrich` result (`eidolon-daemon/src/routes/gate.rs:262-283`) before it reaches the purpose-built SSH and `systemctl` enrichment paths (`eidolon-daemon/src/routes/gate.rs:287-334`).
- This is a real regression risk because the generic path has weaker semantics than the specialized ones.

### High: Absorbed learnings disappear on daemon restart
- The new absorber feeds summaries and discoveries into `Brain::absorb_new()` (`eidolon-daemon/src/absorber.rs:115-206`), but `Brain::absorb_new()` does not persist those new memory rows (`eidolon-lib/src/brain.rs:392-408`).
- That means the flagship "brain as source of truth" fix only exists in RAM.

### Medium: Evolution is presented as working, but default builds do not enable it
- The implementation is feature-gated (`eidolon-daemon/Cargo.toml:14-16`, `eidolon-lib/Cargo.toml:22-23`), and the default build warnings show those blocks are absent.
- README therefore remains too optimistic when it lists evolution as working (`README.md:281-283`).

### Medium: The prompt is less wrong than before, but still over-claims synthesis
- README still says queries "return a synthesized answer grounded in specific memories" (`README.md:26-33`), while the daemon still consumes `Brain::query()` as ranked activated memories plus a fixed template (`eidolon-lib/src/brain.rs:200-358`, `eidolon-daemon/src/prompt/templates.rs:36-115`).

### Medium: No targeted tests were added for any of the new brain-backed paths
- The changed surfaces are prompt generation, generic gate validation, absorption, and evolution wiring. None of them have dedicated tests in this branch.

## Overall Assessment

Eidolon is materially closer to its vision than it was before `7c231eb`. The daemon now uses the in-process brain for prompt recall, gate recall, and post-session absorption, and the double-absorption bug is genuinely fixed.

The branch does not fully close the original vision gaps. The living prompt is still template assembly instead of synthesis, the action gate uses brittle heuristics and now risks bypassing its better SSH/service checks, absorbed learning is not durable across restarts, and the orchestrator still does not learn how to choose agents. The single most important next step is to make post-session absorption durable by persisting new `BrainMemory` rows to `brain.db`, then fix the gate so the generic brain layer augments rather than short-circuits the specialized validators.
