# OmniProxy

OmniProxy is a modern MITM proxy core for API debugging and security analysis.

Cross-platform target:

- Linux (`amd64`, `arm64`, `arm32`)
- Windows (`x64`)
- macOS (local build support)

CI includes multi-target build and `.deb` packaging.

Rules engine:

- default file: `~/.omni-proxy/rules.txt`
- expression syntax follows built-in DSL (e.g. `req.method == "TRACE"`)
- each non-empty non-comment line (`#`) is a deny rule
- supported request fields now include `req.method`, `req.host`, `req.uri`
- string contains operator: `~=`
- action syntax:
  - `deny <expr>`
  - `req.set_header Header: Value if <expr>`
  - `res.set_header Header: Value if <expr>`
  - `res.set_status 4xx/5xx if <expr>`
  - `res.replace_body "text" if <expr>`

Current phase (core-first, no UI):

- Explicit HTTP proxy + HTTPS `CONNECT` interception via MITM
- Dynamic certificate issuance with local CA bootstrap
- Filter-chain architecture inspired by Envoy/Pingora
- Wasm plugin host (Wasmtime) with request/response hooks
- WebSocket forwarding support through the underlying proxy engine

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

5. Run geek-first TUI (first iteration):

```bash
cargo run --bin omni-tui -- --api ws://127.0.0.1:9091
```

6. Flow persistence (JSONL) is on by default:

- `~/.omni-proxy/flows.jsonl`
- override with `--flow-log /path/to/flows.jsonl`

7. Replay v1 from persisted flows:

```bash
cargo run --bin omni-replay -- --list
cargo run --bin omni-replay -- --index 12
cargo run --bin omni-replay -- --request-id 4d3a... --header "Authorization: Bearer xxx"
cargo run --bin omni-replay -- --index 12 --dry-run --print-curl
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

The two hook functions receive a UTF-8 JSON payload pointer/length.
Return `0` for success; non-zero values are logged as warnings.

Wasm execution is isolated and fail-open:

- plugin timeout: `--wasm-timeout-ms` (default `20`)
- timeout/trap/plugin error will be logged, but proxy core keeps running

## Filter Query DSL (Core)

Built-in parser skeleton supports expressions such as:

- `req.method == "POST" && res.status >= 400`
- `res.status >= 500 || req.method == "PUT"`

This parser is ready to be wired into TUI/API query filtering.

Current TUI supports:

- flow list + detail pane
- full keyboard navigation (`j/k`, `q`, `/`, `c`)
- inline declarative filtering via DSL

## Architecture

See [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md).
See [docs/ROADMAP.md](./docs/ROADMAP.md) for phased delivery and iteration estimates.
