# OmniProxy 中文说明

OmniProxy 是面向现代 API 开发者与安全分析师的高性能 MITM 代理核心，目标是：

1. 零依赖部署：单二进制运行。
2. 高性能抓包与劫持：HTTP(S)/WebSocket。
3. 可扩展：过滤器链 + Wasm 沙箱插件。
4. 极客体验：键盘优先 TUI + 后端实时事件 API。

核心能力总计划（权威清单）：
- `docs/CORE_FEATURE_PLAN.md`

## 当前已实现能力

1. 核心代理：
- HTTP 显式代理与 HTTPS `CONNECT` MITM。
- 动态 CA 证书初始化与签发。

2. 过滤器链：
- 请求 ID 注入（`x-omni-request-id`）。
- 规则引擎（deny / 请求头修改 / 响应头修改 / 响应状态覆盖 / 响应体替换）。
- Wasm 插件执行（隔离、失败不影响核心转发）。
- WebSocket 帧级观测（text/binary/ping/pong/close，预览可截断）。

3. 数据与回放：
- 流量事件通过 WebSocket API 实时输出。
- JSONL 持久化存储 flow。
- `omni-replay` 支持按索引/请求 ID 回放、header 覆盖、dry-run、curl 导出。

4. TUI：
- 双窗格列表/详情。
- 全键盘操作（`j/k`、`g/G`、`/`、`c`、`q`）。
- DSL 过滤表达式。

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

5. 启动 TUI：

```bash
cargo run --bin omni-tui -- --api ws://127.0.0.1:9091
```

WebSocket 预览截断长度：

```bash
cargo run -- --ws-preview-bytes 256
```

## 规则引擎

默认规则文件：`~/.omni-proxy/rules.txt`

支持动作：

1. `deny <expr>`
2. `req.set_header Header: Value if <expr>`
3. `res.set_header Header: Value if <expr>`
4. `res.set_status <code> if <expr>`
5. `res.replace_body "text" if <expr>`

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

## 后续重点

1. 回放 v2：请求体回放与结果 diff。
2. 规则引擎 v2：更强 DSL 与规则 lint。
3. Wasm ABI v1：可变更请求/响应的稳定插件接口。
4. TLS 运维增强：证书安装与诊断体验。
