# OmniProxy VPN 架构（跨平台，macOS 优先）

English Version: [VPN_ARCHITECTURE.md](./VPN_ARCHITECTURE.md)

## 1. 目标

目标是实现“真正全局接管”，而不是只覆盖 80/443：
1. 默认路由经过隧道/VPN
2. TCP/UDP/DNS 全量可观测
3. HTTP/HTTPS 深度 MITM 由 Omni 核心处理
4. 一键 up/down，且具备回滚安全

## 2. 为什么要做平台适配层

macOS、Linux、Windows 的 VPN 机制本质不同：
1. macOS：`Network Extension`（`PacketTunnelProvider`）+ Apple 签名权限
2. Linux：TUN + 路由 + iptables/nftables
3. Windows：WFP/WinTUN + 服务化管理

所以必须先把“控制面”抽象稳定，再按平台实现“数据面”。

## 3. 控制面（已落地）

代码位置：
1. `src/vpn/control.rs`
2. `src/vpn/platform.rs`
3. `src/bin/omni-vpn.rs`

统一接口：
1. `VpnSpec`（服务名 + 本地 Omni 端点）
2. `up/down/status`
3. `list/doctor`

## 4. macOS 优先路径（当前）

`omni-vpn` 已通过 `scutil --nc` 接入 macOS VPN 服务控制：
1. `omni-vpn list`
2. `omni-vpn doctor`
3. `omni-vpn up`
4. `omni-vpn status`
5. `omni-vpn down`

当前价值：先把“可控、可诊断”的控制面稳定下来，再持续补齐 PacketTunnel 数据面。

## 5. 下一步（macOS）

1. 加入 `PacketTunnelProvider` 工程骨架与签名流程
2. 将 tunnel packet IO 接到 Omni ingress
3. 自动路由与安全回滚
4. DNS 进隧道链路

## 6. Linux/Windows 后续

在同一 `up/down/status/list/doctor` API 下：
1. Linux adapter 接入 TUN 数据面
2. Windows adapter 接入 WFP/WinTUN
3. 上层 CLI（`omni-stack`）保持不变
