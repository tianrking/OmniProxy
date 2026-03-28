# OmniProxy Master Plan (EN)

中文版: [MASTER_PLAN.zh-CN.md](./MASTER_PLAN.zh-CN.md)

This is the single execution plan for driving OmniProxy to a polished v1.0.

## 1. Delivery Principles

1. Every milestone lands with code + test + docs in same PR/commit batch.
2. Keep core path fast and simple: proxy -> observe -> analyze -> replay -> mutate.
3. Backward compatibility first for CLI flags and event contracts.
4. Any runtime-risk feature must include guardrails (timeout, budget, fallback path).

## 2. Current Capability Snapshot

Done:
1. HTTPS MITM and dynamic CA bootstrap.
2. HTTP/1.1 + HTTP/2 traffic capture and WS event API.
3. Rule engine (header/status/body actions, regex-lite, first-match-wins).
4. Replay CLI with editable/interactive modes and diff hashing.
5. TUI keyboard workflow with replay and CONNECT noise suppression.
6. Wasm v1 mutating hooks (request/response mutation payload).
7. Baseline benchmarks (`omni-bench`, `omni-converge`).
8. Release hardening baseline (checksums/provenance/signatures).

In progress / to close:
1. True streaming body path for unknown-length sustained traffic.
2. HTTP/1.1 + HTTP/2 correctness convergence under larger stress matrix.
3. Wasm ABI compatibility matrix and stronger resource budget metrics.
4. v1.0 release runbook completion and evidence checklist automation.

## 3. Milestones and Iteration Estimate

Planned remaining iterations: **6-8** (if no major architecture reset).

### M1 (1 iteration): Streaming Stability Finalization

Scope:
1. unknown-length body streaming policy refinement.
2. slow-consumer backpressure behavior tests.
3. memory envelope checks under sustained large payload.
4. global-capture adapter track: system-proxy one-command hardening now, transparent TUN adapter design/prototype.

Acceptance:
1. no OOM/leak on 30+ min sustained stream test.
2. bounded memory with capture truncation/sampling enabled.

### M2 (1-2 iterations): Protocol Correctness Convergence

Scope:
1. expand `omni-converge` scenarios (mixed methods, keep-alive stress, varied response sizes).
2. deterministic request-response correlation validation.
3. explicit failure signatures and regression thresholds.

Acceptance:
1. stable error rate target met for both http1/http2pref modes.
2. p95/p99 drift within configured threshold across repeated runs.

### M3 (1-2 iterations): Wasm v1 Hardening

Scope:
1. ABI compatibility suite across plugin payload variants.
2. stricter CPU/memory/failure budget telemetry.
3. safe-fallback mode when plugin repeatedly fails.

Acceptance:
1. compatibility tests pass with historical fixture payloads.
2. plugin failure cannot stall or crash core proxy path.

### M4 (1 iteration): Analyst UX Round

Scope:
1. TUI editable replay entrypoint.
2. session/time-window bulk revisit helpers.
3. richer detail pane shortcuts and operator hints.

Acceptance:
1. analyst can complete capture->filter->edit->replay loop without leaving terminal.

### M5 (1 iteration): Ops + Docs Completion

Scope:
1. runbook final validation matrix (macOS/Linux/Windows).
2. end-to-end deployment checklist with troubleshooting decision tree.
3. bilingual docs parity audit.

Acceptance:
1. new user can finish first successful MITM test in <10 minutes following docs only.

### M6 (1 iteration): v1.0 Freeze and Sign-off

Scope:
1. lock CLI/API contract versioning notes.
2. release candidate soak tests.
3. final signed artifacts and provenance evidence review.

Acceptance:
1. all release gates green.
2. no open P0/P1 defects in core path.

## 4. Definition of Done (Per Iteration)

1. Code merged and builds on CI matrix.
2. Automated tests added/updated and passing.
3. English + Chinese docs updated together.
4. Changelog summary present in commit message.
5. Manual verification commands included in docs.

## 5. Risk Register

1. High-throughput regressions in body buffering path.
2. edge-case correlation mismatch on multiplexed traffic.
3. plugin ABI drift causing silent compatibility issues.
4. docs drift between English and Chinese copies.

Mitigation:
1. add reproducible load scenarios into converge/bench runners.
2. keep event-contract fixtures and ABI fixtures in tests.
3. enforce doc parity update in every feature commit.

## 6. Tracking Cadence

1. Update `docs/ROADMAP.md` and this file every feature batch.
2. Keep rolling commit cadence: small, testable, push-ready increments.
3. Tag milestone completion evidence in commit body/checklist notes.
