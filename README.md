# Eidolon

The living brain for [Engram](https://codeberg.org/GhostFrame/engram) -- a from-scratch neural substrate that learns from memories instead of searching them.

## Status

Experimental. Phase 1 complete. Not production-ready.

## What This Is

A neural substrate that absorbs memories as activation patterns in a high-dimensional space. It forms associations, resolves contradictions through interference, decays unused patterns, and completes partial queries by activating the strongest connected constellation.

Two parallel implementations:
- **Rust** (ndarray + serde_json) -- 27 tests, 0.6ms avg query
- **C++** (Eigen3 + nlohmann/json) -- 25 tests, 0.7ms avg query

Both speak identical JSON-over-stdio protocol. Engram selects via config flag.

## Building

### Rust
```bash
cd rust && cargo build --release
```

### C++
```bash
cd cpp && mkdir -p build && cd build && cmake .. -DCMAKE_BUILD_TYPE=Release && make -j4
```

## How It Works

See [docs/design.md](docs/design.md) for the full design spec.

## License

Elastic License 2.0 -- same as Engram.
