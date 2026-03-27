# OmniProxy 完整功能手册（中文）

English Version: [FULL_MANUAL.md](./FULL_MANUAL.md)

## 1. 功能总览

OmniProxy 提供以下核心能力：

1. 高性能 HTTP(S)/WebSocket MITM 劫持。
2. 可插拔过滤器链流量改写。
3. 声明式规则引擎（DSL）。
4. Wasm 沙箱插件执行。
5. 流量捕获、回放与差异对比。
6. 键盘优先 TUI 与后端 WS 事件 API。
7. 跨平台 CI 与发布流水线。

## 2. 可执行入口

1. `omni_proxy`：核心代理服务。
2. `omni-tui`：终端界面。
3. `omni-replay`：回放与差异分析。
4. `omni-bench`：基准压测工具。

## 3. 核心代理（`omni_proxy`）

### 3.1 启动代理

```bash
cargo run --release -- --listen 127.0.0.1:9090
```

### 3.2 网络参数

1. `--listen` / `OMNI_LISTEN`：代理监听地址。
2. `--api-listen` / `OMNI_API_LISTEN`：后端事件 API 地址。
3. `--api-max-lag` / `OMNI_API_MAX_LAG`：慢消费客户端累计掉包超过阈值后自动断开。
4. `--log-level` / `OMNI_LOG`：日志级别。

### 3.3 CA 与初始化

1. `--bootstrap`：一键初始化 CA/插件目录/规则/流量文件。
2. `--diagnose-ca`：CA 文件健康诊断（存在性/大小/可解析性）。
3. `--ca-cert`、`--ca-key`：自定义 CA 路径。

### 3.4 规则与预检

1. `--rule-file`：规则文件路径。
2. `--check-rules`：不启动代理，仅校验规则文件。

### 3.5 Body 捕获与日志策略

1. `--capture-body-max-bytes`：body 捕获上限。
2. `--capture-body-sample-rate`：body 捕获采样率（`0~1`）。
3. `--capture-body-compressed`：是否捕获压缩体（默认关闭）。
4. `--flow-log`：JSONL 流量文件路径。
5. `--flow-log-rotate-bytes`：滚动阈值。
6. `--flow-log-max-files`：滚动保留数量。

### 3.6 WebSocket 篡改与观测

1. `--ws-preview-bytes`：文本预览截断长度。
2. `--ws-drop-ping`：丢弃 ping 帧。
3. `--ws-text-rewrite "from=>to"`：文本替换规则（可重复）。

## 4. 规则 DSL 与动作

### 4.1 支持字段

1. `req.method`
2. `req.host`
3. `req.uri`
4. `res.status`

### 4.2 支持操作符

1. `==`
2. `~=`
3. `starts_with`
4. `ends_with`
5. `matches`（regex-lite）
6. `>=`
7. `<=`
8. `&&`
9. `||`

### 4.3 支持动作

1. `deny <expr>`
2. `req.set_header Header: Value if <expr>`
3. `res.set_header Header: Value if <expr>`
4. `res.set_status <code> if <expr>`
5. `res.replace_body "text" if <expr>`

冲突处理策略：
1. `res.set_status` 与 `res.replace_body` 采用“首条命中优先（first-match-wins）”。

### 4.4 规则示例

```txt
deny req.method == "TRACE"
req.set_header X-Policy: strict if req.host ~= "internal.example.com"
res.set_header X-Scanned: true if res.status >= 400
res.set_status 451 if req.uri matches "^/restricted"
res.replace_body "blocked" if req.uri matches "^/restricted"
```

## 5. TUI（`omni-tui`）

### 5.1 启动

```bash
cargo run --bin omni-tui -- --api ws://127.0.0.1:9091
```

### 5.2 快捷键

1. `j/k`：上下移动。
2. `g/G`：首条/末条。
3. `/`：进入过滤输入。
4. `c`：清空流量。
5. `q`：退出。

### 5.3 过滤表达式示例

1. `req.method == "POST" && res.status >= 400`
2. `req.uri starts_with "/api/"`
3. `req.host matches ".*example\\.com$"`

## 6. 回放（`omni-replay`）

### 6.1 基础命令

```bash
cargo run --bin omni-replay -- --list
cargo run --bin omni-replay -- --index 12
cargo run --bin omni-replay -- --request-id <id>
```

### 6.2 回放参数

1. `--method-override`
2. `--url-override`
3. `--header "K: V"`（可重复）
4. `--no-body`
5. `--dry-run`
6. `--print-curl`

### 6.3 差异输出

回放会输出“捕获响应 vs 当前回放响应”对比：

1. 状态码
2. body 字节数
3. 响应头 SHA-256
4. 响应体 SHA-256

## 7. 压测（`omni-bench`）

### 7.1 命令示例

```bash
cargo run --bin omni-bench -- --url https://example.com --requests 2000 --concurrency 128 --proxy http://127.0.0.1:9090
```

### 7.2 输出指标

1. 成功/失败数量。
2. 总耗时。
3. RPS。
4. 平均延迟。
5. p50/p95/p99 延迟。

## 8. 后端事件 API

订阅方式：

```bash
websocat ws://127.0.0.1:9091
```

事件类型：

1. `HttpRequest`
2. `HttpResponse`
3. `WebSocketFrame`

## 9. CI 与发布

1. CI：`.github/workflows/ci.yml`
2. 发布：`.github/workflows/release.yml`
3. 发布包包含：
   1. 二进制产物
   2. `SHA256SUMS`
   3. `PROVENANCE.json`
