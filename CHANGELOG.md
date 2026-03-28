# Changelog

## v0.3.0 (2026-03-27)

Phase 3: The Guardian

- Guardian daemon (`eidolon-daemon`) implemented in Rust with axum
- HTTP API at `:7700`: `/tasks`, `/sessions`, `/gate/check`, `/brain/query`, `/prompt/living`
- Living prompt generator: pulls Engram context via 4 parallel searches, synthesizes dynamic system prompts per session
- Action gate: intercepts every agent tool call before execution. Checks SSH targets, destructive commands, OVH reboot protection, demo data seeding, force pushes to protected branches
- Gate hook script (`scripts/eidolon-gate.sh`) for Claude Code `.claude/settings.json` integration. Fails open on daemon unreachable.
- Agent registry with claude-code adapter. Spawns agents with injected living prompts.
- Session absorber: extracts learnings from completed sessions and writes them back to the brain
- Gate benchmarked at less than 5ms per check on live sessions

## v0.2.0 (2026-03-27)

Phase 2: Oracle, Dreaming, Instincts, Evolution

- Oracle: LLM-powered answer synthesis grounded in neural recall. Cites source memories. Corrects stale assumptions.
- Dreaming: offline consolidation process. Replays patterns, strengthens high-value connections, resolves interference. ~60ms per cycle.
- Instincts: synthetic pre-training for new instances. Ships with wiring for quality signal recognition, temporal preference (newer supersedes older), association formation.
- Evolution: feedback loop that reshapes connection weights based on corrections and confirmations. Brain adjusts what it emphasizes over time.
- Hallucination detection in Oracle: verifies synthesized claims against recalled memories before returning answers.
- Memory curation pipeline: keeps the brain clean as contradictory or redundant memories accumulate.

## v0.1.0 (2026-03-27)

Phase 1: Neural Substrate

- Hopfield-based associative memory store. Memories encoded as 1024-dimensional activation patterns.
- Weighted activation graph: 6632 edges across 1628 patterns. Associations strengthen with co-occurrence, decay with neglect.
- Interference resolution: contradictory patterns compete. Stronger patterns suppress weaker ones without deletion.
- Natural decay: unused connection weights decrease over time. Stale patterns become unreachable.
- Two parallel implementations:
  - Rust (ndarray + serde_json): 0.6ms avg query, 24.1MB RAM, 910ms init, 27 tests
  - C++ (Eigen3 + nlohmann/json): 0.7ms avg query, 25.6MB RAM, 963ms init, 25 tests
- Both speak identical JSON-over-stdio protocol. Engram selects via config flag.
- Sub-millisecond queries over 1628 patterns on commodity hardware.
