# OmniProxy 抓取定位（MITM 优先）

English Version: [CAPTURE_POSITIONING.md](./CAPTURE_POSITIONING.md)

## 1. 产品定位

OmniProxy 定位为 **指定目标的 MITM 分析平台**，不是强制全机 VPN。

核心目标：
1. 对指定应用/进程工作流实现高覆盖抓取
2. 深度查看/篡改/回放 HTTP(S)/WebSocket
3. 保持部署简单、行为可预测、可回滚

## 2. 双模式抓取

### 模式 A：`env`（进程环境注入）

入口：`omni-run --mode env -- <cmd>`

特性：
1. 仅对目标进程注入 `HTTP_PROXY/HTTPS_PROXY/ALL_PROXY`
2. 适合 CLI 与大量开发工具
3. 无系统级副作用

### 模式 B：`system`（生命周期托管系统代理）

入口：`omni-run --mode system -- <cmd>`

特性：
1. 启动目标程序前开启系统代理，退出自动回滚
2. 适合读取系统代理的 GUI 程序
3. 以命令生命周期为边界，具备回滚保护

## 3. 深入分析侧车

入口：`omni-run --trace-sockets --trace-file .omni-proxy/process_flows.jsonl -- <cmd>`

特性：
1. 通过 `lsof -p <pid> -i` 采样目标进程 socket 活动
2. 生成时间线 JSONL，补齐非 HTTP 通道可见性
3. 与 Omni MITM payload 视图形成互补

## 4. 当前非目标

1. 默认强制接管整机所有协议流量
2. 在不走 Network Extension/TUN 时宣称 100% UDP/DNS 全捕获

## 5. 架构摘要

1. 接入核心：`omni_proxy` / `omni-global`
2. 指定程序抓取入口：`omni-run`
3. 控制面辅助：`omni-vpn`（自管模式 + PacketTunnel 脚手架）
4. 分析面：TUI + WS API + JSONL + replay 工具链
