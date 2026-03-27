# OmniProxy Full Manual (English)

中文版本: [FULL_MANUAL.zh-CN.md](./FULL_MANUAL.zh-CN.md)

## 1. What OmniProxy Provides

OmniProxy is a modern MITM toolkit with:

1. High-performance HTTP(S)/WebSocket interception.
2. Filter-chain traffic mutation.
3. Rule engine with declarative DSL.
4. Wasm plugin execution in isolation.
5. Flow capture, replay, diff analysis.
6. Keyboard-first TUI and WebSocket backend event API.
7. Cross-platform CI and release pipeline.

## 2. Binary Entry Points

1. `omni_proxy`: proxy core service.
2. `omni-tui`: terminal UI.
3. `omni-replay`: replay and compare captured requests.
4. `omni-bench`: benchmark helper.

## 3. Proxy Core (`omni_proxy`)

### 3.1 Start Proxy

```bash
cargo run --release -- --listen 127.0.0.1:9090
```

### 3.2 Core Network Parameters

1. `--listen` / `OMNI_LISTEN`: proxy listen address.
2. `--api-listen` / `OMNI_API_LISTEN`: backend WS API address.
3. `--api-max-lag` / `OMNI_API_MAX_LAG`: close lagging API clients after accumulated dropped events exceed the threshold.
4. `--log-level` / `OMNI_LOG`: log level.

### 3.3 CA and Bootstrap

1. `--bootstrap`: initialize CA/plugin/rule/flow files.
2. `--diagnose-ca`: inspect CA cert/key existence and parse health.
3. `--ca-cert`, `--ca-key`: custom CA file paths.

### 3.4 Rules and Validation

1. `--rule-file`: rule path.
2. `--check-rules`: parse and validate rules without launching proxy.

### 3.5 Body Capture and Log Strategy

1. `--capture-body-max-bytes`: max body bytes to capture.
2. `--capture-body-sample-rate`: capture sampling rate in `[0,1]`.
3. `--capture-body-compressed`: capture compressed payloads (disabled by default).
4. `--flow-log`: JSONL flow log path.
5. `--flow-log-rotate-bytes`: rotation threshold.
6. `--flow-log-max-files`: retained rotated files count.

### 3.6 WebSocket Mutation and Observability

1. `--ws-preview-bytes`: text preview truncation size.
2. `--ws-drop-ping`: drop incoming ping frames.
3. `--ws-text-rewrite "from=>to"`: repeatable text rewrite rule.

## 4. Rule DSL and Actions

### 4.1 Supported Fields

1. `req.method`
2. `req.host`
3. `req.uri`
4. `res.status`

### 4.2 Operators

1. `==`
2. `~=`
3. `starts_with`
4. `ends_with`
5. `matches` (regex-lite)
6. `>=`
7. `<=`
8. `&&`
9. `||`

### 4.3 Actions

1. `deny <expr>`
2. `req.set_header Header: Value if <expr>`
3. `res.set_header Header: Value if <expr>`
4. `res.set_status <code> if <expr>`
5. `res.replace_body "text" if <expr>`

Conflict policy:
1. For `res.set_status` and `res.replace_body`, first matched rule wins.

### 4.4 Example Rule File

```txt
deny req.method == "TRACE"
req.set_header X-Policy: strict if req.host ~= "internal.example.com"
res.set_header X-Scanned: true if res.status >= 400
res.set_status 451 if req.uri matches "^/restricted"
res.replace_body "blocked" if req.uri matches "^/restricted"
```

## 5. TUI (`omni-tui`)

### 5.1 Start

```bash
cargo run --bin omni-tui -- --api ws://127.0.0.1:9091
```

### 5.2 Keybindings

1. `j/k`: move selection.
2. `g/G`: first/last item.
3. `/`: enter filter input mode.
4. `c`: clear flows.
5. `r`: replay selected flow directly.
6. `q`: quit.

### 5.3 Filter Expressions

Use DSL expression such as:
1. `req.method == "POST" && res.status >= 400`
2. `req.uri starts_with "/api/"`
3. `req.host matches ".*example\\.com$"`

## 6. Replay (`omni-replay`)

### 6.1 Basic Commands

```bash
cargo run --bin omni-replay -- --list
cargo run --bin omni-replay -- --index 12
cargo run --bin omni-replay -- --request-id <id>
```

### 6.2 Replay Options

1. `--method-override`
2. `--url-override`
3. `--header "K: V"` (repeatable)
4. `--no-body`
5. `--dry-run`
6. `--print-curl`
7. `--session-client`
8. `--session-limit`

### 6.3 Diff Output

Replay prints captured-vs-live differences for:
1. response status
2. response bytes
3. response headers SHA-256
4. response body SHA-256

## 7. Benchmark (`omni-bench`)

### 7.1 Command

```bash
cargo run --bin omni-bench -- --url https://example.com --requests 2000 --concurrency 128 --proxy http://127.0.0.1:9090
```

### 7.2 Output Metrics

1. total success/failure count
2. elapsed time
3. requests per second
4. latency average
5. latency p50/p95/p99

## 8. API Event Stream

Subscribe:

```bash
websocat ws://127.0.0.1:9091
```

Event categories:
1. `HttpRequest`
2. `HttpResponse`
3. `WebSocketFrame`

## 9. CI and Release

1. CI workflow: `.github/workflows/ci.yml`
2. Release workflow: `.github/workflows/release.yml`
3. Release bundle includes:
   1. binaries
   2. `SHA256SUMS`
   3. `PROVENANCE.json`

## 10. API Contract Reference

1. `docs/API_CONTRACT.md` defines the backend event schema for `HttpRequest` / `HttpResponse` / `WebSocketFrame`.
