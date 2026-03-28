# OmniProxy

OmniProxy is a modern MITM proxy core for API debugging and security analysis.

中文文档: [README.zh-CN.md](./README.zh-CN.md)
Full Manual (EN): [FULL_MANUAL.md](./FULL_MANUAL.md)
完整手册（中文）: [FULL_MANUAL.zh-CN.md](./FULL_MANUAL.zh-CN.md)
Usage Cookbook (EN): [docs/USAGE_COOKBOOK.md](./docs/USAGE_COOKBOOK.md)
使用手册（中文实战）: [docs/USAGE_COOKBOOK.zh-CN.md](./docs/USAGE_COOKBOOK.zh-CN.md)
Master Plan (EN): [docs/MASTER_PLAN.md](./docs/MASTER_PLAN.md)
总体规划（中文）: [docs/MASTER_PLAN.zh-CN.md](./docs/MASTER_PLAN.zh-CN.md)

Cross-platform target:

- Linux (`amd64`, `arm64`, `arm32`)
- Windows (`x64`)
- macOS (`x64`, `arm64`)

CI includes multi-target build and `.deb` packaging.

Core implementation checklist:
- `docs/CORE_FEATURE_PLAN.md`

Release hardening:
- `.github/workflows/release.yml` builds release bundle + `SHA256SUMS` + `PROVENANCE.json`.
- release workflow also signs `SHA256SUMS` using keyless sigstore (`SHA256SUMS.sig` + `SHA256SUMS.pem`).

Rules engine:

- default file: `~/.omni-proxy/rules.txt`
- expression syntax follows built-in DSL (e.g. `req.method == "TRACE"`)
- each non-empty non-comment line (`#`) is a deny rule
- supported request fields now include `req.method`, `req.host`, `req.uri`
- string contains operator: `~=`
- string prefix/suffix operators: `starts_with`, `ends_with`
- regex-lite operator: `matches` (e.g. `req.uri matches "^/api/v[0-9]+/users$"`)
- action syntax:
  - `deny <expr>`
  - `req.set_header Header: Value if <expr>`
  - `res.set_header Header: Value if <expr>`
  - `res.set_status 4xx/5xx if <expr>`
  - `res.replace_body "text" if <expr>`
  - conflict strategy for `res.set_status`/`res.replace_body`: first matched rule wins

Current phase (core-first, no UI):

- Explicit HTTP proxy + HTTPS `CONNECT` interception via MITM
- Dynamic certificate issuance with local CA bootstrap
- Filter-chain architecture inspired by Envoy/Pingora
- Wasm plugin host (Wasmtime) with request/response hooks
- WebSocket forwarding support through the underlying proxy engine
- WebSocket frame-level observability via backend event API (text/binary/ping/pong/close)

## Quick Start

1. Run:

```bash
cargo run --release -- --listen 127.0.0.1:9090
```

2. Configure your client/system proxy to `127.0.0.1:9090`.

3. Trust the generated CA certificate:

- Certificate path: `~/.omni-proxy/ca.crt`
- Key path: `~/.omni-proxy/ca.key`

4. Subscribe backend event stream (for future UI):

```bash
websocat ws://127.0.0.1:9091
```

Slow-consumer protection for backend WS API:
- `--api-max-lag` / `OMNI_API_MAX_LAG` (default `8192`)

WebSocket frame preview truncation can be tuned via:
- `--ws-preview-bytes`
- env: `OMNI_WS_PREVIEW_BYTES`

WebSocket active mutation controls:
- `--ws-drop-ping` (drop incoming ping frames)
- `--ws-text-rewrite "from=>to"` (repeatable text rewrite rule)

5. Run geek-first TUI (first iteration):

```bash
cargo run --bin omni-tui -- --api ws://127.0.0.1:9091/ws
```

Global-capture helper (one command, real-world workflow):

```bash
# local machine global capture (macOS auto system proxy optional)
cargo run --bin omni-global -- --mode local --set-system-proxy
# restore system proxy
cargo run --bin omni-global -- --unset-system-proxy
# LAN gateway mode for other devices
cargo run --bin omni-global -- --mode lan
```

Note:
- macOS supports auto system proxy setup now.
- Linux (GNOME with `gsettings`) supports auto system proxy setup now.
- If auto setup fails, `omni-global` continues running and prints `set_proxy_hint` / `unset_proxy_hint`.

6. Flow persistence (JSONL) is on by default:

- `~/.omni-proxy/flows.jsonl`
- override with `--flow-log /path/to/flows.jsonl`
- bounded body capture for events/logging: `--capture-body-max-bytes` (default `65536`)
- body capture sampling: `--capture-body-sample-rate` (`0.0~1.0`, default `1.0`)
- skip compressed body capture by default; enable with `--capture-body-compressed`
- log rotation: `--flow-log-rotate-bytes` (default `134217728`) and `--flow-log-max-files` (default `5`)

7. Replay v1 from persisted flows:

```bash
cargo run --bin omni-replay -- --list
cargo run --bin omni-replay -- --index 12
cargo run --bin omni-replay -- --request-id 4d3a... --header "Authorization: Bearer xxx"
cargo run --bin omni-replay -- --index 12 --dry-run --print-curl
cargo run --bin omni-replay -- --index 12 --no-body
cargo run --bin omni-replay -- --session-client 127.0.0.1:54022 --session-limit 10
cargo run --bin omni-replay -- --index 12 --drop-header Cookie --query trace_id=dev --body-text '{"debug":true}'
cargo run --bin omni-replay -- --index 12 --interactive --print-curl
cargo run --bin omni-replay -- --exclude-connect --since-ms 1774671000000 --until-ms 1774672000000 --batch-limit 20 --dry-run
```

Replay output now includes captured-vs-live diff for:
- status code
- body size
- response headers SHA-256 hash
- response body SHA-256 hash

10. Quick benchmark helper:

```bash
cargo run --bin omni-bench -- --url https://example.com --requests 2000 --concurrency 128 --proxy http://127.0.0.1:9090
```

12. Concurrency convergence runner (http1 + http2 preference):

```bash
cargo run --bin omni-converge -- --url https://example.com --requests 4000 --concurrency 256 --proxy http://127.0.0.1:9090
```

13. Flow diagnostics analyzer:

```bash
cargo run --bin omni-analyze -- --flow-log ~/.omni-proxy/flows.jsonl --top 20 --slow-ms 800
cargo run --bin omni-analyze -- --flow-log ./.omni-proxy/flows.jsonl --include-connect
```

8. Validate rules before starting proxy:

```bash
cargo run --bin omni_proxy -- --check-rules
cargo run --bin omni_proxy -- --check-rules --rule-file ./examples/rules-ci.txt
```

9. Diagnose local CA trust material quickly:

```bash
cargo run --bin omni_proxy -- --diagnose-ca
```

11. One-command local bootstrap:

```bash
cargo run --bin omni_proxy -- --bootstrap
```

Rule file example:

```txt
# Block dangerous methods
req.method == "TRACE"
req.method == "CONNECT"
# Block specific target host/path
req.host ~= "internal.example.com"
req.uri ~= "/admin"
# Mutate request/response headers by policy
req.set_header X-Omni-Policy: strict if req.host ~= "internal.example.com"
res.set_header X-Omni-Scanned: true if res.status >= 400
# Override response status and body (for mock/blocking workflows)
res.set_status 451 if req.uri ~= "/geo-restricted"
res.replace_body "blocked by policy" if req.uri ~= "/geo-restricted"
```

## Plugin Directory

Default plugin directory: `~/.omni-proxy/plugins`

Any `*.wasm` file in this directory is loaded on startup.

Current ABI (v0):

- export memory: `memory`
- export alloc: `alloc(i32) -> i32`
- export dealloc: `dealloc(i32, i32) -> ()`
- optional hook: `on_http_request(i32, i32) -> i32`
- optional hook: `on_http_response(i32, i32) -> i32`
- optional mutating hook: `on_http_request_mut(i32, i32) -> i64`
- optional mutating hook: `on_http_response_mut(i32, i32) -> i64`

The two hook functions receive a UTF-8 JSON payload pointer/length.
Return `0` for success; non-zero values are logged as warnings.

Mutating ABI (`*_mut`) return value:
- `0` means no mutation.
- non-zero packs output `(ptr, len)` into `i64`: `(ptr << 32) | len`.
- output bytes must be UTF-8 JSON:
  - request: `{"add_headers":[["x-k","v"]]}`
  - response: `{"add_headers":[["x-k","v"]],"set_status":418,"replace_body":"rewritten"}`

Wasm execution is isolated and fail-open:

- plugin timeout: `--wasm-timeout-ms` (default `20`)
- plugin failure budget: `--wasm-max-failures` (default `100`, plugin is skipped after threshold)
- timeout/trap/plugin error will be logged, but proxy core keeps running

## Filter Query DSL (Core)

Built-in parser skeleton supports expressions such as:

- `req.method == "POST" && res.status >= 400`
- `res.status >= 500 || req.method == "PUT"`

This parser is ready to be wired into TUI/API query filtering.

Current TUI supports:

- flow list + detail pane
- full keyboard navigation (`j/k`, `q`, `/`, `c`, `r`, `x`)
- inline declarative filtering via DSL
- replay selected flow with `r`
- hide CONNECT tunnel entries with `x`
- request/response correlation detail: `request_id`, latency, body sizes, capture reasons
- ws connection status and frame/bytes counters in footer

## Architecture

See [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md).
See [docs/API_CONTRACT.md](./docs/API_CONTRACT.md) for WS event contract.
See [docs/RUNBOOK.md](./docs/RUNBOOK.md) for v1.0 operational checklist.
See [docs/ROADMAP.md](./docs/ROADMAP.md) for phased delivery and iteration estimates.
