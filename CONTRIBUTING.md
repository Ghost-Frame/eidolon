# Contributing

## Building

### Rust workspace

```bash
cd eidolon
cargo build --release --workspace
```

This builds all crates: `eidolon-lib`, `eidolon`, `eidolon-daemon`, `eidolon-cli`.

### C++ backend

Requires CMake 3.14+ and Eigen3.

```bash
cd cpp
mkdir -p build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
make -j4
```

## Running Tests

### Rust tests

```bash
cargo test --workspace
```

The Rust workspace has 27 tests across the neural substrate.

### C++ tests

```bash
cd cpp/build && ctest --output-on-failure
```

The C++ backend has 25 tests covering the same behavior.

### Integration tests

```bash
# Requires a live Engram instance and brain.db
node --experimental-strip-types tests/brain-openspace.test.ts
```

## Code Style

- Rust: run `cargo fmt` before committing. CI will reject unformatted code.
- C++: 4-space indent, follow the existing style in `cpp/src/`.
- TypeScript: 2-space indent, no semicolons.
- No em dashes anywhere in code, comments, or documentation.
- Commit messages: lowercase imperative (`fix: gate fails open on timeout`, not `Fixed gate`).

## Adding Gate Rules

Static rules live in `eidolon-daemon/src/routes/gate.rs` in `check_dangerous_patterns()`.
Dynamic rules come from the brain via Engram context queries in the same file.

When adding a static rule:
1. Add the pattern match in `check_dangerous_patterns()`
2. Return a clear, specific block message (not "not allowed". Say what to do instead)
3. Add a test case in the `#[cfg(test)]` block at the bottom

## Reporting Issues

This project is experimental and tied to a specific infrastructure setup. Issues and PRs are welcome but response time is not guaranteed.
