# OmniProxy 中文说明

English README: [README.md](./README.md)
Full Manual (EN): [FULL_MANUAL.md](./FULL_MANUAL.md)
完整手册（中文）: [FULL_MANUAL.zh-CN.md](./FULL_MANUAL.zh-CN.md)

OmniProxy 是面向现代 API 开发者与安全分析师的高性能 MITM 代理核心，目标是：

1. 零依赖部署：单二进制运行。
2. 高性能抓包与劫持：HTTP(S)/WebSocket。
3. 可扩展：过滤器链 + Wasm 沙箱插件。
4. 极客体验：键盘优先 TUI + 后端实时事件 API。

核心能力总计划（权威清单）：
- `docs/CORE_FEATURE_PLAN.md`

发布硬化：
- `.github/workflows/release.yml` 已接入产物打包、`SHA256SUMS` 与 `PROVENANCE.json` 生成。
- release 流水线已接入 keyless sigstore 签名（`SHA256SUMS.sig` + `SHA256SUMS.pem`）。

迭代节奏（当前估算）：

1. 剩余大版本：7 个（R2..R8）
2. 每个大版本预计 2~4 次提交
3. 到 v1.0 范围预计还需 14~28 次提交
4. 每次功能提交必须同步更新计划与中文文档

## 当前已实现能力

1. 核心代理：
- HTTP 显式代理与 HTTPS `CONNECT` MITM。
- 动态 CA 证书初始化与签发。

2. 过滤器链：
- 请求 ID 注入（`x-omni-request-id`）。
- 规则引擎（deny / 请求头修改 / 响应头修改 / 响应状态覆盖 / 响应体替换）。
- Wasm 插件执行（隔离、失败不影响核心转发）。
- Wasm 插件支持失败预算熔断（超过阈值后自动跳过该插件）。
- Wasm 可变更 ABI 已接入（请求/响应头、响应状态、响应体修改）。
- WebSocket 帧级观测（text/binary/ping/pong/close，预览可截断）。

3. 数据与回放：
- 流量事件通过 WebSocket API 实时输出。
- JSONL 持久化存储 flow。
- `omni-replay` 支持按索引/请求 ID 回放、header 覆盖、dry-run、curl 导出。
- `omni-replay` 支持按 client 维度会话式顺序回放（`--session-client` / `--session-limit`）。
- 请求/响应 body 有界捕获（基于 `Content-Length` 与阈值，二进制以 base64 记录）。
- 回放输出会展示“捕获响应 vs 实际回放响应”的差异摘要（状态码/字节数）。
- 回放差异已升级到 `header/body SHA-256` 哈希对比。
- body 捕获支持采样率（`--capture-body-sample-rate`）与压缩体策略（`--capture-body-compressed`）。
- flow 日志支持滚动与保留（`--flow-log-rotate-bytes` / `--flow-log-max-files`）。

4. TUI：
- 双窗格列表/详情。
- 全键盘操作（`j/k`、`g/G`、`/`、`c`、`q`）。
- DSL 过滤表达式。
- 支持 `r` 直接回放当前选中流量。

## 快速开始

1. 启动代理：

```bash
cargo run --release -- --listen 127.0.0.1:9090
```

2. 客户端系统代理指向 `127.0.0.1:9090`。

3. 信任证书：
- `~/.omni-proxy/ca.crt`
- `~/.omni-proxy/ca.key`

4. 查看实时事件：

```bash
websocat ws://127.0.0.1:9091
```

后端 WS API 慢消费者保护：
- `--api-max-lag` / `OMNI_API_MAX_LAG`（默认 `8192`）

5. 启动 TUI：

```bash
cargo run --bin omni-tui -- --api ws://127.0.0.1:9091
```

WebSocket 预览截断长度：

```bash
cargo run -- --ws-preview-bytes 256
```

WebSocket 主动篡改开关：

```bash
cargo run -- --ws-drop-ping
cargo run -- --ws-text-rewrite "foo=>bar" --ws-text-rewrite "token=>[REDACTED]"
```

## 规则引擎

默认规则文件：`~/.omni-proxy/rules.txt`

支持动作：

1. `deny <expr>`
2. `req.set_header Header: Value if <expr>`
3. `res.set_header Header: Value if <expr>`
4. `res.set_status <code> if <expr>`
5. `res.replace_body "text" if <expr>`

表达式新增前后缀匹配：

1. `req.uri starts_with "/api/"`
2. `req.host ends_with ".internal"`

正则轻量匹配（regex-lite）：

1. `req.uri matches "^/api/v[0-9]+/users$"`

冲突处理策略：

1. `res.set_status` / `res.replace_body` 多条命中时采用“首条命中优先（first-match-wins）”。

示例：

```txt
deny req.method == "TRACE"
req.set_header X-Omni-Policy: strict if req.host ~= "internal.example.com"
res.set_header X-Omni-Scanned: true if res.status >= 400
res.set_status 451 if req.uri ~= "/geo-restricted"
res.replace_body "blocked by policy" if req.uri ~= "/geo-restricted"
```

规则预检（不启动代理）：

```bash
cargo run --bin omni_proxy -- --check-rules
cargo run --bin omni_proxy -- --check-rules --rule-file ./examples/rules-ci.txt
```

CA 证书诊断（不启动代理）：

```bash
cargo run --bin omni_proxy -- --diagnose-ca
```

一键本地初始化：

```bash
cargo run --bin omni_proxy -- --bootstrap
```

快速压测工具：

```bash
cargo run --bin omni-bench -- --url https://example.com --requests 2000 --concurrency 128 --proxy http://127.0.0.1:9090
```

## CI 与跨平台

当前 CI 覆盖：

1. Linux: `x86_64`, `aarch64`, `armv7`
2. Windows: `x86_64`
3. macOS: `x86_64`, `aarch64`
4. `.deb` 打包
5. `fmt` + `check` + `test` + 规则预检

## 架构分层

1. Data Ingest：tokio + rustls 接管连接与 TLS。
2. Protocol Brain：基于 hyper 处理 HTTP 协议。
3. Filter Chain：可插拔策略链，统一扩展点。
4. UX/API Shell：TUI 与 WebSocket API 解耦。

API 合同文档：
1. `docs/API_CONTRACT.md`

## 后续重点

1. 回放 v2：请求体回放与结果 diff。
2. 规则引擎 v2：更强 DSL 与规则 lint。
3. Wasm ABI v1：可变更请求/响应的稳定插件接口。
4. TLS 运维增强：证书安装与诊断体验。
