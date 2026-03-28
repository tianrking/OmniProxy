# OmniProxy 中文说明

English README: [README.md](./README.md)
Full Manual (EN): [FULL_MANUAL.md](./FULL_MANUAL.md)
完整手册（中文）: [FULL_MANUAL.zh-CN.md](./FULL_MANUAL.zh-CN.md)
Usage Cookbook (EN): [docs/USAGE_COOKBOOK.md](./docs/USAGE_COOKBOOK.md)
使用手册（中文实战）: [docs/USAGE_COOKBOOK.zh-CN.md](./docs/USAGE_COOKBOOK.zh-CN.md)
Master Plan (EN): [docs/MASTER_PLAN.md](./docs/MASTER_PLAN.md)
总体规划（中文）: [docs/MASTER_PLAN.zh-CN.md](./docs/MASTER_PLAN.zh-CN.md)
VPN 架构（英文）: [docs/VPN_ARCHITECTURE.md](./docs/VPN_ARCHITECTURE.md)
VPN 架构（中文）: [docs/VPN_ARCHITECTURE.zh-CN.md](./docs/VPN_ARCHITECTURE.zh-CN.md)
抓取定位（英文）: [docs/CAPTURE_POSITIONING.md](./docs/CAPTURE_POSITIONING.md)
抓取定位（中文）: [docs/CAPTURE_POSITIONING.zh-CN.md](./docs/CAPTURE_POSITIONING.zh-CN.md)

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

1. 剩余迭代：6~8 个版本（见 `docs/MASTER_PLAN.zh-CN.md`）
2. 每个里程碑按“代码 + 测试 + 中英文文档”同步交付
3. 每次功能提交必须同步更新计划文档

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
- `omni-replay` 支持时间窗与批量重访（`--since-ms` / `--until-ms` / `--batch-limit`）。
- `omni-replay` 支持编辑式回放：`--drop-header`、`--query`、`--body-text`、`--body-file`、`--interactive`。
- 请求/响应 body 有界捕获（基于 `Content-Length` 与阈值，二进制以 base64 记录）。
- 回放输出会展示“捕获响应 vs 实际回放响应”的差异摘要（状态码/字节数）。
- 回放差异已升级到 `header/body SHA-256` 哈希对比。
- body 捕获支持采样率（`--capture-body-sample-rate`）与压缩体策略（`--capture-body-compressed`）。
- flow 日志支持滚动与保留（`--flow-log-rotate-bytes` / `--flow-log-max-files`）。

4. TUI：
- 双窗格列表/详情。
- 全键盘操作（`j/k`、`g/G`、`/`、`c`、`q`、`r`、`x`）。
- DSL 过滤表达式。
- 支持 `r` 直接回放当前选中流量，`x` 一键隐藏 CONNECT 隧道噪音。
- 详情面板包含 `request_id`、时延、请求/响应 body 大小、捕获策略原因、WS 帧统计。
- 底栏显示 WS 连接状态与帧/字节总计。

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
cargo run --bin omni-tui -- --api ws://127.0.0.1:9091/ws
```

一键全局抓包入口（真实场景）：

```bash
# 本机全局抓包（macOS 可自动设置系统代理）
cargo run --bin omni-global -- --mode local --set-system-proxy
# 关闭系统代理恢复
cargo run --bin omni-global -- --unset-system-proxy
# 局域网网关模式（让其他设备走这台机器）
cargo run --bin omni-global -- --mode lan
# 叠加内核级 tcp/udp 元数据抓取（tcpdump 风格）
cargo run --bin omni-global -- --mode local --set-system-proxy --kernel-capture
```

透明重定向辅助（HTTP 首版）：

```bash
# 仅打印将执行的命令
cargo run --bin omni-transparent -- up
# 真正应用规则（需要 sudo 权限）
cargo run --bin omni-transparent -- up --apply
# 启动透明守护进程（处理 80/443）
cargo run --bin omni-transparentd -- --http-listen 127.0.0.1:10080 --https-listen 127.0.0.1:10443
# 清理规则
cargo run --bin omni-transparent -- down --apply
```

一键全栈（代理 + 透明重定向 + 内核侧车抓包）：

```bash
cargo run --bin omni-stack -- --mode local
# 叠加 macOS VPN 控制面（需提前存在 VPN 服务/配置）
cargo run --bin omni-stack -- --mode local --vpn --vpn-service-name "OmniProxy VPN"
```

说明：
- 从源码目录运行时，`omni-stack` 会自动补编译缺失的配套二进制（`omni-global`、`omni-transparent*`、`omni-vpn`）。
- VPN 模式下请使用 `cargo run --bin omni-vpn -- list` 中存在的服务名。

macOS 优先的 VPN 控制面（跨平台适配层）：

```bash
cargo run --bin omni-vpn -- list
cargo run --bin omni-vpn -- --service-name "OmniProxy VPN" doctor
cargo run --bin omni-vpn -- --service-name "OmniProxy VPN" up
cargo run --bin omni-vpn -- --service-name "OmniProxy VPN" status
cargo run --bin omni-vpn -- --service-name "OmniProxy VPN" down
cargo run --bin omni-vpn -- prepare
```

`omni-vpn` 在 macOS 的行为：
- 若 `--service-name` 存在于 `scutil --nc list`，则由 OmniProxy 控制该 VPN 服务。
- 若服务不存在，自动切换到 OmniProxy 自管模式：在 `--network-service`（默认 `Wi-Fi`）上把系统 HTTP/HTTPS 代理指向 `--local-http-proxy`。
- `omni-vpn prepare` 会在 `macos/OmniProxyPacketTunnelTemplate` 生成 PacketTunnel 模板文件。

说明：
- macOS 已支持自动系统代理接管。
- Linux（GNOME + `gsettings`）已支持自动系统代理接管。
- 若自动接管失败，`omni-global` 会继续运行，并输出 `set_proxy_hint` / `unset_proxy_hint` 供手动执行。

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

并发收敛压测（HTTP/1 + HTTP/2 偏好）：

```bash
cargo run --bin omni-converge -- --url https://example.com --requests 4000 --concurrency 256 --proxy http://127.0.0.1:9090
```

流量诊断分析：

```bash
cargo run --bin omni-analyze -- --flow-log ~/.omni-proxy/flows.jsonl --top 20 --slow-ms 800
cargo run --bin omni-analyze -- --flow-log ./.omni-proxy/flows.jsonl --include-connect
```

指定程序抓包入口（按进程注入代理环境）：

```bash
# 用注入后的代理环境运行单个程序
cargo run --bin omni-run -- -- curl -k https://httpbin.org/get
# 用生命周期托管系统代理运行程序（退出自动回滚）
cargo run --bin omni-run -- --mode system -- open -a "Safari"
# 开启进程 socket 时间线侧车
cargo run --bin omni-run -- --mode env --trace-sockets -- curl -k https://httpbin.org/get
# 仅预览将注入的参数，不执行
cargo run --bin omni-run -- --print-only -- curl https://example.com
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
2. `docs/RUNBOOK.md`（v1.0 运维与收敛检查清单）

## 后续重点

1. 回放 v2：请求体回放与结果 diff。
2. 规则引擎 v2：更强 DSL 与规则 lint。
3. Wasm ABI v1：可变更请求/响应的稳定插件接口。
4. TLS 运维增强：证书安装与诊断体验。
