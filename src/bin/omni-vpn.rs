use anyhow::Result;
use clap::{Parser, Subcommand};
use omni_proxy::vpn::{
    control::VpnSpec,
    platform::{doctor, down, list_services, status, up},
};

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let spec = VpnSpec {
        service_name: cli.service_name,
        local_socks5: cli.local_socks5,
        local_http_proxy: cli.local_http_proxy,
        local_dns: cli.local_dns,
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
            println!("platform=macos-first");
            println!("goal=full-tunnel packet tunnel");
            println!(
                "required=Network Extension (PacketTunnelProvider) + Apple signing entitlement"
            );
            println!("service_name={}", spec.service_name);
            println!("next=install PacketTunnel app/profile then use `omni-vpn up`");
        }
    }

    Ok(())
}
