use anyhow::Result;
use clap::{Parser, Subcommand};
use omni_proxy::vpn::{
    control::VpnSpec,
    platform::{doctor, down, list_services, status, up},
};
use std::fs;
use std::path::Path;

#[derive(Debug, Subcommand)]
enum Cmd {
    Up,
    Down,
    Status,
    List,
    Doctor,
    Prepare,
}

#[derive(Debug, Parser)]
#[command(
    name = "omni-vpn",
    about = "Cross-platform VPN control plane (mac first)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    #[arg(long, default_value = "OmniProxy VPN")]
    service_name: String,

    #[arg(long, default_value = "127.0.0.1:1080")]
    local_socks5: String,

    #[arg(long, default_value = "127.0.0.1:9090")]
    local_http_proxy: String,

    #[arg(long, default_value = "127.0.0.1:5353")]
    local_dns: String,

    #[arg(long, default_value = "Wi-Fi")]
    network_service: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let spec = VpnSpec {
        service_name: cli.service_name,
        local_socks5: cli.local_socks5,
        local_http_proxy: cli.local_http_proxy,
        local_dns: cli.local_dns,
        network_service: cli.network_service,
    };

    match cli.cmd {
        Cmd::Up => {
            up(&spec)?;
            println!("vpn_up=true\nservice={}", spec.service_name);
        }
        Cmd::Down => {
            down(&spec)?;
            println!("vpn_down=true\nservice={}", spec.service_name);
        }
        Cmd::Status => {
            let s = status(&spec)?;
            println!(
                "platform={}\nservice={}\nconnected={}\nraw_status={}",
                s.platform, s.service_name, s.connected, s.raw_status
            );
        }
        Cmd::List => {
            let services = list_services()?;
            println!("service_count={}", services.len());
            for s in services {
                println!("service={}", s);
            }
        }
        Cmd::Doctor => {
            let r = doctor(&spec)?;
            println!(
                "platform={}\nadapter_ready={}\nservice={}\nservice_exists={}\nconnected={}",
                r.platform, r.adapter_ready, r.service_name, r.service_exists, r.connected
            );
            for note in r.notes {
                println!("note={}", note);
            }
        }
        Cmd::Prepare => {
            prepare_macos_template()?;
            println!("platform=macos-first");
            println!("goal=full-tunnel packet tunnel");
            println!(
                "required=Network Extension (PacketTunnelProvider) + Apple signing entitlement"
            );
            println!("service_name={}", spec.service_name);
            println!("template_dir=macos/OmniProxyPacketTunnelTemplate");
            println!(
                "next=build signed PacketTunnel app/profile from template then use `omni-vpn up`"
            );
        }
    }

    Ok(())
}

fn prepare_macos_template() -> Result<()> {
    let root = Path::new("macos/OmniProxyPacketTunnelTemplate");
    fs::create_dir_all(root)?;

    fs::write(
        root.join("PacketTunnelProvider.swift"),
        PACKET_TUNNEL_PROVIDER_SWIFT,
    )?;
    fs::write(root.join("Info.plist"), PACKET_TUNNEL_INFO_PLIST)?;
    fs::write(
        root.join("OmniProxyPacketTunnel.entitlements"),
        PACKET_TUNNEL_ENTITLEMENTS,
    )?;
    fs::write(root.join("README.md"), PACKET_TUNNEL_README)?;
    Ok(())
}

const PACKET_TUNNEL_PROVIDER_SWIFT: &str = r#"import Foundation
import NetworkExtension

final class PacketTunnelProvider: NEPacketTunnelProvider {
    override func startTunnel(options: [String : NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "198.18.0.1")
        settings.mtu = 1500

        let ipv4 = NEIPv4Settings(addresses: ["198.18.0.2"], subnetMasks: ["255.255.255.0"])
        ipv4.includedRoutes = [NEIPv4Route.default()]
        settings.ipv4Settings = ipv4

        let dns = NEDNSSettings(servers: ["1.1.1.1", "8.8.8.8"])
        dns.matchDomains = [""]
        settings.dnsSettings = dns

        let proxy = NEProxySettings()
        proxy.httpEnabled = true
        proxy.httpsEnabled = true
        proxy.httpServer = NEProxyServer(address: "127.0.0.1", port: 9090)
        proxy.httpsServer = NEProxyServer(address: "127.0.0.1", port: 9090)
        proxy.excludeSimpleHostnames = false
        proxy.matchDomains = [""]
        settings.proxySettings = proxy

        setTunnelNetworkSettings(settings) { [weak self] error in
            guard error == nil else {
                completionHandler(error)
                return
            }
            self?.startPacketPump()
            completionHandler(nil)
        }
    }

    override func stopTunnel(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        completionHandler()
    }

    private func startPacketPump() {
        packetFlow.readPackets { [weak self] packets, _ in
            guard let self = self else { return }
            if !packets.isEmpty {
                // TODO: Replace with real packet forwarding data plane (tun2socks / custom engine).
            }
            self.startPacketPump()
        }
    }
}
"#;

const PACKET_TUNNEL_INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>OmniProxy PacketTunnel</string>
  <key>CFBundleExecutable</key>
  <string>$(EXECUTABLE_NAME)</string>
  <key>CFBundleIdentifier</key>
  <string>com.tianrking.omniproxy.packet-tunnel</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>OmniProxyPacketTunnel</string>
  <key>CFBundlePackageType</key>
  <string>XPC!</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>NSExtension</key>
  <dict>
    <key>NSExtensionPointIdentifier</key>
    <string>com.apple.networkextension.packet-tunnel</string>
    <key>NSExtensionPrincipalClass</key>
    <string>$(PRODUCT_MODULE_NAME).PacketTunnelProvider</string>
  </dict>
</dict>
</plist>
"#;

const PACKET_TUNNEL_ENTITLEMENTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.developer.networking.networkextension</key>
  <array>
    <string>packet-tunnel-provider-systemextension</string>
  </array>
</dict>
</plist>
"#;

const PACKET_TUNNEL_README: &str = r#"# OmniProxy PacketTunnel Template (macOS)

This template is generated by:

```bash
cargo run --bin omni-vpn -- prepare
```

## What This Gives You

1. A PacketTunnelProvider Swift skeleton with default-route + proxy settings.
2. Entitlements and Info.plist baseline for Network Extension target.
3. A fast starting point for a signed app/extension delivery.

## What You Still Must Complete

1. Create an Xcode App + Packet Tunnel Extension target and copy these files in.
2. Set your Team ID and signing profile with Network Extension entitlement.
3. Replace `startPacketPump()` TODO with real packet forwarding data plane.
4. Build/install profile, then use `omni-vpn up`.
"#;
