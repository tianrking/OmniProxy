# OmniProxy Roadmap

## Scope Summary

OmniProxy target capabilities:

1. Zero-dependency, cross-platform binaries (`Linux/Windows/macOS`)
2. High-performance HTTP(S)/WebSocket MITM core
3. Wasm sandbox plugin runtime with crash isolation
4. Envoy-like filter chain and policy engine
5. Geek-first UX: keyboard TUI and decoupled backend API
6. Flow storage, replay, and security analysis workflows

## Iteration Plan

Estimated total: **10 major iterations** from current baseline.

### I1 (done)
- Core MITM skeleton
- Filter-chain framework
- Wasm host (v0)

### I2 (done)
- Backend WebSocket event API
- First TUI (list/detail/filter input)

### I3 (done)
- Async JSONL flow persistence

### I4 (in progress)
- Replay engine v1 (replay by flow id, method override, target override)
- CLI commands for replay
  - status: `omni-replay` list + replay-by-index 已落地，后续补 body/header 重放

### I5
- Rule router v1 (host/path/method matching)
- Request/response mutation actions (headers/status/body-lite)

### I6
- Wasm ABI v1 (mutating hooks + deny/allow actions)
- Plugin timeout/resource policy hardening

### I7
- TUI v2 (tabs, focus model, live filter panel, replay controls)
- Improved keyboard ergonomics and search

### I8
- TLS/CA UX hardening (install helpers, trust diagnostics)
- Better platform compatibility validation (`linux/windows/macos`)

### I9
- Security analyst features (match/replace presets, payload templates, audit logs)
- Import/export flow bundles

### I10
- Stabilization release: benchmark suite, docs polish, release automation, signed artifacts

## Risks and Cost Drivers

- Cross-compiling ARM targets with native TLS stacks can require linker/toolchain tuning.
- Wasm mutation ABI design affects long-term plugin compatibility.
- Replay correctness for complex streaming/WebSocket flows needs multiple refinement cycles.

## Delivery Strategy

- One feature set per version, always commit + push.
- Keep each iteration buildable and CI-green before moving forward.
- Prioritize core correctness and extensibility over UI polish.
