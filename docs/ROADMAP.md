# OmniProxy Roadmap

## Scope Summary

OmniProxy target capabilities:

1. Zero-dependency, cross-platform binaries (`Linux/Windows/macOS`)
2. High-performance HTTP(S)/WebSocket MITM core
3. Wasm sandbox plugin runtime with crash isolation
4. Envoy-like filter chain and policy engine
5. Geek-first UX: keyboard TUI and decoupled backend API
6. Flow storage, replay, and security analysis workflows

Detailed master checklist:
- `docs/CORE_FEATURE_PLAN.md` (authoritative parity and execution plan)

## Iteration Plan

Estimated total: **14 major iterations** (7 completed, 7 remaining).

Execution cadence:
- Major iterations remaining: `7` (`R2..R8`)
- Minor commits per major iteration: `2~4`
- Expected remaining commits: `14~28`
- Update contract: every feature commit must sync `docs/CORE_FEATURE_PLAN.md` + this roadmap + impacted Chinese doc.

### Completed

### I1 (done)
- Core MITM skeleton
- Filter-chain framework
- Wasm host (v0)

### I2 (done)
- Backend WebSocket event API
- First TUI (list/detail/filter input)

### I3 (done)
- Async JSONL flow persistence

### I4 (done)
- Replay engine v1 (list/replay-by-index/request-id/header override)
- Replay dry-run and `curl` export

### I5 (done)
- Rule router v1 (deny + req/res header mutation)
- Response policy actions in rule engine (`res.set_status`, `res.replace_body`)

### I6 (done)
- Cross-platform CI baseline (`linux amd64/arm64/arm32`, `windows x64`, `.deb`)
- CI validation with format/check/test gates

### Remaining

### I7
- Replay v2:
  - Capture and replay request body (json/form/binary-safe)
  - Better request/response pairing for keep-alive and multiplexed traffic
  - Replay result diff view (status/header/body hash)
  - status: request/response body bounded capture (event/log base64) 已接入
  - status+: replay 已支持 body 重放及捕获响应差异摘要（status/body-bytes）
  - status++: body 捕获采样率、压缩策略与 flow 日志轮转保留已接入
  - status+++: replay 差异报告已升级到 header/body hash
  - status++++: WS API 慢消费者背压阈值（`--api-max-lag`）已接入
  - status+++++: replay 已支持按 client 的会话式顺序回放（session helpers）
- Exit criteria:
  - `omni-replay` can replay captured body with deterministic request-id correlation

### I8
- Rule engine v2:
  - More expressive DSL (`starts_with`, `ends_with`, regex-lite)
  - Action precedence and conflict policy
  - Rule lint/check subcommand
  - status: `starts_with` / `ends_with` 已接入 DSL
  - status+: `matches` regex-lite 已接入，`res.set_status/res.replace_body` 冲突采用 first-match-wins
- Exit criteria:
  - `omni_proxy --check-rules` validates files and prints actionable diagnostics

### I9
- Wasm ABI v1:
  - Mutating request/response hooks
  - Structured hostcalls (log, kv, metrics, reject)
  - ABI versioning contract
- Exit criteria:
  - One sample plugin can modify request and response safely

### I10
- Wasm hardening:
  - Per-plugin timeout
  - CPU/memory/failure budget
  - Panic/fault isolation metrics
- Exit criteria:
  - Plugin timeout/fault does not block core forwarding path

### I11
- TUI v2:
  - Split panes with focus model
  - Search/filter history
  - Replay trigger from selected flow
  - status: `docs/API_CONTRACT.md` 已落地，后端事件结构对外可直接对接
- Exit criteria:
  - Full keyboard workflow from capture -> filter -> replay

### I12
- TLS/CA operations:
  - CA install helpers (macOS/Linux/Windows docs + commands)
  - Trust diagnostics and verification command
  - Cert cache visibility
  - status: `--diagnose-ca` 已接入（证书存在性/尺寸/可解析性检查）
  - status+: `--bootstrap` 已接入（一键初始化 CA/插件目录/规则与流量文件）
- Exit criteria:
  - Users can validate local trust chain with one command

### I13
- Security analyst toolkit:
  - Match/replace presets
  - Payload templates
  - Structured audit export bundle
- Exit criteria:
  - Analyst can run repeatable manipulation profile and export evidence

### I14
- Stabilization and release:
  - Benchmark suite (latency/memory/concurrency)
  - Release automation and signed artifacts
  - Operator docs and troubleshooting runbook
  - status: `omni-bench` 基准工具已接入（requests/concurrency/rps/latency p50/p95/p99）
  - status+: release workflow 已接入 SHA256SUMS 与 PROVENANCE.json 产出
- Exit criteria:
  - v1.0.0 release candidate with reproducible CI artifacts

## Full Work Breakdown

1. Core protocol:
- HTTP/2 stream correctness and header normalization.
- WebSocket frame interception and optional mutation.
2. Data pipeline:
- Backpressure strategy for API/TUI/event sinks.
- Retention and compaction for stored flow logs.
3. Rule/Wasm extensibility:
- Stable public contracts (DSL and ABI) before ecosystem growth.
- Compatibility tests across plugin versions.
4. UX/Operations:
- Proxy bootstrap command, cert trust diagnosis, operator troubleshooting.
5. Delivery:
- Benchmark gating in CI and deterministic release packaging.

## Risks and Cost Drivers

- Cross-compiling ARM targets with native TLS stacks can require linker/toolchain tuning.
- Wasm mutation ABI design affects long-term plugin compatibility.
- Replay correctness for complex streaming/WebSocket flows needs multiple refinement cycles.

## Delivery Strategy

- One feature set per version, always commit + push.
- Keep each iteration buildable and CI-green before moving forward.
- Prioritize core correctness and extensibility over UI polish.
