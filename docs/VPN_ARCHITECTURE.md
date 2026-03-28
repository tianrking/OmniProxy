# OmniProxy VPN Architecture (Cross-Platform, macOS First)

中文版本: [VPN_ARCHITECTURE.zh-CN.md](./VPN_ARCHITECTURE.zh-CN.md)

## 1. Target

Goal is true full-traffic takeover (not only 80/443):
1. default-route through tunnel/VPN
2. TCP/UDP/DNS flow visibility
3. HTTP/HTTPS deep MITM via Omni core
4. one-command up/down with rollback safety

## 2. Why Platform Adapters

macOS, Linux, Windows VPN stacks are fundamentally different:
1. macOS: Network Extension (`PacketTunnelProvider`) + entitlement/signing required
2. Linux: TUN + routing + iptables/nftables
3. Windows: WFP/WinTUN + service model

So architecture must isolate platform mechanics behind one control plane.

## 3. Control Plane (Implemented)

Code:
1. `src/vpn/control.rs`
2. `src/vpn/platform.rs`
3. `src/bin/omni-vpn.rs`

Contract:
1. `VpnSpec` (service name + local Omni endpoints)
2. `up/down/status` operations
3. platform adapter selected internally

## 4. macOS First Path (Current)

`omni-vpn` currently integrates with macOS VPN service control via `scutil --nc`:
1. `omni-vpn up`
2. `omni-vpn down`
3. `omni-vpn status`
4. `omni-vpn prepare` (prints PacketTunnel requirements)

This gives a stable control facade now, while PacketTunnel runtime is completed next.

## 5. Next macOS Milestones

1. Add PacketTunnelProvider project scaffold and signed app wrapper.
2. Bind tunnel packet IO to Omni core ingress.
3. Add automatic route setup and safe rollback.
4. Integrate DNS handling into tunnel path.

## 6. Linux/Windows Follow-up

With control-plane stabilized:
1. Linux adapter plugs TUN engine under same `up/down/status` API.
2. Windows adapter plugs WFP/WinTUN under same `up/down/status` API.
3. Higher-level CLI (`omni-stack`) remains unchanged.
