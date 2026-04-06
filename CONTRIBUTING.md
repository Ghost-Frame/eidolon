# Contributing to Eidolon

Guidelines for contributing to the neural brain and guardian daemon.

## Development Setup

```bash
git clone https://codeberg.org/GhostFrame/eidolon.git
cd eidolon

# Enable pre-commit hook (blocks private info leaks)
git config core.hooksPath .githooks

# Build all workspace members
cargo build --release --workspace
```

**Requirements:**
- Rust 1.75+
- [Engram](https://codeberg.org/GhostFrame/engram) running and accessible (the daemon talks to it)

## Architecture

Eidolon is a Rust workspace with five crates:

```
eidolon-lib/          Neural substrate (Hopfield store, activation graph, decay, dreaming, evolution)
eidolon/              Brain binary (standalone neural operations, diagnostics)
eidolon-daemon/       Guardian daemon (HTTP API at :7700, gate, prompt generation, agent orchestration)
  src/
    agents/           Agent registry and adapters (claude-code)
    prompt/           Living prompt generator and templates
    routes/           HTTP routes (activity, gate, brain, sessions, tasks, audit, growth)
    absorber.rs       Session absorption back into brain
    session.rs        Session lifecycle management
  tests/              Security pentest suite (72 tests)
eidolon-tui/          Terminal UI with local LLM sidecar + daemon integration
eidolon-cli/          CLI client for task submission and status queries
```

### Key Design Decisions

1. **Hopfield networks, not vector search.** Memories are activation patterns in a neural space. Pattern completion replaces ranked document retrieval. Conflicting patterns compete and the stronger one wins.

2. **The daemon is the core.** Every agent interaction goes through the daemon's HTTP API. The TUI is one frontend. Cloud agents (Claude Code, Cursor, etc.) get the same intelligence layer via hooks.

3. **Fail-open gate.** The action gate blocks dangerous operations, but if the daemon is unreachable, commands proceed normally. A dead gate is better than a dead agent.

4. **Activity fan-out.** Agents report to one endpoint (`POST /activity`). The daemon distributes to Chiasm, Axon, Broca, Engram, Soma, Thymus, and the neural brain. All fan-out is best-effort.

5. **Session absorption.** When an agent session ends, the daemon absorbs learnings back into the brain as new activation patterns.

6. **Growth system.** Post-dream reflection via Together.ai. After each dream cycle there is a configurable probability (default 20%) of calling `/growth/reflect`. Observations are stored in Engram under `category=growth` and injected into living prompts via `/growth/materialize`. The API key is loaded from credd (`together/api-key`) -- never from config.

## Testing

The pentest suite covers gate bypass, command obfuscation, injection, auth, secrets, and SSRF:

```bash
cargo test --workspace
```

72 tests across 6 files. Focus areas for new tests:

| Feature | Location |
|---------|----------|
| Gate bypass via obfuscation | `eidolon-daemon/tests/pentest_gate_bypass.rs` |
| Prompt injection resistance | `eidolon-daemon/tests/pentest_injection.rs` |
| Auth edge cases | `eidolon-daemon/tests/pentest_auth.rs` |
| Brain pattern completion | `eidolon-lib/` (needs coverage) |
| Dreaming consolidation | `eidolon-lib/` (needs coverage) |
| Growth reflection logic | `eidolon-lib/src/growth.rs` (needs coverage) |
| TUI daemon integration | `eidolon-tui/` (needs coverage) |

## Code Style

- Rust 2021 edition
- `cargo fmt` and `cargo clippy` before committing
- Release profile: opt-level 3, thin LTO, stripped binaries
- Error handling: `anyhow` for applications, `thiserror` for libraries
- Async runtime: tokio with axum for HTTP

## Pull Request Process

1. Fork the repo and create a feature branch
2. Run `cargo test --workspace` and `cargo clippy --workspace`
3. Ensure the pre-commit hook passes (no private infrastructure details)
4. Submit a PR with a clear description of what changed and why

## Areas Where Help Is Needed

- **Brain test coverage**: The neural substrate (`eidolon-lib`) needs unit tests for pattern completion, interference resolution, and dreaming cycles
- **Gate hardening**: The gate uses pattern matching which has known bypass gaps (base64 encoding, variable expansion). Shell-aware parsing would close these.
- **Multi-agent orchestration**: The daemon currently supports Claude Code. Adapters for other agents (Cursor, OpenCode, Codex) need building.
- **Instinct training data**: The `data/` directory contains pre-training instincts. More domain-specific instinct sets would help new deployments.
- **Growth system tests**: `eidolon-lib/src/growth.rs` has no unit tests. Coverage for `validate_observation`, `should_reflect`, and `build_dream_context` would be straightforward to add.
- **Growth prompt tuning**: Built-in system prompts in `get_system_prompt` cover `eidolon`, `claude-code`, `engram`, `chiasm`, and `thymus`. New service adapters need entries here.

## License

Elastic License 2.0 (ELv2). See [LICENSE](LICENSE) for details.
