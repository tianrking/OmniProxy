# OmniProxy Usage Cookbook (EN)

中文版: [USAGE_COOKBOOK.zh-CN.md](./USAGE_COOKBOOK.zh-CN.md)

This document is task-oriented. Copy commands and run directly.

## 1. Local Bootstrap (macOS)

```bash
cd /Users/w0x7ce/Downloads/OOO/OmniProxy
cargo build --release
./target/release/omni_proxy --bootstrap
```

Trust generated CA:

```bash
sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain /Users/<you>/Downloads/OOO/OmniProxy/.omni-proxy/ca.crt
```

## 2. Start Core + TUI

Terminal 1:
```bash
./target/release/omni_proxy --listen 127.0.0.1:9090 --api-listen 127.0.0.1:9091
```

Terminal 2:
```bash
./target/release/omni-tui --api ws://127.0.0.1:9091/ws
```

Terminal 3 (generate traffic):
```bash
HTTPS_PROXY=http://127.0.0.1:9090 curl -k https://httpbin.org/get
HTTP_PROXY=http://127.0.0.1:9090 curl http://httpbin.org/get
```

## 3. TUI Analysis Workflow

1. Use `j/k` to select business request rows (`GET/POST...`).
2. Use `x` to hide CONNECT tunnel noise.
3. Use `/` with DSL filters, e.g.:
   - `req.method == "GET" && res.status >= 200`
   - `req.host == "httpbin.org"`
4. Press `r` to replay selected flow.
5. Detail pane includes request-id, latency, body sizes, capture reasons, WS counters.

## 4. Rule-Based Mutation

Edit `.omni-proxy/rules.txt`:

```txt
res.set_header X-Omni-Debug: on if req.host == "httpbin.org"
res.set_status 418 if req.uri matches ".*/status/200"
res.replace_body "blocked by policy" if req.uri matches ".*/status/200"
```

Validate rules:

```bash
./target/release/omni_proxy --check-rules --rule-file ./.omni-proxy/rules.txt
```

Restart proxy and verify:

```bash
HTTPS_PROXY=http://127.0.0.1:9090 curl -k https://httpbin.org/status/200 -i
```

## 5. Replay (Basic + Editable)

List:

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl --list
```

Replay by request-id:

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl --request-id <id>
```

Editable replay:

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl --index 1 \
  --drop-header Cookie \
  --query trace_id=dev \
  --body-text '{"debug":true}' \
  --dry-run --print-curl
```

Interactive edit:

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl --index 1 --interactive --print-curl
```

Session replay:

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl --session-client 127.0.0.1:54022 --session-limit 10
```

## 6. Offline Flow Analysis

```bash
./target/release/omni-analyze --flow-log ./.omni-proxy/flows.jsonl --top 20 --slow-ms 800
./target/release/omni-analyze --flow-log ./.omni-proxy/flows.jsonl --include-connect
```

Output includes:
1. total and error rate
2. latency p50/p95/p99
3. top host/status/method distributions
4. slow-request ranking
5. ws frame/byte totals

## 7. Wasm Plugin Quick Test

Put wasm plugin into `.omni-proxy/plugins/` and restart proxy.

Example mutating response contract (JSON returned by plugin):

```json
{"add_headers":[["x-plugin","on"]],"set_status":418,"replace_body":"rewritten by wasm"}
```

## 8. Common Troubleshooting

1. TUI `0/0` flows:
   - confirm `--api ws://127.0.0.1:9091/ws`
   - confirm proxy log has `request client=...`
   - clear filter and press `x` to show all
2. CA trust failed:
   - ensure cert path is repo-local `.omni-proxy/ca.crt` if bootstrap was run in repo cwd
3. Replay index not found:
   - run `--list` again and use existing index

## 9. Production Baseline Checklist

1. `--capture-body-max-bytes`, `--capture-body-sample-rate` tuned
2. flow log rotation configured (`--flow-log-rotate-bytes`, `--flow-log-max-files`)
3. ws lag limit configured (`--api-max-lag`)
4. rules validated (`--check-rules`) before deployment
5. run benchmark (`omni-bench` + `omni-converge`) per release
