# OmniProxy Architecture (Core Phase)

## 1. Goals

- Runtime zero-dependency distribution: one binary, no Python/Java runtime.
- Fast and memory-efficient MITM core.
- Extensible request/response processing pipeline.
- Future-proof plugin model through WebAssembly.

## 2. Borrowed Ideas and Our Positioning

- Envoy/Pingora: filter-chain lifecycle and strict stage boundaries.
- mitmproxy: programmable hook model and protocol-aware interception.
- hudsucker/rustls: practical Rust-native HTTPS interception path.
- wasmtime: safe, sandboxed plugin execution.

OmniProxy combines these into a core-first architecture:

1. data ingest: socket accept + TLS interception bootstrap
2. protocol brain: HTTP(S)/WebSocket parsing and forwarding
3. filter chain: deterministic per-flow stage execution
4. wasm runtime: safe extension points

## 3. Modules

- `src/config.rs`
  - CLI/env parsing
  - runtime paths and listen address

- `src/cert/mod.rs`
  - root CA load-or-create
  - local CA persistence for stable trust chain

- `src/filter/`
  - `HttpFilter` trait
  - `FilterChain` orchestration
  - built-in filters:
    - request id
    - access log
    - wasm dispatch

- `src/plugins/mod.rs`
  - plugin discovery (`*.wasm`)
  - Wasmtime lifecycle (compile/instantiate/call)
  - request/response hook dispatch
  - timeout-isolated execution and fail-open plugin faults

- `src/proxy/mod.rs`
  - server bootstrapping
  - MITM authority wiring
  - handler integration with filter chain

- `src/api/mod.rs`
  - backend WebSocket event stream for decoupled clients

- `src/query/mod.rs`
  - declarative filter DSL parser/evaluator core

- `src/replay/mod.rs`
  - replay-domain model (`ReplayCandidate`)
  - persisted flow loading and request-response correlation
  - shared base for CLI/TUI/API replay workflows

- `src/platform/system_proxy.rs`
  - platform adapter boundary for system-proxy control
  - macOS implementation + Windows/Linux hint contracts
  - keeps cross-platform expansion isolated from core proxy engine

- `src/storage/mod.rs`
  - async JSONL flow persistence for replay/analytics foundations

## 4. Request Lifecycle

1. client request enters proxy
2. `FilterChain::handle_request` runs in order
3. if no filter short-circuits, request forwards upstream
4. response returns and `FilterChain::handle_response` runs in reverse order
5. response goes back to client

This gives deterministic behavior and predictable composition.

## 5. Wasm Hook Model (v1 baseline)

- Hook payload: JSON snapshot of request/response metadata.
- Mutating exports now supported for request/response.
- ABI contracts remain strict (`memory/alloc/dealloc`, pointer-length payload convention).

- per-plugin timeout execution (`--wasm-timeout-ms`)
- plugin failures are isolated and do not terminate proxy core

## 6. Backend/UI Decoupling

OmniProxy exposes a WebSocket event API (`--api-listen`, default `127.0.0.1:9091`).
Request/response metadata can be consumed by external TUI or Web UI clients.
Default runtime path for subscription is `ws://127.0.0.1:9091/ws`.

## 7. Next Milestones

1. native replay API + flow storage
2. transparent mode (TUN/eBPF) adapters
3. independent backend API for future Web UI/TUI shell
4. hardened wasm ABI with policy DSL
