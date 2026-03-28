# OmniProxy Capture Positioning (MITM-First)

中文版本: [CAPTURE_POSITIONING.zh-CN.md](./CAPTURE_POSITIONING.zh-CN.md)

## 1. Product Positioning

OmniProxy is positioned as a **targeted MITM analysis platform**, not a mandatory whole-machine VPN.

Primary goal:
1. capture all traffic of a designated target app/process workflow
2. inspect/mutate/replay HTTP(S)/WebSocket deeply
3. keep setup simple and deterministic for analysts/developers

## 2. Two Capture Modes

### Mode A: `env` (Process Env Injection)

Entry: `omni-run --mode env -- <cmd>`

Characteristics:
1. injects `HTTP_PROXY/HTTPS_PROXY/ALL_PROXY` for the target process only
2. safest for CLI tools and many developer programs
3. no system-wide side effects

### Mode B: `system` (Lifecycle-Scoped System Proxy)

Entry: `omni-run --mode system -- <cmd>`

Characteristics:
1. enables system proxy at start, auto-restores on exit
2. useful for GUI apps that read system proxy settings
3. scoped to command lifecycle, with rollback guard

## 3. Deep Analysis Sidecar

Entry: `omni-run --trace-sockets --trace-file .omni-proxy/process_flows.jsonl -- <cmd>`

Characteristics:
1. samples target process socket activity via `lsof -p <pid> -i`
2. records timeline JSONL for non-HTTP channels visibility
3. complements MITM payload view from Omni core

## 4. Non-Goals (Current Stage)

1. force-capture every protocol of entire machine by default
2. claim guaranteed 100% UDP/DNS capture without Network Extension/TUN path

## 5. Architecture Summary

1. Ingest core: `omni_proxy` / `omni-global`
2. Targeted launcher: `omni-run`
3. Control-plane helper: `omni-vpn` (self-managed + PacketTunnel scaffold path)
4. Analysis surfaces: TUI + WS API + JSONL + replay tools
