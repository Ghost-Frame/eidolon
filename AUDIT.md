# Eidolon TUI Security & Architecture Audit

**Instructions for Codex**: This is a read-only audit. Do NOT make edits. Analyze the `eidolon-tui` crate and return a structured report covering all sections below. Be specific -- cite file paths, line numbers, and code snippets for every finding.

---

## 1. Security Audit

### Process Spawning & Command Injection
- Audit how Claude and Codex CLI processes are spawned (`src/agents/claude.rs`, `src/agents/codex.rs`)
- Check if user input (task descriptions) can inject shell commands through the CLI args
- Verify that `Command::new()` is used safely -- no shell expansion, no `sh -c` wrapping
- Check if the `model` field passed via `--model` can be exploited
- Audit the working directory (`session_dir`, `working_dir`) for path traversal

### Credential Handling
- Check how the Engram API key is stored and transmitted (`src/config.rs`, `src/syntheos/engram.rs`)
- Is the API key logged anywhere? Check all logging/tracing paths
- Check if credentials are exposed in process arguments (visible via `ps aux`)
- Review the config file loading -- is it world-readable? Does it warn about permissions?

### LLM Sidecar Security
- Audit `src/llm/sidecar.rs` for process management safety
- Check the `taskkill /F /IM llama-server.exe /T` call -- could this kill unrelated processes?
- Verify the health check endpoint isn't spoofable (is localhost binding enforced?)
- Check if the sidecar stderr log file has safe permissions
- Review the 120s polling loop -- could it be exploited for denial of service?

### Input Handling
- Audit the TUI input bar for buffer overflow or panic on malformed input
- Check if slash commands sanitize their arguments
- Review the `/model` command -- can it set arbitrary values that break things downstream?
- Check grammar-constrained routing for injection via user message content

### Network Requests
- Audit all outbound HTTP requests (Engram, LLM client)
- Check for SSRF if any user input flows into URLs
- Verify TLS is used for Engram communication (or document when it's not)

---

## 2. Architecture Review

### Event Loop & Concurrency (`src/main.rs`)
- Audit the main event loop for race conditions between:
  - Sidecar startup result
  - Health check polling
  - Routing decision arrival
  - Token streaming
  - User input events
- Check if `stream_abort` properly cancels the spawned task and cleans up resources
- Review the speculative execution pattern: what happens if routing completes AFTER the casual stream finishes?
- Check for deadlock potential in the `oneshot::channel` usage
- Verify that `AbortHandle::abort()` doesn't leave the LLM client in a broken state

### Agent Orchestrator (`src/agents/`)
- Review the session lifecycle: spawn -> stream -> cleanup
- Check if child processes are properly reaped on session end
- What happens if Claude/Codex process hangs indefinitely?
- Audit the stdout/stderr streaming -- can backpressure from a slow consumer cause issues?
- Check if `UnboundedSender` can cause unbounded memory growth with fast-producing agents

### LLM Client (`src/llm/client.rs`)
- Audit the streaming SSE parser for correctness
- Check error handling on malformed SSE responses
- Review connection timeout and retry behavior
- What happens if the LLM server returns a 500 during streaming?

### Router (`src/conversation/router.rs`)
- Audit the GBNF grammar for completeness -- can it produce invalid JSON?
- Check the `from_json` parser for panics on malformed input
- Review the complexity-to-model mapping -- what if the config has empty model strings?
- What happens if the routing LLM call times out?

### Configuration (`src/config.rs`)
- Check for panics on missing or malformed config values
- Review defaults -- are they safe and sensible?
- What happens if the config file doesn't exist?
- Check if `model_light`, `model_medium`, `model_heavy` fields are validated

### Personality & System Prompt (`src/conversation/personality.rs`)
- Review the system prompt for prompt injection vectors
- Check if conversation history can be used to override system prompt instructions
- Verify the prompt doesn't leak internal system details

---

## 3. Correctness & Reliability

### State Machine
- Map out all `AppMode` transitions and verify they're all reachable and all have exit paths
- Check for states that can get stuck (e.g., `Routing` with no timeout, `AwaitingConfirmation` with no escape)
- What happens if the user sends input while in `Routing` or `Generating` mode?
- Verify the `commit_pending_response` function handles all edge cases

### Dataset Collector (`src/dataset/collector.rs`)
- Check if the JSONL file is flushed correctly
- What happens if the data directory doesn't exist?
- Is there a size limit or rotation on the training data file?
- Check for data loss on crash (unflushed writes)

### Terminal Management (`src/tui/terminal.rs`)
- Verify raw mode is always restored on exit (including panics)
- Check the `KeyEventKind::Press` filter -- does it correctly handle all platforms?
- What happens if the terminal is resized to very small dimensions?

### Error Handling
- Search for `unwrap()` calls that could panic in production
- Check for `.expect()` messages that would be confusing to users
- Review `map_err` chains for error message quality
- Check if errors from child processes are properly propagated to the UI

---

## 4. Improvement Opportunities

For each finding, rate priority as **critical**, **high**, **medium**, or **low** and explain why.

Focus on:
- Things that could cause crashes or hangs
- Things that could cause security issues
- Things that produce silent incorrect behavior
- Things that make debugging significantly harder

Do NOT suggest:
- Style changes, formatting, or clippy lints
- Adding comments or documentation
- Refactoring that doesn't fix a concrete problem
- Adding features that don't exist yet

---

## Output Format

Structure your report as:

```
## [Section Name]

### [Finding Title]
- **Severity**: critical / high / medium / low
- **File**: path/to/file.rs:line_number
- **Description**: What the issue is
- **Evidence**: Code snippet or trace showing the problem
- **Recommendation**: Specific fix (but do NOT implement it)
```
