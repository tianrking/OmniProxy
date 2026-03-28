# OmniProxy 使用手册（中文实战）

English Version: [USAGE_COOKBOOK.md](./USAGE_COOKBOOK.md)

本文按“具体任务”组织，命令可直接复制执行。

## 1. 本地初始化（macOS）

```bash
cd /Users/w0x7ce/Downloads/OOO/OmniProxy
cargo build --release
./target/release/omni_proxy --bootstrap
```

信任本地 CA：

```bash
sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain /Users/<你>/Downloads/OOO/OmniProxy/.omni-proxy/ca.crt
```

## 2. 启动核心代理与 TUI

终端 1：
```bash
./target/release/omni_proxy --listen 127.0.0.1:9090 --api-listen 127.0.0.1:9091
```

终端 2：
```bash
./target/release/omni-tui --api ws://127.0.0.1:9091/ws
```

终端 3（制造流量）：
```bash
HTTPS_PROXY=http://127.0.0.1:9090 curl -k https://httpbin.org/get
HTTP_PROXY=http://127.0.0.1:9090 curl http://httpbin.org/get
```

## 3. TUI 分析流程

1. `j/k` 选中业务请求行（`GET/POST...`）。
2. `x` 隐藏 CONNECT 隧道噪音。
3. `/` 输入 DSL 过滤，例如：
   - `req.method == "GET" && res.status >= 200`
   - `req.host == "httpbin.org"`
4. `r` 回放当前选中流量。
5. 右侧详情可查看 request-id、时延、body 大小、捕获原因、WS 计数。

## 4. 规则改包与拦截

编辑 `.omni-proxy/rules.txt`：

```txt
res.set_header X-Omni-Debug: on if req.host == "httpbin.org"
res.set_status 418 if req.uri matches ".*/status/200"
res.replace_body "blocked by policy" if req.uri matches ".*/status/200"
```

规则预检：

```bash
./target/release/omni_proxy --check-rules --rule-file ./.omni-proxy/rules.txt
```

重启代理并验证：

```bash
HTTPS_PROXY=http://127.0.0.1:9090 curl -k https://httpbin.org/status/200 -i
```

## 5. 回放（基础 + 可编辑）

列出可回放流量：

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl --list
```

按 request-id 回放：

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl --request-id <id>
```

可编辑回放：

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl --index 1 \
  --drop-header Cookie \
  --query trace_id=dev \
  --body-text '{"debug":true}' \
  --dry-run --print-curl
```

交互编辑回放：

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl --index 1 --interactive --print-curl
```

会话重访（批量）：

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl --session-client 127.0.0.1:54022 --session-limit 10
```

时间窗批量重访：

```bash
./target/release/omni-replay --flow-log ./.omni-proxy/flows.jsonl \
  --exclude-connect \
  --since-ms 1774671000000 \
  --until-ms 1774672000000 \
  --batch-limit 20 \
  --dry-run --print-curl
```

## 6. 离线流量分析

```bash
./target/release/omni-analyze --flow-log ./.omni-proxy/flows.jsonl --top 20 --slow-ms 800
./target/release/omni-analyze --flow-log ./.omni-proxy/flows.jsonl --include-connect
```

输出包含：
1. 总量与错误率
2. 延迟 p50/p95/p99
3. 主机/状态码/方法分布
4. 慢请求排行
5. WS 帧数/字节数统计

## 7. Wasm 插件快速验证

将 wasm 插件放入 `.omni-proxy/plugins/` 后重启代理。

响应改写返回 JSON 示例：

```json
{"add_headers":[["x-plugin","on"]],"set_status":418,"replace_body":"rewritten by wasm"}
```

## 8. 常见故障排查

1. TUI 显示 `0/0`：
   - 确认 `--api ws://127.0.0.1:9091/ws`
   - 确认代理日志出现 `request client=...`
   - 清空 filter，并按 `x` 切换 CONNECT 显示
2. CA 信任失败：
   - 若在仓库目录执行 bootstrap，证书路径是仓库内 `.omni-proxy/ca.crt`
3. 回放 index 不存在：
   - 先执行 `--list` 再选存在的索引

## 9. 生产基线检查

1. 调优 `--capture-body-max-bytes`、`--capture-body-sample-rate`
2. 配置日志轮转（`--flow-log-rotate-bytes` / `--flow-log-max-files`）
3. 配置慢消费者保护（`--api-max-lag`）
4. 部署前做规则预检（`--check-rules`）
5. 每个版本跑压测（`omni-bench` + `omni-converge`）
