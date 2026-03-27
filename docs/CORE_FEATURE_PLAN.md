# OmniProxy Core Feature Master Plan

This is the canonical implementation checklist for reaching full core capability parity (and beyond) for modern API/security proxy workflows.

Status legend:
- `DONE`: implemented and integrated.
- `IN_PROGRESS`: partially implemented, still missing key acceptance criteria.
- `TODO`: not implemented yet.

## Release Cadence (Authoritative)

1. Planning unit:
- Major iteration = one user-visible capability slice (R1..R8).
- Minor iteration = concrete commit set inside one major iteration.

2. Current estimate:
- Remaining major iterations: **7**
- Expected minor commits per major iteration: **2 to 4**
- Expected remaining commits to v1.0 scope: **14 to 28**

3. Progress policy:
- Every merged feature commit must update:
  - this file status (`DONE/IN_PROGRESS/TODO`)
  - `docs/ROADMAP.md` remaining iteration count
  - Chinese doc section impacted by the feature

## A. MITM Core (HTTP/HTTPS/WebSocket)

1. Explicit HTTP proxy + HTTPS CONNECT MITM: `DONE`
2. Dynamic local CA generation and certificate cache: `DONE`
3. HTTP/1.1 correctness under keep-alive and pipelining: `IN_PROGRESS`
4. HTTP/2 correctness and multiplex stream handling: `IN_PROGRESS`
5. WebSocket transparent interception: `DONE`
6. WebSocket frame-level observability (text/binary/ping/pong/close): `DONE`
7. WebSocket frame mutation pipeline: `DONE`
8. Large payload streaming with bounded memory strategy: `TODO`
9. Backpressure policy for slow downstream sinks: `TODO`
10. Deterministic request/response correlation under concurrency: `IN_PROGRESS`

## B. Flow Capture and Storage

1. Real-time event bus (ws API): `DONE`
2. Persistent flow logging (JSONL): `DONE`
3. Request/response headers capture: `DONE`
4. Request/response body capture (binary-safe): `IN_PROGRESS`
5. Truncation/sampling policy for huge payloads: `IN_PROGRESS`
6. Compression-aware storage strategy: `TODO`
7. Flow retention and rotation policy: `IN_PROGRESS`

## C. Replay Engine

1. Replay by index/request-id: `DONE`
2. Method/url/header override: `DONE`
3. Dry-run and cURL export: `DONE`
4. Request body replay: `DONE`
5. Replay diff report (status/header/body hash): `DONE`
6. Stateful session replay helpers: `TODO`

## D. Rule Engine and Traffic Mutation

1. DSL parser (`==`, `~=`, `>=`, `<=`, `&&`, `||`): `DONE`
2. Deny action: `DONE`
3. Request header mutation: `DONE`
4. Response header mutation: `DONE`
5. Response status override: `DONE`
6. Response body replacement: `DONE`
7. Rule lint / validate command: `DONE` (`--check-rules`)
8. Advanced operators (`starts_with`, regex-lite): `DONE`
9. Rule precedence/conflict strategy: `DONE`

## E. Plugin Runtime (Wasm)

1. Plugin discovery and loading: `DONE`
2. Isolated execution and timeout: `DONE`
3. Fail-open behavior: `DONE`
4. Stable mutating ABI v1 (req/res edit actions): `TODO`
5. Resource budgets (CPU/memory/fault counters): `TODO`
6. Versioned ABI compatibility tests: `TODO`

## F. UX and Operations

1. Keyboard-first TUI baseline: `DONE`
2. Live query filtering: `DONE`
3. Replay from TUI selected flow: `TODO`
4. TLS trust diagnostics command: `IN_PROGRESS`
5. One-command local bootstrap workflow: `DONE`
6. Web API contract documentation: `IN_PROGRESS`

## G. Delivery and Platform

1. CI: Linux amd64/arm64/arm32 build: `DONE`
2. CI: Windows x64 build: `DONE`
3. CI: macOS x64/arm64 build: `DONE`
4. CI: `.deb` packaging: `DONE`
5. CI: fmt/check/test + rules preflight gate: `DONE`
6. Performance benchmark suite (latency/memory/concurrency): `IN_PROGRESS`
7. Signed release artifacts and provenance: `TODO`

## Iteration Execution Plan (Remaining)

1. `R2`: Request/response body capture with bounded memory and truncation policy.
2. `R3`: Replay v2 with body replay and diff report.
3. `R4`: Rule engine v2 operators + precedence model.
4. `R5`: Wasm mutating ABI v1 and hostcall contract.
5. `R6`: TLS trust diagnostics and bootstrap UX.
6. `R7`: Performance benchmarks and memory pressure tests.
7. `R8`: v1.0 release hardening, signed artifacts, operator runbook.

Estimated remaining major iterations: **7**.

## Iteration Tracker

1. `R1` WebSocket mutation hooks + ws filter pipeline: `DONE`
2. `R2` Body capture with bounded memory and truncation policy: `IN_PROGRESS`
3. `R3` Replay v2 with body replay and diff report: `TODO`
4. `R4` Rule engine v2 operators + precedence model: `TODO`
5. `R5` Wasm mutating ABI v1 and hostcall contract: `TODO`
6. `R6` TLS trust diagnostics and bootstrap UX: `TODO`
7. `R7` Performance benchmarks and memory pressure tests: `TODO`
8. `R8` v1.0 release hardening and signed artifacts: `TODO`
