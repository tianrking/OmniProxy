use crate::vpn::control::{VpnDoctorReport, VpnSpec, VpnStatus};
use anyhow::{Context, Result, bail};
use std::process::Command;

#[derive(Debug, Clone, Copy)]
pub enum PlatformKind {
    MacOs,
    Linux,
    Windows,
    Other,
}

pub fn detect_platform() -> PlatformKind {
    if cfg!(target_os = "macos") {
        PlatformKind::MacOs
    } else if cfg!(target_os = "linux") {
        PlatformKind::Linux
    } else if cfg!(target_os = "windows") {
        PlatformKind::Windows
    } else {
        PlatformKind::Other
    }
}

pub fn platform_name(kind: PlatformKind) -> &'static str {
    match kind {
        PlatformKind::MacOs => "macos",
        PlatformKind::Linux => "linux",
        PlatformKind::Windows => "windows",
        PlatformKind::Other => "other",
    }
}

pub fn up(spec: &VpnSpec) -> Result<()> {
    match detect_platform() {
        PlatformKind::MacOs => {
            ensure_macos_service_exists(spec)?;
            run_cmd("scutil", &["--nc", "start", &spec.service_name])?;
            Ok(())
        }
        PlatformKind::Linux | PlatformKind::Windows | PlatformKind::Other => {
            bail!("vpn up is not implemented on this platform yet (adapter boundary is ready)")
        }
    }
}

pub fn down(spec: &VpnSpec) -> Result<()> {
    match detect_platform() {
        PlatformKind::MacOs => {
            ensure_macos_service_exists(spec)?;
            run_cmd("scutil", &["--nc", "stop", &spec.service_name])?;
            Ok(())
        }
        PlatformKind::Linux | PlatformKind::Windows | PlatformKind::Other => {
            bail!("vpn down is not implemented on this platform yet (adapter boundary is ready)")
        }
    }
}

pub fn status(spec: &VpnSpec) -> Result<VpnStatus> {
    match detect_platform() {
        PlatformKind::MacOs => macos_status(spec),
        PlatformKind::Linux | PlatformKind::Windows | PlatformKind::Other => Ok(VpnStatus {
            platform: platform_name(detect_platform()).to_string(),
            service_name: spec.service_name.clone(),
            connected: false,
            raw_status: "status not implemented on this platform yet".into(),
        }),
    }
}

pub fn list_services() -> Result<Vec<String>> {
    match detect_platform() {
        PlatformKind::MacOs => {
            let raw = run_cmd_capture("scutil", &["--nc", "list"])?;
            Ok(parse_macos_nc_list(&raw))
        }
        PlatformKind::Linux | PlatformKind::Windows | PlatformKind::Other => Ok(Vec::new()),
    }
}

pub fn doctor(spec: &VpnSpec) -> Result<VpnDoctorReport> {
    match detect_platform() {
        PlatformKind::MacOs => {
            let services = list_services()?;
            let exists = services.iter().any(|s| s == &spec.service_name);
            let status_info = status(spec).ok();
            let connected = status_info.as_ref().is_some_and(|s| s.connected);

            let mut notes = vec![
                "macOS adapter uses `scutil --nc` control-plane facade".to_string(),
                "For full tunnel packet forwarding, PacketTunnelProvider app/profile must be installed and signed".to_string(),
            ];
            if !exists {
                notes.push(format!(
                    "VPN service `{}` not found; run `omni-vpn list` and choose an existing service name",
                    spec.service_name
                ));
            }

            Ok(VpnDoctorReport {
                platform: "macos".to_string(),
                adapter_ready: true,
                service_name: spec.service_name.clone(),
                service_exists: exists,
                connected,
                notes,
            })
        }
        PlatformKind::Linux | PlatformKind::Windows | PlatformKind::Other => {
            let platform = platform_name(detect_platform()).to_string();
            Ok(VpnDoctorReport {
                platform,
                adapter_ready: false,
                service_name: spec.service_name.clone(),
                service_exists: false,
                connected: false,
                notes: vec![
                    "Platform adapter is planned but not implemented yet".to_string(),
                    "Current stable path is explicit proxy / global system proxy mode".to_string(),
                ],
            })
        }
    }
}

fn macos_status(spec: &VpnSpec) -> Result<VpnStatus> {
    let out = Command::new("scutil")
        .args(["--nc", "status", &spec.service_name])
        .output()
        .with_context(|| format!("run scutil status {}", spec.service_name))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let raw = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    let connected = is_connected_status(&raw);
    Ok(VpnStatus {
        platform: "macos".into(),
        service_name: spec.service_name.clone(),
        connected,
        raw_status: raw.trim().to_string(),
    })
}

fn ensure_macos_service_exists(spec: &VpnSpec) -> Result<()> {
    let services = list_services()?;
    if services.iter().any(|s| s == &spec.service_name) {
        return Ok(());
    }
    let joined = if services.is_empty() {
        "(none)".to_string()
    } else {
        services.join(", ")
    };
    bail!(
        "vpn service `{}` not found. existing services: {}. use `omni-vpn list` or pass --service-name",
        spec.service_name,
        joined
    )
}

fn is_connected_status(raw: &str) -> bool {
    raw.contains("Connected") || raw.contains("connecting") || raw.contains("Connecting")
}

fn parse_macos_nc_list(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(extract_double_quoted)
        .collect::<Vec<_>>()
}

fn extract_double_quoted(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rem = &line[start + 1..];
    let end = rem.find('"')?;
    let value = rem[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("execute {} {}", cmd, args.join(" ")))?;
    if !status.success() {
        bail!("command failed: {} {}", cmd, args.join(" "));
    }
    Ok(())
}

fn run_cmd_capture(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("execute {} {}", cmd, args.join(" ")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!(
            "command failed: {} {}: {}",
            cmd,
            args.join(" "),
            stderr.trim()
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::{is_connected_status, parse_macos_nc_list};

    #[test]
    fn parse_nc_list_extracts_names() {
        let raw = r#"Available network connection services in the current set (*=enabled):
* (Disconnected)   "OmniProxy VPN"   [PPP]
* (Connected)      "Corp VPN"        [IPSec]
"#;
        let names = parse_macos_nc_list(raw);
        assert_eq!(
            names,
            vec!["OmniProxy VPN".to_string(), "Corp VPN".to_string()]
        );
    }

    #[test]
    fn status_connected_parser() {
        assert!(is_connected_status("Connected"));
        assert!(is_connected_status("Connecting"));
        assert!(!is_connected_status("Disconnected"));
    }
}
